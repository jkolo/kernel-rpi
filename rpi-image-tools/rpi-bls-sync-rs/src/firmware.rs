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
