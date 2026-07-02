//! T5 (RED phase): failing tests for `fatsync::StdBootFs` — the real
//! `std::fs`-backed `BootFs` implementation. Genuinely testable without
//! root/hardware (std::fs behaves identically on any writable directory;
//! only the actual `mount(2)` calls in mounts.rs are untestable here).

use rpi_bls_sync::fatsync::{BootFs, StdBootFs};

fn tempdir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("rpi-bls-sync-rs-stdbootfs-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn write_then_read_round_trips() {
    let dir = tempdir();
    let mut fs = StdBootFs::new(&dir);
    fs.write("cmdline.txt", b"root=/dev/mapper/root").unwrap();
    assert_eq!(fs.read_to_string("cmdline.txt").unwrap(), "root=/dev/mapper/root");
    assert_eq!(fs.file_size("cmdline.txt"), Some(21));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn missing_file_reads_as_none() {
    let dir = tempdir();
    let fs = StdBootFs::new(&dir);
    assert_eq!(fs.read_to_string("nope.txt"), None);
    assert_eq!(fs.file_size("nope.txt"), None);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn rename_moves_content() {
    let dir = tempdir();
    let mut fs = StdBootFs::new(&dir);
    fs.write("cmdline.txt.new", b"hello").unwrap();
    fs.rename("cmdline.txt.new", "cmdline.txt").unwrap();
    assert_eq!(fs.read_to_string("cmdline.txt").unwrap(), "hello");
    assert_eq!(fs.read_to_string("cmdline.txt.new"), None);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn remove_deletes_file() {
    let dir = tempdir();
    let mut fs = StdBootFs::new(&dir);
    fs.write("stale.new", b"x").unwrap();
    fs.remove("stale.new").unwrap();
    assert_eq!(fs.read_to_string("stale.new"), None);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn remove_missing_file_returns_error_not_panic() {
    let dir = tempdir();
    let mut fs = StdBootFs::new(&dir);
    assert!(fs.remove("does-not-exist").is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn fsync_succeeds_on_existing_file() {
    let dir = tempdir();
    let mut fs = StdBootFs::new(&dir);
    fs.write("a.txt", b"x").unwrap();
    assert!(fs.fsync("a.txt").is_ok());
    assert!(fs.fsync_dir().is_ok());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn write_failure_surfaces_as_error_not_silent() {
    // Writing into a path whose parent directory doesn't exist must return
    // an Err, not silently no-op — this is the exact regression this
    // Result-returning trait exists to prevent (a swallowed write failure
    // on the EFI partition would be worse than the bash version, which
    // aborts under `set -euo pipefail`).
    let dir = tempdir();
    let mut fs = StdBootFs::new(&dir);
    assert!(fs.write("nonexistent-subdir/cmdline.txt", b"x").is_err());
    let _ = std::fs::remove_dir_all(dir);
}
