//! Boot/EFI partition location — self-contained mount(2) (no exec of
//! mount(8)/blkid), with an `instmods vfat ext4` insurance policy in the
//! dracut module since `mount(2)` does not autoload filesystem modules the
//! way `mount(8)` does.
//! Ground truth: rpi-bls-sync.sh lines 29-55, 184-200.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootMount {
    /// Already mounted at /boot (or /sysroot/boot).
    Existing(String),
    /// Not mounted — must be located by PARTLABEL and temp-mounted read-only.
    NeedsTempMount,
}

/// Locate the boot partition, preferring an existing mountpoint over a
/// PARTLABEL lookup + temp mount. Mirrors lines 33-46.
pub fn locate_boot(mounted_at_boot: bool, mounted_at_sysroot_boot: bool) -> BootMount {
    if mounted_at_boot {
        BootMount::Existing("/boot".to_string())
    } else if mounted_at_sysroot_boot {
        BootMount::Existing("/sysroot/boot".to_string())
    } else {
        BootMount::NeedsTempMount
    }
}

/// Locate the EFI (FAT32) partition, same existing-mountpoint-first pattern
/// as `locate_boot`. Mirrors lines 184-200.
pub fn locate_efi(mounted_at_boot_efi: bool, mounted_at_sysroot_boot_efi: bool) -> BootMount {
    if mounted_at_boot_efi {
        BootMount::Existing("/boot/efi".to_string())
    } else if mounted_at_sysroot_boot_efi {
        BootMount::Existing("/sysroot/boot/efi".to_string())
    } else {
        BootMount::NeedsTempMount
    }
}

// ---------------------------------------------------------------------------
// Real I/O. NOT covered by `cargo test` — mount(2)/umount(2) require root
// plus a real or loop block device, which a plain `cargo test` run doesn't
// have. `tests/qemu/run.sh` exercises this for real instead: a disposable
// QEMU guest (root only inside the guest) loop-attaches a genuine GPT disk
// image and calls partlabel_device/mount_readonly/mount_writable/
// is_mountpoint/unmount against real ext4/vfat partitions — see
// tests/qemu/README.md. RPi-hardware-specific correctness (the parts even
// that harness can't reach — real GPIO/EEPROM/boot) is still validated by
// the canary w1 boot-proof gate (plan Section E). Every decision ABOVE this
// line (locate_boot/locate_efi and everything in orchestrate::plan_sync) IS
// covered by both `cargo test` AND the QEMU harness; this is deliberately
// the thinnest possible real-syscall shell around that tested logic.
// ---------------------------------------------------------------------------

use std::io;
use std::path::{Path, PathBuf};

/// Is `path` currently a mountpoint? Parses `/proc/mounts` for an exact
/// match on the mount-point column — works uniformly for bind mounts,
/// tmpfs, and real block devices, no `statx`/device-number comparison
/// needed. Mirrors the effect of `mountpoint -q <path>`.
pub fn is_mountpoint(path: &Path) -> bool {
    let Ok(contents) = std::fs::read_to_string("/proc/mounts") else {
        return false;
    };
    let target = path.to_string_lossy();
    contents
        .lines()
        .any(|line| line.split_whitespace().nth(1) == Some(target.as_ref()))
}

/// Resolve a partition by PARTLABEL via the udev-maintained
/// `/dev/disk/by-partlabel/<label>` symlink — no `blkid` exec required.
/// Mirrors `blkid -t PARTLABEL=<label> -o device`.
pub fn partlabel_device(label: &str) -> Option<PathBuf> {
    let link = PathBuf::from("/dev/disk/by-partlabel").join(label);
    std::fs::canonicalize(&link).ok()
}

/// Mount `device` read-only at `target`. Real mount(2) syscall — no exec of
/// mount(8). Mirrors `mount -t <fs_type> -o ro <device> <target>`.
pub fn mount_readonly(device: &Path, target: &Path, fs_type: &str) -> io::Result<()> {
    rustix::mount::mount(
        device,
        target,
        fs_type,
        rustix::mount::MountFlags::RDONLY,
        None,
    )
    .map_err(io::Error::from)
}

/// Mount `device` (writable) at `target`. Used for the EFI partition, which
/// is written to (unlike the boot partition, which is only ever read).
pub fn mount_writable(device: &Path, target: &Path, fs_type: &str) -> io::Result<()> {
    rustix::mount::mount(
        device,
        target,
        fs_type,
        rustix::mount::MountFlags::empty(),
        None,
    )
    .map_err(io::Error::from)
}

/// Unmount `target`. Real umount(2) syscall — no exec of umount(8).
pub fn unmount(target: &Path) -> io::Result<()> {
    rustix::mount::unmount(target, rustix::mount::UnmountFlags::empty()).map_err(io::Error::from)
}
