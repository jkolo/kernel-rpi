pub mod bls;
pub mod cmdline;
pub mod config_txt;
pub mod context;
pub mod fatsync;
pub mod firmware;
pub mod model;
pub mod mounts;
pub mod orchestrate;
pub mod slot;

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fatsync::{BootFs, PendingWrite, StdBootFs};
use mounts::BootMount;

// ---------------------------------------------------------------------------
// Real I/O orchestration shell. NOT covered by `cargo test` — it wires
// together mount(2)/std::fs calls that need root and a real (or loop) block
// device. `tests/qemu/run.sh` exercises the underlying mount(2)/StdBootFs
// primitives for real in a disposable QEMU guest (see
// examples/qemu_harness.rs + tests/qemu/README.md) — deliberately by
// calling `orchestrate::plan_sync` + `fatsync`/`mounts` directly rather
// than through this file's `run()`, so it needs no aarch64/device-tree
// emulation. This file's OWN glue specifically (reading
// `/proc/device-tree/model`, `read_sorted_conf_files`, `resolve_mount`'s
// PARTLABEL-not-found/mount-failure error paths) is still uncovered by
// either `cargo test` or the QEMU harness. Every DECISION made along the
// way (model parsing, BLS selection, cmdline assembly, slot resolution,
// idempotency) is delegated to `orchestrate::plan_sync` and the modules it
// composes — ALL of which are covered by `cargo test` AND the QEMU
// harness. RPi-hardware-specific correctness is validated by the canary w1
// boot-proof gate (plan Section E), not by any of this.
// ---------------------------------------------------------------------------

/// RAII guard for a temp-mounted partition: unmounts + removes the temp dir
/// on drop, mirroring bash's `trap cleanup EXIT`.
struct TempMountGuard {
    path: PathBuf,
}

impl Drop for TempMountGuard {
    fn drop(&mut self) {
        let _ = mounts::unmount(&self.path);
        let _ = std::fs::remove_dir(&self.path);
    }
}

fn resolve_mount(
    existing: BootMount,
    partlabel_candidates: &[&str],
    fs_type: &str,
    writable: bool,
) -> Result<(PathBuf, Option<TempMountGuard>), i32> {
    match existing {
        BootMount::Existing(path) => Ok((PathBuf::from(path), None)),
        BootMount::NeedsTempMount => {
            let Some(device) = partlabel_candidates
                .iter()
                .find_map(|label| mounts::partlabel_device(label))
            else {
                // The EFI mount is the only writable one. A missing EFI-SYSTEM
                // FAT is a brick-class condition, not a benign skip: fail LOUD
                // (exit 1 → visible in journal+console) rather than exit 0,
                // which would masquerade as a successful no-op sync.
                if writable {
                    eprintln!(
                        "rpi-bls-sync: EFI-SYSTEM FAT partition not found ({}) — refusing to sync",
                        partlabel_candidates.join(" or ")
                    );
                    return Err(1);
                }
                eprintln!(
                    "rpi-bls-sync: partition not found ({}), skipping",
                    partlabel_candidates.join(" or ")
                );
                return Err(0);
            };
            // Use /run (core tmpfs — writable and mounted until very late in
            // the shutdown sequence) rather than /tmp. At
            // ostree-finalize-staged's ExecStopPost (the reliable post-BLS-swap
            // hook) the root fs is already read-only, so a /tmp mkdir fails
            // with "Read-only file system" and the sync silently never happens
            // — leaving a stale slot in cmdline.txt that bricks the next boot.
            let tmp = Path::new("/run").join(format!(
                "rpi-bls-sync-{}",
                partlabel_candidates[0]
            ));
            if let Err(e) = std::fs::create_dir_all(&tmp) {
                eprintln!("rpi-bls-sync: mkdir {}: {e}", tmp.display());
                return Err(1);
            }
            let mount_result = if writable {
                mounts::mount_writable(&device, &tmp, fs_type)
            } else {
                mounts::mount_readonly(&device, &tmp, fs_type)
            };
            if let Err(e) = mount_result {
                eprintln!("rpi-bls-sync: mount {}: {e}", device.display());
                return Err(1);
            }
            Ok((tmp.clone(), Some(TempMountGuard { path: tmp })))
        }
    }
}

