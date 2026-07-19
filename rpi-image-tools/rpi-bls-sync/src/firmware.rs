//! NEW (not in bash): runtime sync of DTB + overlays + GPU firmware (rpi4)
//! from `<deploy_dir>/usr/lib/modules/$KVER/dtb/` to the EFI partition,
//! keeping them coupled to the currently-synced kernel.

use crate::model::KernelName;

/// Build the source path for the model-specific DTB under
/// `<deploy_dir>/usr/lib/modules/$KVER/dtb/broadcom/`.
/// rpi5 → `bcm2712*.dtb`, rpi4 → `bcm2711*.dtb`.
pub fn dtb_glob_pattern(model: KernelName) -> &'static str {
    match model {
        KernelName::Rpi5 => "bcm2712*.dtb",
        KernelName::Rpi4 => "bcm2711*.dtb",
    }
}

/// GPU firmware filenames to sync for rpi4 only (`start4.elf`, `fixup4.dat`,
/// `start4cd.elf`, `fixup4cd.dat`). rpi5 (RP1) does not use the VideoCore
/// GPU firmware boot path → empty (skip).
pub fn gpu_firmware_files(model: KernelName) -> &'static [&'static str] {
    match model {
        KernelName::Rpi4 => &["start4.elf", "fixup4.dat", "start4cd.elf", "fixup4cd.dat"],
        KernelName::Rpi5 => &[],
    }
}

/// Does `filename` match this model's DTB glob (`bcm2712*.dtb` for rpi5,
/// `bcm2711*.dtb` for rpi4)? The pattern is always a single `*` between a
/// fixed prefix and suffix; the length guard stops a file named exactly
/// `bcm2712.dtb` (prefix and suffix overlapping) from matching spuriously.
///
/// Verified source: on a live node the DTBs sit at
/// `/boot/ostree/rhcos-<csum>/dtb/broadcom/` — a sibling of the vmlinuz the
/// sync already copies, so the real I/O layer derives the dir from
/// `src_kernel.parent()` and filters its entries through this.
pub fn dtb_matches(model: KernelName, filename: &str) -> bool {
    let pat = dtb_glob_pattern(model);
    let (prefix, suffix) = pat
        .split_once('*')
        .expect("dtb glob pattern always has exactly one '*'");
    filename.len() >= prefix.len() + suffix.len()
        && filename.starts_with(prefix)
        && filename.ends_with(suffix)
}

/// A firmware file the real I/O layer discovered in the deploy source, with
/// its source byte-size. `dest` is the path RELATIVE to the EFI mountpoint —
/// e.g. `"bcm2712-rpi-5-b.dtb"` (DTB, EFI root) or
/// `"overlays/disable-bt.dtbo"` (overlay, EFI subdir).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareFile {
    pub dest: String,
    pub src_size: u64,
}

impl FirmwareFile {
    pub fn new(dest: impl Into<String>, src_size: u64) -> Self {
        Self { dest: dest.into(), src_size }
    }
}

/// Decide which firmware files (DTBs, overlays, rpi4 GPU firmware) must be
/// (re)written to the EFI partition: those whose dest is MISSING, or whose
/// on-EFI size DIFFERS from the source.
///
/// Size-only comparison deliberately mirrors [`crate::fatsync::needs_sync`] —
/// DTBs/overlays/GPU-fw are immutable build artifacts (a change of content is
/// a change of size in every realistic bump), the FAT partition has no room
/// for content hashing, and firmware never carries the clock/slot mutation
/// that forces `cmdline.txt`'s special-case compare.
///
/// PURE: no filesystem access — the caller enumerates the source dir, stats
/// each candidate, and supplies `dst_size`. Returns references into `files`
/// so the caller can read each `dest`'s source bytes only for the subset that
/// actually needs copying.
pub fn plan_firmware_copies(
    files: &[FirmwareFile],
    mut dst_size: impl FnMut(&str) -> Option<u64>,
) -> Vec<&FirmwareFile> {
    files
        .iter()
        .filter(|f| dst_size(&f.dest) != Some(f.src_size))
        .collect()
}
