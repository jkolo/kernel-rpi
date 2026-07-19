//! T4 (RED phase): failing tests for `firmware` DTB glob + GPU firmware
//! file selection. NEW capability (no bash runtime equivalent).

use rpi_bls_sync::firmware::{
    dtb_glob_pattern, dtb_matches, gpu_firmware_files, plan_firmware_copies, FirmwareFile,
};
use rpi_bls_sync::model::KernelName;
use std::collections::HashMap;

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

// --- dtb_matches: model DTB glob against real filenames from a live node ---

#[test]
fn rpi5_dtb_matches_real_bcm2712_family() {
    // The 8 real files observed under .../dtb/broadcom/ on cp/w rpi5 nodes.
    for f in [
        "bcm2712-rpi-5-b.dtb",
        "bcm2712-d-rpi-5-b.dtb",
        "bcm2712d0-rpi-5-b.dtb",
        "bcm2712-rpi-500.dtb",
        "bcm2712-rpi-cm5-cm4io.dtb",
    ] {
        assert!(dtb_matches(KernelName::Rpi5, f), "{f} should match rpi5 glob");
    }
}

#[test]
fn rpi5_glob_rejects_rpi4_dtb_and_non_dtb() {
    assert!(!dtb_matches(KernelName::Rpi5, "bcm2711-rpi-4-b.dtb"));
    assert!(!dtb_matches(KernelName::Rpi5, "README"));
    assert!(!dtb_matches(KernelName::Rpi5, "overlays"));
    assert!(!dtb_matches(KernelName::Rpi5, "bcm2712-rpi-5-b.dtbo")); // overlay, not a DTB
}

#[test]
fn rpi4_dtb_matches_bcm2711_only() {
    assert!(dtb_matches(KernelName::Rpi4, "bcm2711-rpi-4-b.dtb"));
    assert!(!dtb_matches(KernelName::Rpi4, "bcm2712-rpi-5-b.dtb"));
}

#[test]
fn dtb_glob_no_spurious_match_on_prefix_suffix_overlap() {
    // A file named exactly "bcm2712.dtb" — prefix "bcm2712" + suffix ".dtb"
    // with nothing between. Real DTBs always have a board suffix; the length
    // guard keeps the semantics of a `*` requiring the fixed parts to fit.
    assert!(dtb_matches(KernelName::Rpi5, "bcm2712.dtb"));
    assert!(!dtb_matches(KernelName::Rpi5, "bcm271.dtb"));
}

// --- plan_firmware_copies: which firmware files need (re)writing to FAT ---

fn dests(files: &[&FirmwareFile]) -> Vec<String> {
    files.iter().map(|f| f.dest.clone()).collect()
}

#[test]
fn missing_dest_is_copied() {
    let files = [FirmwareFile::new("bcm2712-rpi-5-b.dtb", 80_000)];
    let dst: HashMap<String, u64> = HashMap::new(); // nothing on FAT yet
    let plan = plan_firmware_copies(&files, |d| dst.get(d).copied());
    assert_eq!(dests(&plan), vec!["bcm2712-rpi-5-b.dtb"]);
}

#[test]
fn size_mismatch_is_copied() {
    let files = [FirmwareFile::new("bcm2712-rpi-5-b.dtb", 80_000)];
    let dst = HashMap::from([("bcm2712-rpi-5-b.dtb".to_string(), 79_000_u64)]);
    let plan = plan_firmware_copies(&files, |d| dst.get(d).copied());
    assert_eq!(dests(&plan), vec!["bcm2712-rpi-5-b.dtb"]);
}

#[test]
fn identical_size_is_skipped() {
    let files = [FirmwareFile::new("bcm2712-rpi-5-b.dtb", 80_000)];
    let dst = HashMap::from([("bcm2712-rpi-5-b.dtb".to_string(), 80_000_u64)]);
    let plan = plan_firmware_copies(&files, |d| dst.get(d).copied());
    assert!(plan.is_empty(), "unchanged DTB must not be rewritten (idempotency)");
}

#[test]
fn mixed_set_copies_only_the_stale_and_missing() {
    // DTB unchanged, overlay changed, GPU firmware missing → only the last two.
    let files = [
        FirmwareFile::new("bcm2711-rpi-4-b.dtb", 60_000),
        FirmwareFile::new("overlays/disable-bt.dtbo", 1_200),
        FirmwareFile::new("start4.elf", 2_200_000),
    ];
    let dst = HashMap::from([
        ("bcm2711-rpi-4-b.dtb".to_string(), 60_000_u64),
        ("overlays/disable-bt.dtbo".to_string(), 900_u64),
    ]);
    let plan = plan_firmware_copies(&files, |d| dst.get(d).copied());
    assert_eq!(dests(&plan), vec!["overlays/disable-bt.dtbo", "start4.elf"]);
}

#[test]
fn empty_source_yields_empty_plan() {
    let files: [FirmwareFile; 0] = [];
    let plan = plan_firmware_copies(&files, |_| Some(1));
    assert!(plan.is_empty());
}
