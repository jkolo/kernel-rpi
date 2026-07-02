//! FAT32 EFI-partition write ordering, atomicity, and idempotency.
//! Ground truth: rpi-bls-sync.sh lines 202-233 (idempotency + direct write).
//!
//! The EFI partition is ~127MB — kernel (~31MB) + initramfs (~55MB) leave no
//! room for old+new copies simultaneously, so large files are written
//! in-place. Small files use write-tmp→fsync→rename→fsync(dir). In all
//! cases `cmdline.txt` (the boot pointer) is written LAST, after fsync of
//! everything else — it is the commit point.

/// Minimal filesystem trait so idempotency/ordering logic is testable
/// against an in-memory fake instead of a real FAT32 mount. Mutating
/// methods return `io::Result<()>` — a real write failure on the EFI
/// partition MUST abort the sync (mirrors bash `set -euo pipefail`: a
/// failed `cp -f` kills the script immediately), never fail silently. The
/// in-memory fake used by this module's tests always returns `Ok(())`.
pub trait BootFs {
    fn file_size(&self, path: &str) -> Option<u64>;
    fn read_to_string(&self, path: &str) -> Option<String>;
    fn write(&mut self, path: &str, contents: &[u8]) -> std::io::Result<()>;
    fn rename(&mut self, from: &str, to: &str) -> std::io::Result<()>;
    fn remove(&mut self, path: &str) -> std::io::Result<()>;
    fn fsync(&mut self, path: &str) -> std::io::Result<()>;
    fn fsync_dir(&mut self) -> std::io::Result<()>;
}

/// Compare source/destination sizes + clock-stripped cmdline to decide
/// whether a sync is needed. Mirrors lines 204-220.
pub fn needs_sync(
    src_kernel_size: u64,
    dst_kernel_size: Option<u64>,
    src_initramfs_size: u64,
    dst_initramfs_size: Option<u64>,
    current_cmdline_stripped: &str,
    new_cmdline_stripped: &str,
) -> bool {
    // `stat ... 2>/dev/null || echo 0`: a missing dest compares as size 0.
    let dst_kernel_size = dst_kernel_size.unwrap_or(0);
    let dst_initramfs_size = dst_initramfs_size.unwrap_or(0);
    src_kernel_size != dst_kernel_size
        || src_initramfs_size != dst_initramfs_size
        || current_cmdline_stripped != new_cmdline_stripped
}

/// A pending write: `Large` files (kernel, initramfs) are written in-place
/// (no `.new`/rename — the FAT partition has no room for old+new
/// simultaneously); `Small` files use write-tmp→fsync→rename atomicity.
pub enum PendingWrite<'a> {
    Large { dest: &'a str, contents: &'a [u8] },
    Small { dest: &'a str, contents: &'a [u8] },
}

/// Execute the sync write plan: remove stale `.new` stragglers from any
/// previous interrupted sync (line 224), write large files in-place, write
/// small files atomically, then ALWAYS write `cmdline.txt` (the boot
/// pointer) LAST — after `fsync_dir()` of everything else. `cmdline.txt`
/// itself still goes through the small-file atomic path; it's the ORDER
/// relative to everything else that's the invariant, not its own atomicity.
pub fn sync_write_plan(
    fs: &mut dyn BootFs,
    stale_new_files: &[&str],
    large_files: &[PendingWrite],
    small_files_excl_cmdline: &[PendingWrite],
    cmdline_dest: &str,
    cmdline_contents: &[u8],
) -> std::io::Result<()> {
    for stale in stale_new_files {
        // Stale .new cleanup: a missing file is not an error (mirrors `rm -f`).
        let _ = fs.remove(stale);
    }
    for pw in large_files {
        if let PendingWrite::Large { dest, contents } = pw {
            fs.write(dest, contents)?;
        }
    }
    for pw in small_files_excl_cmdline {
        if let PendingWrite::Small { dest, contents } = pw {
            write_atomic(fs, dest, contents)?;
        }
    }
    fs.fsync_dir()?;
    // cmdline.txt is the commit point — always last.
    write_atomic(fs, cmdline_dest, cmdline_contents)?;
    fs.fsync_dir()
}

fn write_atomic(fs: &mut dyn BootFs, dest: &str, contents: &[u8]) -> std::io::Result<()> {
    let tmp = format!("{dest}.new");
    fs.write(&tmp, contents)?;
    fs.fsync(&tmp)?;
    fs.rename(&tmp, dest)
}

/// Real `BootFs` over `std::fs`, rooted at a directory (the EFI mountpoint
/// in production; any writable directory in tests — `std::fs` behaves
/// identically regardless of the underlying filesystem, so this part
/// genuinely IS testable without a real FAT32 mount, unlike `mounts.rs`'s
/// `mount(2)` calls).
pub struct StdBootFs {
    root: std::path::PathBuf,
}

impl StdBootFs {
    pub fn new(root: impl Into<std::path::PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn full_path(&self, path: &str) -> std::path::PathBuf {
        self.root.join(path)
    }
}

impl BootFs for StdBootFs {
    fn file_size(&self, path: &str) -> Option<u64> {
        std::fs::metadata(self.full_path(path)).ok().map(|m| m.len())
    }

    fn read_to_string(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(self.full_path(path)).ok()
    }

    fn write(&mut self, path: &str, contents: &[u8]) -> std::io::Result<()> {
        std::fs::write(self.full_path(path), contents)
    }

    fn rename(&mut self, from: &str, to: &str) -> std::io::Result<()> {
        std::fs::rename(self.full_path(from), self.full_path(to))
    }

    fn remove(&mut self, path: &str) -> std::io::Result<()> {
        std::fs::remove_file(self.full_path(path))
    }

    fn fsync(&mut self, path: &str) -> std::io::Result<()> {
        std::fs::File::open(self.full_path(path))?.sync_all()
    }

    fn fsync_dir(&mut self) -> std::io::Result<()> {
        std::fs::File::open(&self.root)?.sync_all()
    }
}
