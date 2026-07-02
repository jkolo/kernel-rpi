//! Pure end-to-end sync planning — the full pipeline (model → BLS selection
//! → cmdline assembly → liveness-first slot resolution → idempotency check)
//! as one testable function, with all I/O results passed in as data. The
//! real I/O shell (`run()` in `lib.rs`) gathers these inputs for real and
//! executes the returned `SyncPlan`.

use crate::{bls, cmdline, context, fatsync, model, slot};

pub struct SyncInputs<'a> {
    /// Raw `/proc/device-tree/model` contents (NUL not yet stripped).
    pub model_raw: &'a str,
    /// `(path, contents)` for each `loader/entries/ostree-*.conf`, already
    /// sorted bytewise (LC_ALL=C glob order) by the caller.
    pub bls_entries_sorted: &'a [(&'a str, &'a str)],
    /// `/proc/cmdline` contents.
    pub proc_cmdline: &'a str,
    /// Whether `$BOOT_MOUNT/ignition.firstboot` exists.
    pub ignition_firstboot_present: bool,
    /// Contents of each `/etc/cmdline.d/*.conf` then `/sysroot/etc/cmdline.d/*.conf`
    /// file, in glob order (a file present under both prefixes appears twice,
    /// matching bash's double-read — dedup handles it).
    pub cmdline_d_contents_ordered: &'a [&'a str],
    /// Whether `<prefix>/ostree/deploy` exists, for `""` then `"/sysroot"`.
    pub deploy_root_prefixes_present: [bool; 2],
    /// Whether the GUARD's target directory (see `slot::target_after_adoption`)
    /// exists, for `""` then `"/sysroot"` — the caller must derive this
    /// directory from `target_after_adoption`'s result BEFORE doing this I/O
    /// check (see the `DeployProbe` caller contract in `slot.rs`).
    pub guard_target_dir_present_prefixes: [bool; 2],
    /// Fresh wall-clock in microseconds (RPi has no RTC).
    pub clock_usec: u64,
    /// EFI partition's current `cmdline.txt`, if present.
    pub current_cmdline_txt: Option<&'a str>,
    pub src_kernel_size: u64,
    pub dst_kernel_size: Option<u64>,
    pub src_initramfs_size: u64,
    pub dst_initramfs_size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncPlan {
    /// Nothing to do — log the reason and exit 0.
    Skip(&'static str),
    /// The slot guard refused — leave the last-good cmdline.txt untouched.
    RefuseExit0,
    /// Write kernel/initramfs/cmdline.txt (+ config.txt/DTB/firmware, wired
    /// in the real I/O layer) — this is the resolved `cmdline.txt` content.
    Sync {
        cmdline_txt: String,
        kernel_name: &'static str,
    },
}

/// Result of the part of the pipeline that must run BEFORE slot resolution,
/// so a real I/O caller can derive `target_after_adoption` and probe the
/// correct guard directory before calling `plan_sync`. See the
/// `DeployProbe` caller contract in `slot.rs`.
pub struct PreSlot {
    pub kernel_name: &'static str,
    pub cmdline: String,
    /// The winning BLS entry's `linux`/`initrd` paths — the real I/O layer
    /// copies these as the source kernel/initramfs.
    pub linux_path: String,
    pub initrd_path: String,
}

/// Model + BLS default-entry selection + cmdline assembly (firstboot expand,
/// cmdline.d append, dedup).
pub fn assemble_pre_slot_cmdline(
    model_raw: &str,
    bls_entries_sorted: &[(&str, &str)],
    ignition_firstboot_present: bool,
    cmdline_d_contents_ordered: &[&str],
) -> Result<PreSlot, &'static str> {
    let model = model::parse_model(model_raw).map_err(|_| "unsupported model")?;

    let entries: Vec<_> = bls_entries_sorted
        .iter()
        .map(|(_, contents)| bls::parse_entry(contents))
        .collect();
    let winning = bls::select_default(&entries).ok_or("no BLS entry found")?;

    let mut assembled = cmdline::expand_firstboot(&winning.options, ignition_firstboot_present);
    for extra in cmdline_d_contents_ordered {
        assembled = format!("{assembled} {extra}");
    }
    Ok(PreSlot {
        kernel_name: model.efi_filename(),
        cmdline: cmdline::dedup_tokens(&assembled),
        linux_path: winning.linux.clone(),
        initrd_path: winning.initrd.clone(),
    })
}

pub fn plan_sync(inputs: &SyncInputs) -> SyncPlan {
    let pre = match assemble_pre_slot_cmdline(
        inputs.model_raw,
        inputs.bls_entries_sorted,
        inputs.ignition_firstboot_present,
        inputs.cmdline_d_contents_ordered,
    ) {
        Ok(v) => v,
        Err(reason) => return SyncPlan::Skip(reason),
    };
    let (kernel_name, assembled) = (pre.kernel_name, pre.cmdline);

    let ctx = context::probe_deploy_root(&inputs.deploy_root_prefixes_present);
    let guard_dir_present = inputs.guard_target_dir_present_prefixes.iter().any(|&p| p);
    let probe = context::to_deploy_probe(ctx, guard_dir_present);

    let resolution = slot::resolve(&assembled, inputs.proc_cmdline, probe);
    let resolved = match resolution {
        slot::Resolution::RefuseExit0 => return SyncPlan::RefuseExit0,
        slot::Resolution::Rewrite(s) => s,
        slot::Resolution::Keep => assembled,
    };

    let final_cmdline = cmdline::inject_clock(&resolved, inputs.clock_usec);

    let current_stripped = inputs
        .current_cmdline_txt
        .map(cmdline::strip_clock)
        .unwrap_or_default();
    let new_stripped = cmdline::strip_clock(&final_cmdline);
    let sync_needed = fatsync::needs_sync(
        inputs.src_kernel_size,
        inputs.dst_kernel_size,
        inputs.src_initramfs_size,
        inputs.dst_initramfs_size,
        &current_stripped,
        &new_stripped,
    );
    if !sync_needed {
        return SyncPlan::Skip("already in sync");
    }

    SyncPlan::Sync {
        cmdline_txt: final_cmdline,
        kernel_name,
    }
}