fn read_sorted_conf_files(dir: &Path, prefix: Option<&str>, suffix: &str) -> Vec<(String, String)> {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = read_dir
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                return false;
            };
            let prefix_ok = prefix.is_none_or(|pfx| name.starts_with(pfx));
            prefix_ok && name.ends_with(suffix)
        })
        .collect();
    // Bytewise sort == LC_ALL=C glob order (see bls.rs / slot.rs doc comments
    // on the same pin).
    paths.sort_by(|a, b| a.as_os_str().cmp(b.as_os_str()));
    paths
        .into_iter()
        .filter_map(|p| {
            let contents = std::fs::read_to_string(&p).ok()?;
            Some((p.to_string_lossy().into_owned(), contents))
        })
        .collect()
}

/// POSIX `dirname` on a `/`-separated path — mirrors bash `dirname
/// "${_OARG#ostree=}"` (line 170).
fn dirname(path: &str) -> String {
    match path.rfind('/') {
        Some(0) => "/".to_string(),
        Some(idx) => path[..idx].to_string(),
        None => ".".to_string(),
    }
}

fn dir_present_under_prefixes(rel: &str) -> [bool; 2] {
    ["", "/sysroot"].map(|prefix| Path::new(&format!("{prefix}{rel}")).is_dir())
}

fn clock_usec_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

/// Enumerate the firmware files (model DTBs + all overlays) present in the
/// deployment's `<bootcsum>/dtb/` dir, returning each as a
/// `(FirmwareFile, source_path)` pair so the planner can decide which need
/// copying and the caller can read only those. Missing subdirs yield an empty
/// list (fail-closed → skip the firmware sync entirely). VERIFIED source
/// layout (live w1-jurek 2026-07-19): `<bootcsum>/dtb/broadcom/bcm27xx*.dtb`
/// and `<bootcsum>/dtb/overlays/*.dtbo`, a sibling of the synced vmlinuz.
/// GPU firmware (rpi4 start4.elf/…) is deliberately NOT synced here: it lives
/// outside the per-deployment dtb dir and is firmware-version- (not kernel-)
/// coupled, so build-time seeding suffices.
pub fn collect_firmware_sources(
    dtb_dir: &Path,
    model: model::KernelName,
) -> Vec<(firmware::FirmwareFile, PathBuf)> {
    let mut out = Vec::new();
    let mut collect = |subdir: &str, keep: &dyn Fn(&str) -> bool, dest_prefix: &str| {
        let Ok(read_dir) = std::fs::read_dir(dtb_dir.join(subdir)) else {
            return;
        };
        for entry in read_dir.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !keep(name) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            out.push((
                firmware::FirmwareFile::new(format!("{dest_prefix}{name}"), meta.len()),
                entry.path(),
            ));
        }
    };
    // DTBs land at the EFI root; overlays under EFI `overlays/`.
    collect("broadcom", &|n| firmware::dtb_matches(model, n), "");
    collect("overlays", &|n| n.ends_with(".dtbo"), "overlays/");
    out
}

