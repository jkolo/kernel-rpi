//! Integration test for `collect_firmware_sources` — the real-`std::fs`
//! enumeration of the deployment `<bootcsum>/dtb/` dir. Genuinely testable
//! without root/hardware (read_dir on a tempdir behaves identically to a real
//! ext4 /boot). Mirrors the VERIFIED live-node layout: broadcom/bcm27xx*.dtb
//! at the EFI root, overlays/*.dtbo under `overlays/`.

use rpi_bls_sync::collect_firmware_sources;
use rpi_bls_sync::firmware::plan_firmware_copies;
use rpi_bls_sync::model::KernelName;
use std::collections::HashMap;

fn tempdir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("rpi-bls-sync-fwcollect-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Build a `<dtb>/broadcom` + `<dtb>/overlays` tree mirroring a live node and
/// return the dtb dir.
fn seed_dtb_dir(root: &std::path::Path) -> std::path::PathBuf {
    let dtb = root.join("dtb");
    let broadcom = dtb.join("broadcom");
    let overlays = dtb.join("overlays");
    std::fs::create_dir_all(&broadcom).unwrap();
    std::fs::create_dir_all(&overlays).unwrap();
    // rpi5 DTBs (sizes stand in for the ~78KB real files)
    std::fs::write(broadcom.join("bcm2712-rpi-5-b.dtb"), vec![0u8; 100]).unwrap();
    std::fs::write(broadcom.join("bcm2712-d-rpi-5-b.dtb"), vec![0u8; 101]).unwrap();
    // an rpi4 DTB that must NOT be picked for an rpi5 sync
    std::fs::write(broadcom.join("bcm2711-rpi-4-b.dtb"), vec![0u8; 90]).unwrap();
    // non-DTB stragglers ostree ships in broadcom/
    std::fs::write(broadcom.join("README"), b"not a dtb").unwrap();
    // overlays
    std::fs::write(overlays.join("disable-bt.dtbo"), vec![0u8; 20]).unwrap();
    std::fs::write(overlays.join("w1-gpio.dtbo"), vec![0u8; 21]).unwrap();
    std::fs::write(overlays.join("README"), b"overlay readme").unwrap();
    dtb
}

fn dests(collected: &[(rpi_bls_sync::firmware::FirmwareFile, std::path::PathBuf)]) -> Vec<String> {
    let mut d: Vec<String> = collected.iter().map(|(f, _)| f.dest.clone()).collect();
    d.sort();
    d
}

#[test]
fn collects_model_dtbs_and_overlays_only() {
    let root = tempdir();
    let dtb = seed_dtb_dir(&root);

    let got = collect_firmware_sources(&dtb, KernelName::Rpi5);

    assert_eq!(
        dests(&got),
        vec![
            "bcm2712-d-rpi-5-b.dtb".to_string(),
            "bcm2712-rpi-5-b.dtb".to_string(),
            "overlays/disable-bt.dtbo".to_string(),
            "overlays/w1-gpio.dtbo".to_string(),
        ],
        "rpi4 DTB, README and overlay README must be excluded; overlays get the overlays/ prefix"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn source_path_points_at_the_real_file() {
    let root = tempdir();
    let dtb = seed_dtb_dir(&root);

    let got = collect_firmware_sources(&dtb, KernelName::Rpi5);
    let (_, path) = got
        .iter()
        .find(|(f, _)| f.dest == "overlays/disable-bt.dtbo")
        .expect("overlay collected");
    assert_eq!(*path, dtb.join("overlays").join("disable-bt.dtbo"));
    assert!(path.is_file());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn missing_dtb_dir_yields_empty_fail_closed() {
    let root = tempdir();
    // no dtb/ subtree created at all
    let got = collect_firmware_sources(&root.join("dtb"), KernelName::Rpi5);
    assert!(got.is_empty(), "a missing dtb dir must fail closed (skip), not panic");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn end_to_end_idempotent_when_fat_matches() {
    // The current-cluster case: FAT already holds identically-sized DTBs +
    // overlays → plan_firmware_copies returns empty → zero writes.
    let root = tempdir();
    let dtb = seed_dtb_dir(&root);
    let collected = collect_firmware_sources(&dtb, KernelName::Rpi5);
    let files: Vec<_> = collected.iter().map(|(f, _)| f.clone()).collect();

    let fat: HashMap<String, u64> = files.iter().map(|f| (f.dest.clone(), f.src_size)).collect();
    let plan = plan_firmware_copies(&files, |d| fat.get(d).copied());
    assert!(plan.is_empty(), "in-sync FAT must produce no firmware writes");

    // Now simulate a kernel bump: one DTB changed size, one overlay missing.
    let mut fat2 = fat.clone();
    fat2.insert("bcm2712-rpi-5-b.dtb".to_string(), 999); // size differs
    fat2.remove("overlays/w1-gpio.dtbo"); // missing on FAT
    let mut plan2: Vec<String> = plan_firmware_copies(&files, |d| fat2.get(d).copied())
        .iter()
        .map(|f| f.dest.clone())
        .collect();
    plan2.sort();
    assert_eq!(
        plan2,
        vec!["bcm2712-rpi-5-b.dtb".to_string(), "overlays/w1-gpio.dtbo".to_string()]
    );
    let _ = std::fs::remove_dir_all(root);
}
