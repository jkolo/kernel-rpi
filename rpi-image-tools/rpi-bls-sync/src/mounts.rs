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

/// Locate the EFI (FAT32) partition. CRITICAL DIFFERENCE from `locate_boot`:
/// the bool inputs are NOT "is a mountpoint" but "is a mountpoint backed by
/// the REAL EFI-SYSTEM partition" (see `efi_mount_is_real_fat`). An existing
/// `/boot/efi` is trusted ONLY when it genuinely IS the FAT; otherwise we
/// fall through to a PARTLABEL temp-mount.
///
/// Why: on RHCOS-RPi `/boot/efi` can surface as a mountpoint that is an EMPTY
/// read-only stub (the ostree deployment's own `/boot/efi` dir via composefs,
/// or an early pre-FAT bind) rather than the real FAT p2. The old
/// "prefer any existing mountpoint" logic trusted that stub → reads returned
/// nothing, writes hit EACCES → the FAT was SILENTLY never updated → a dead
/// ostree slot in cmdline.txt after the BLS swap bricked the next boot
/// (confirmed canary brick). Validating the backing device before trusting
/// the mount — and temp-mounting EFI-SYSTEM by PARTLABEL otherwise — makes
/// every write land on the real FAT.
pub fn locate_efi(boot_efi_is_real_fat: bool, sysroot_boot_efi_is_real_fat: bool) -> BootMount {
    if boot_efi_is_real_fat {
        BootMount::Existing("/boot/efi".to_string())
    } else if sysroot_boot_efi_is_real_fat {
        BootMount::Existing("/sysroot/boot/efi".to_string())
    } else {
        BootMount::NeedsTempMount
    }
}

/// Pure: the backing device (first column) of the `target` mountpoint in
/// `/proc/mounts` text, or None if `target` is not a mountpoint. Split out
/// from the file read so the stub-vs-real-FAT discrimination is unit-testable.
pub fn parse_mount_device<'a>(proc_mounts: &'a str, target: &str) -> Option<&'a str> {
    proc_mounts.lines().find_map(|line| {
        let mut cols = line.split_whitespace();
        let dev = cols.next()?;
        let mnt = cols.next()?;
        (mnt == target).then_some(dev)
    })
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

/// Is `path` a mountpoint backed by the REAL EFI-SYSTEM partition (the FAT),
/// as opposed to an empty stub / bind? Reads `/proc/mounts` for `path`'s
/// backing device and compares it (canonicalised) to the EFI-SYSTEM PARTLABEL
/// device. `false` when `path` is not a mountpoint, when EFI-SYSTEM can't be
/// resolved, or when the devices differ — all of which correctly route the
/// caller to a fresh PARTLABEL temp-mount instead of trusting the mount.
pub fn efi_mount_is_real_fat(path: &Path) -> bool {
    let Ok(proc_mounts) = std::fs::read_to_string("/proc/mounts") else {
        return false;
    };
    let Some(mnt_dev) = parse_mount_device(&proc_mounts, &path.to_string_lossy()) else {
        return false;
    };
    let Some(efi_dev) = partlabel_device("EFI-SYSTEM") else {
        return false;
    };
    std::fs::canonicalize(mnt_dev).ok().as_deref() == Some(efi_dev.as_path())
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
