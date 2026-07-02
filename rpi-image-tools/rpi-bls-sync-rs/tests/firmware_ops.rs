//! T4 (RED phase): failing tests for `firmware` DTB glob + GPU firmware
//! file selection. NEW capability (no bash runtime equivalent).

use rpi_bls_sync::firmware::{dtb_glob_pattern, gpu_firmware_files};
use rpi_bls_sync::model::KernelName;

#[test]
fn rpi5_dtb_pattern_is_bcm2712() {
    assert_eq!(dtb_glob_pattern(KernelName::Rpi5), "bcm2712*.dtb");
}

#[test]
fn rpi4_dtb_pattern_is_bcm2711() {
    assert_eq!(dtb_glob_pattern(KernelName::Rpi4), "bcm2711*.dtb");
}

#[test]
fn rpi4_has_gpu_firmware_files() {
    let files = gpu_firmware_files(KernelName::Rpi4);
    assert_eq!(
        files,
        &["start4.elf", "fixup4.dat", "start4cd.elf", "fixup4cd.dat"]
    );
}

#[test]
fn rpi5_has_no_gpu_firmware_files() {
    // rpi5 (RP1) does not use the VideoCore GPU firmware boot path.
    assert!(gpu_firmware_files(KernelName::Rpi5).is_empty());
}
