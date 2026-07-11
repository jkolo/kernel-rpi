//! T4 (RED phase): failing tests for `mounts::locate_boot` / `locate_efi`.
//! Ground truth: rpi-bls-sync.sh lines 33-46 (boot), 184-200 (EFI).

use rpi_bls_sync::mounts::{locate_boot, locate_efi, BootMount};

#[test]
fn boot_prefers_existing_boot_mountpoint() {
    assert_eq!(locate_boot(true, true), BootMount::Existing("/boot".to_string()));
}

#[test]
fn boot_falls_back_to_sysroot_boot() {
    assert_eq!(
        locate_boot(false, true),
        BootMount::Existing("/sysroot/boot".to_string())
    );
}

#[test]
fn boot_needs_temp_mount_when_neither_present() {
    assert_eq!(locate_boot(false, false), BootMount::NeedsTempMount);
}

#[test]
fn efi_prefers_existing_boot_efi_mountpoint() {
    assert_eq!(locate_efi(true, true), BootMount::Existing("/boot/efi".to_string()));
}

#[test]
fn efi_falls_back_to_sysroot_boot_efi() {
    assert_eq!(
        locate_efi(false, true),
        BootMount::Existing("/sysroot/boot/efi".to_string())
    );
}

#[test]
fn efi_needs_temp_mount_when_neither_present() {
    assert_eq!(locate_efi(false, false), BootMount::NeedsTempMount);
}