/// Orchestration entry point. Returns the process exit code.
pub fn run() -> i32 {
    let model_raw = std::fs::read_to_string("/proc/device-tree/model").unwrap_or_default();

    let boot_existing = mounts::locate_boot(
        mounts::is_mountpoint(Path::new("/boot")),
        mounts::is_mountpoint(Path::new("/sysroot/boot")),
    );
    let (boot_mount, _boot_guard) =
        match resolve_mount(boot_existing, &["boot"], "ext4", false) {
            Ok(v) => v,
            Err(code) => return code,
        };

    let bls_entries = read_sorted_conf_files(&boot_mount.join("loader/entries"), Some("ostree-"), ".conf");
    let bls_entries_refs: Vec<(&str, &str)> =
        bls_entries.iter().map(|(p, c)| (p.as_str(), c.as_str())).collect();

    let proc_cmdline = std::fs::read_to_string("/proc/cmdline").unwrap_or_default();
    let ignition_firstboot_present = boot_mount.join("ignition.firstboot").exists();

    let mut cmdline_d = read_sorted_conf_files(Path::new("/etc/cmdline.d"), None, ".conf");
    cmdline_d.extend(read_sorted_conf_files(Path::new("/sysroot/etc/cmdline.d"), None, ".conf"));
    let cmdline_d_refs: Vec<&str> = cmdline_d.iter().map(|(_, c)| c.as_str()).collect();

    let pre = match orchestrate::assemble_pre_slot_cmdline(
        &model_raw,
        &bls_entries_refs,
        ignition_firstboot_present,
        &cmdline_d_refs,
    ) {
        Ok(v) => v,
        Err(reason) => {
            eprintln!("rpi-bls-sync: {reason}, skipping");
            return 0;
        }
    };

    let deploy_root_prefixes_present = dir_present_under_prefixes("/ostree/deploy");
    // Probe BOTH candidate slots' deploy dirs (see the DeployProbe caller
    // contract in slot.rs): the live /proc slot (target_after_adoption) and
    // the BLS slot (bls_slot_token).
    let slot_dir_present =
        |token: &str| dir_present_under_prefixes(&dirname(token.trim_start_matches("ostree=")));
    let live_slot_dir_present_prefixes = slot::target_after_adoption(&pre.cmdline, &proc_cmdline)
        .map(|t| slot_dir_present(&t))
        .unwrap_or([false, false]);
    let bls_slot_dir_present_prefixes = slot::bls_slot_token(&pre.cmdline)
        .map(|t| slot_dir_present(&t))
        .unwrap_or([false, false]);

    // NB: unlike locate_boot, these bools are "is the REAL EFI-SYSTEM FAT",
    // not "is a mountpoint" — an empty /boot/efi stub must NOT be trusted
    // (silent-stub-write brick). See mounts::locate_efi / efi_mount_is_real_fat.
    let efi_existing = mounts::locate_efi(
        mounts::efi_mount_is_real_fat(Path::new("/boot/efi")),
        mounts::efi_mount_is_real_fat(Path::new("/sysroot/boot/efi")),
    );
    let (efi_mount, _efi_guard) = match resolve_mount(
        efi_existing,
        &["EFI-SYSTEM", "EFI System Partition"],
        "vfat",
        true,
    ) {
        Ok(v) => v,
        Err(code) => return code,
    };

    let mut fs = StdBootFs::new(&efi_mount);

    let src_kernel = boot_mount.join(pre.linux_path.trim_start_matches('/'));
    let src_initramfs = boot_mount.join(pre.initrd_path.trim_start_matches('/'));
    let (Ok(src_kernel_meta), Ok(src_initramfs_meta)) =
        (std::fs::metadata(&src_kernel), std::fs::metadata(&src_initramfs))
    else {
        eprintln!("rpi-bls-sync: kernel/initramfs not found under {}", boot_mount.display());
        return 1;
    };

    let current_cmdline_txt = fs.read_to_string("cmdline.txt");
    let inputs = orchestrate::SyncInputs {
        model_raw: &model_raw,
        bls_entries_sorted: &bls_entries_refs,
        proc_cmdline: &proc_cmdline,
        ignition_firstboot_present,
        cmdline_d_contents_ordered: &cmdline_d_refs,
        deploy_root_prefixes_present,
        live_slot_dir_present_prefixes,
        bls_slot_dir_present_prefixes,
        clock_usec: clock_usec_now(),
        current_cmdline_txt: current_cmdline_txt.as_deref(),
        src_kernel_size: src_kernel_meta.len(),
        dst_kernel_size: fs.file_size(pre.kernel_name),
        src_initramfs_size: src_initramfs_meta.len(),
        dst_initramfs_size: fs.file_size("initramfs.img"),
    };

    match orchestrate::plan_sync(&inputs) {
        orchestrate::SyncPlan::Skip(reason) => {
            eprintln!("rpi-bls-sync: {reason}");
            0
        }
        orchestrate::SyncPlan::RefuseExit0 => {
            eprintln!("rpi-bls-sync: refusing to write absent ostree slot; keeping current cmdline.txt");
            0
        }
        orchestrate::SyncPlan::Sync { cmdline_txt, kernel_name } => {
            let kernel_bytes = match std::fs::read(&src_kernel) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("rpi-bls-sync: read {}: {e}", src_kernel.display());
                    return 1;
                }
            };
            let initramfs_bytes = match std::fs::read(&src_initramfs) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("rpi-bls-sync: read {}: {e}", src_initramfs.display());
                    return 1;
                }
            };

            // Firmware (DTB + overlays) sync — keep the FAT device-tree coupled
            // to the kernel being synced. Firstboot seeds DTBs via
            // build-node-disks.sh but the runtime sync never touched them, so a
            // day-2 kernel bump that ships new DTBs would leave the FAT with the
            // OLD device-tree. Source = the SAME deployment as the kernel:
            // `src_kernel.parent()/dtb/` (VERIFIED sibling of the vmlinuz on a
            // live node). Fail-closed on every axis: unsupported model or a
            // missing dtb dir → skip (degrades to pre-firmware.rs behavior); a
            // source read error → abort BEFORE the cmdline.txt commit so the
            // last-good pointer is preserved, never a half-written device-tree.
            // Idempotent by size (plan_firmware_copies): an in-sync node reads
            // and writes nothing.
            let mut fw_bytes: Vec<(String, Vec<u8>)> = Vec::new();
            if let (Ok(fw_model), Some(bootcsum_dir)) =
                (model::parse_model(&model_raw), src_kernel.parent())
            {
                let sources = collect_firmware_sources(&bootcsum_dir.join("dtb"), fw_model);
                let files: Vec<firmware::FirmwareFile> =
                    sources.iter().map(|(f, _)| f.clone()).collect();
                for needed in firmware::plan_firmware_copies(&files, |d| fs.file_size(d)) {
                    let Some((_, src_path)) =
                        sources.iter().find(|(ff, _)| ff.dest == needed.dest)
                    else {
                        continue;
                    };
                    match std::fs::read(src_path) {
                        Ok(b) => fw_bytes.push((needed.dest.clone(), b)),
                        Err(e) => {
                            eprintln!("rpi-bls-sync: read firmware {}: {e}", src_path.display());
                            return 1;
                        }
                    }
                }
                if !fw_bytes.is_empty() {
                    eprintln!(
                        "rpi-bls-sync: {} DTB/overlay file(s) need sync",
                        fw_bytes.len()
                    );
                }
            }

            // config.txt followkernel directive, kept in step with the kernel
            // being synced. Source = the EFI partition's OWN config.txt via the
            // RESOLVED mount (`fs`), never a raw absolute path: `/boot/efi/…`
            // is context-ambiguous — before /boot is unmounted it's an empty
            // mountpoint stub, and during late shutdown (after /boot unmounts)
            // it resolves to the DEPLOYMENT's pristine copy, which is missing
            // the build-time directive. Reading that pristine copy + the old
            // substring presence check clobbered the FAT without a directive
            // → firmware booted the kernel with no initramfs → VFS panic
            // (cp-jurek brick, 2026-07-17). Mount-discipline rule: every read
            // and write goes through a partition that resolve_mount() has
            // verified mounted (temp-mounting and unmounting if needed).
            let mut small_files: Vec<PendingWrite> = Vec::new();
            let config_txt_new = fs
                .read_to_string("config.txt")
                .map(|c| config_txt::ensure_followkernel(&c));
            if config_txt_new.is_none() {
                eprintln!("rpi-bls-sync: no config.txt on EFI partition — leaving as-is");
            }
            if let Some(ref content) = config_txt_new {
                small_files.push(PendingWrite::Small {
                    dest: "config.txt",
                    contents: content.as_bytes(),
                });
            }
            // DTBs/overlays share the small-file atomic path (write-tmp→fsync
            // →rename); like config.txt they land BEFORE the cmdline.txt commit.
            for (dest, bytes) in &fw_bytes {
                small_files.push(PendingWrite::Small {
                    dest: dest.as_str(),
                    contents: bytes,
                });
            }

            let stale_new = [
                format!("{kernel_name}.new"),
                "initramfs.img.new".to_string(),
                "cmdline.txt.new".to_string(),
                "config.txt.new".to_string(),
            ];
            let stale_new_refs: Vec<&str> = stale_new.iter().map(String::as_str).collect();

            let large_files = [
                PendingWrite::Large { dest: kernel_name, contents: &kernel_bytes },
                PendingWrite::Large { dest: "initramfs.img", contents: &initramfs_bytes },
            ];

            match fatsync::sync_write_plan(
                &mut fs,
                &stale_new_refs,
                &large_files,
                &small_files,
                "cmdline.txt",
                cmdline_txt.as_bytes(),
            ) {
                Ok(()) => {
                    eprintln!("rpi-bls-sync: synced {kernel_name}, initramfs.img");
                    eprintln!("rpi-bls-sync: cmdline.txt = {cmdline_txt}");
                    0
                }
                Err(e) => {
                    eprintln!("rpi-bls-sync: write failed: {e}");
                    1
                }
            }
        }
    }
}
