//! Tests for `mounts::locate_boot` / `locate_efi` / `parse_mount_device`.
//! Ground truth: rpi-bls-sync.sh lines 33-46 (boot), 184-200 (EFI).
//!
//! NB for locate_efi: its bools now mean "is the REAL EFI-SYSTEM FAT" (device
//! validated), not "is a mountpoint" — the pure branching is identical, but a
//! non-FAT stub mountpoint feeds `false` here and correctly routes to a
//! PARTLABEL temp-mount. The device discrimination itself lives in
//! `efi_mount_is_real_fat` (I/O shell) over `parse_mount_device` (pure, below).

use rpi_bls_sync::mounts::{locate_boot, locate_efi, parse_mount_device, BootMount};

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
fn efi_trusts_boot_efi_only_when_it_is_the_real_fat() {
    assert_eq!(locate_efi(true, true), BootMount::Existing("/boot/efi".to_string()));
}

#[test]
fn efi_falls_back_to_sysroot_boot_efi_real_fat() {
    assert_eq!(
        locate_efi(false, true),
        BootMount::Existing("/sysroot/boot/efi".to_string())
    );
}

#[test]
fn efi_temp_mounts_when_neither_is_the_real_fat() {
    // Covers BOTH "not a mountpoint" and "mountpoint but an empty stub" — the
    // stub case is exactly the confirmed brick: a false here → PARTLABEL mount.
    assert_eq!(locate_efi(false, false), BootMount::NeedsTempMount);
}

// --- parse_mount_device: backing device discrimination (stub vs real FAT) ---

// A trimmed /proc/mounts sample from a live RPi5 node (w1-jurek, 2026-07-19):
// /boot is the ext4 p3; the FAT p2 is NOT auto-mounted at /boot/efi there.
const PROC_MOUNTS_SAMPLE: &str = "\
/dev/mapper/root / xfs rw,relatime 0 0
/dev/mmcblk0p3 /boot ext4 rw,relatime 0 0
/dev/mmcblk0p2 /run/rpi-bls-sync-EFI-SYSTEM vfat rw,relatime 0 0
tmpfs /run tmpfs rw,nosuid,nodev 0 0";

#[test]
fn parse_mount_device_finds_backing_device() {
    assert_eq!(parse_mount_device(PROC_MOUNTS_SAMPLE, "/boot"), Some("/dev/mmcblk0p3"));
    assert_eq!(
        parse_mount_device(PROC_MOUNTS_SAMPLE, "/run/rpi-bls-sync-EFI-SYSTEM"),
        Some("/dev/mmcblk0p2")
    );
}

#[test]
fn parse_mount_device_none_when_not_a_mountpoint() {
    // /boot/efi is NOT in the table → not a mountpoint → stub-safe None, which
    // routes locate_efi to the PARTLABEL temp-mount.
    assert_eq!(parse_mount_device(PROC_MOUNTS_SAMPLE, "/boot/efi"), None);
    assert_eq!(parse_mount_device(PROC_MOUNTS_SAMPLE, "/nonexistent"), None);
}

#[test]
fn parse_mount_device_ignores_partial_path_prefixes() {
    // Must match the mountpoint column exactly, not by prefix.
    assert_eq!(parse_mount_device(PROC_MOUNTS_SAMPLE, "/bo"), None);
    assert_eq!(parse_mount_device("", "/boot"), None);
}
