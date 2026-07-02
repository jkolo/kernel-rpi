//! T4 (RED phase): failing tests for `fatsync::needs_sync` and the
//! write-ordering guarantee (cmdline.txt LAST — the commit point).
//! Ground truth: rpi-bls-sync.sh lines 202-233.

use rpi_bls_sync::fatsync::{needs_sync, sync_write_plan, BootFs, PendingWrite};

#[test]
fn needs_sync_false_when_everything_matches() {
    assert!(!needs_sync(31_000_000, Some(31_000_000), 55_000_000, Some(55_000_000), "a b", "a b"));
}

#[test]
fn needs_sync_true_when_kernel_size_differs() {
    assert!(needs_sync(31_000_001, Some(31_000_000), 55_000_000, Some(55_000_000), "a b", "a b"));
}

#[test]
fn needs_sync_true_when_initramfs_size_differs() {
    assert!(needs_sync(31_000_000, Some(31_000_000), 55_000_001, Some(55_000_000), "a b", "a b"));
}

#[test]
fn needs_sync_true_when_dest_missing() {
    // stat -c%s ... 2>/dev/null || echo 0 — a missing dest file compares as
    // size 0, which (almost) never matches a real source size.
    assert!(needs_sync(31_000_000, None, 55_000_000, Some(55_000_000), "a b", "a b"));
}

#[test]
fn needs_sync_true_when_cmdline_differs() {
    assert!(needs_sync(31_000_000, Some(31_000_000), 55_000_000, Some(55_000_000), "a b", "a c"));
}

// ---- Write-order guarantee: cmdline.txt is ALWAYS the last write ----

#[derive(Default)]
struct RecordingFs {
    calls: Vec<String>,
    files: std::collections::HashMap<String, Vec<u8>>,
}

impl BootFs for RecordingFs {
    fn file_size(&self, path: &str) -> Option<u64> {
        self.files.get(path).map(|c| c.len() as u64)
    }
    fn read_to_string(&self, path: &str) -> Option<String> {
        self.files
            .get(path)
            .map(|c| String::from_utf8_lossy(c).into_owned())
    }
    fn write(&mut self, path: &str, contents: &[u8]) -> std::io::Result<()> {
        self.calls.push(format!("write {path}"));
        self.files.insert(path.to_string(), contents.to_vec());
        Ok(())
    }
    fn rename(&mut self, from: &str, to: &str) -> std::io::Result<()> {
        self.calls.push(format!("rename {from} -> {to}"));
        if let Some(c) = self.files.remove(from) {
            self.files.insert(to.to_string(), c);
        }
        Ok(())
    }
    fn remove(&mut self, path: &str) -> std::io::Result<()> {
        self.calls.push(format!("remove {path}"));
        self.files.remove(path);
        Ok(())
    }
    fn fsync(&mut self, path: &str) -> std::io::Result<()> {
        self.calls.push(format!("fsync {path}"));
        Ok(())
    }
    fn fsync_dir(&mut self) -> std::io::Result<()> {
        self.calls.push("fsync_dir".to_string());
        Ok(())
    }
}

#[test]
fn cmdline_txt_write_is_always_last() {
    let mut fs = RecordingFs::default();
    sync_write_plan(
        &mut fs,
        &["kernel_2712.img.new", "initramfs.img.new", "cmdline.txt.new"],
        &[
            PendingWrite::Large { dest: "kernel_2712.img", contents: b"KERNEL" },
            PendingWrite::Large { dest: "initramfs.img", contents: b"INITRAMFS" },
        ],
        &[PendingWrite::Small { dest: "config.txt", contents: b"CONFIG" }],
        "cmdline.txt",
        b"root=/dev/mapper/root",
    )
    .unwrap();

    // The stale-`.new` cleanup (including a stale cmdline.txt.new from a
    // prior interrupted run) legitimately happens up front, before ANY
    // real write — it's debris removal, not part of this run's write
    // order. What matters: the WRITE of the new cmdline.txt content must
    // come after every OTHER file's write/rename.
    let cmdline_write_idx = fs
        .calls
        .iter()
        .position(|c| c == "write cmdline.txt.new")
        .expect("cmdline.txt.new must be written");
    for (i, call) in fs.calls.iter().enumerate() {
        let is_other_write_or_rename = (call.starts_with("write ") || call.starts_with("rename "))
            && !call.contains("cmdline.txt");
        if is_other_write_or_rename {
            assert!(
                i < cmdline_write_idx,
                "non-cmdline write/rename {call:?} at index {i} happened at/after the cmdline.txt.new write (index {cmdline_write_idx})"
            );
        }
    }
    assert_eq!(fs.files.get("cmdline.txt").unwrap(), b"root=/dev/mapper/root");
    assert_eq!(fs.files.get("kernel_2712.img").unwrap(), b"KERNEL");
    assert_eq!(fs.files.get("config.txt").unwrap(), b"CONFIG");
}

#[test]
fn stale_new_files_removed_before_any_write() {
    let mut fs = RecordingFs::default();
    sync_write_plan(
        &mut fs,
        &["stale.new"],
        &[],
        &[],
        "cmdline.txt",
        b"x",
    )
    .unwrap();
    assert_eq!(fs.calls.first().unwrap(), "remove stale.new");
}
