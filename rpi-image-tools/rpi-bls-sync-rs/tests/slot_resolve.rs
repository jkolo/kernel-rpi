//! T2 (RED phase): failing tests for `slot::resolve` — the liveness-first
//! `ostree=` boot-slot re-resolution that fixes the write-then-prune
//! emergency-boot bug (commit 75793e5/7c482bb). Ground truth:
//! rpi-bls-sync.sh lines 135-182. See tests/oracle/slot_resolve.sh.

use rpi_bls_sync::slot::{resolve, strip_slot_prefix, DeployProbe, Resolution};
use std::path::Path;

// ---- Pure unit tests (approved plan sketches) ----

#[test]
fn adopts_live_slot_when_same_deployment() {
    let bls = "root=/dev/mapper/root rw ostree=/ostree/boot.0/rhcos/CSUM/0 console=ttyS0";
    let proc = "ostree=/ostree/boot.1/rhcos/CSUM/0";
    let r = resolve(bls, proc, DeployProbe::RootAccessible);
    assert_eq!(
        r,
        Resolution::Rewrite(
            "root=/dev/mapper/root rw ostree=/ostree/boot.1/rhcos/CSUM/0 console=ttyS0"
                .to_string()
        )
    );
}

#[test]
fn keeps_when_different_deployment() {
    let bls = "ostree=/ostree/boot.0/rhcos/CSUM_A/0";
    let proc = "ostree=/ostree/boot.1/rhcos/CSUM_B/0";
    // Different stateroot/csum/serial → BLS value kept as-is; the existence
    // guard (if it fires) validates the ORIGINAL BLS value, not /proc's.
    assert_eq!(resolve(bls, proc, DeployProbe::RootAccessible), Resolution::Keep);
}

#[test]
fn refuses_absent_slot_in_realroot() {
    let bls = "ostree=/ostree/boot.0/rhcos/GONE/0";
    let proc = "ostree=/ostree/boot.0/rhcos/GONE/0";
    assert_eq!(
        resolve(bls, proc, DeployProbe::RootAccessibleDirAbsent),
        Resolution::RefuseExit0
    );
}

#[test]
fn initrd_trusts_value_no_refuse() {
    let bls = "ostree=/ostree/boot.0/rhcos/GONE/0";
    let proc = "ostree=/ostree/boot.0/rhcos/GONE/0";
    // Pre-LUKS initrd: deploy root not accessible → guard is skipped
    // entirely (firstboot-safety) — must NEVER RefuseExit0, even though the
    // slot dir is (as far as we could tell) absent.
    let r = resolve(bls, proc, DeployProbe::RootInaccessible);
    assert_ne!(r, Resolution::RefuseExit0);
    assert_eq!(r, Resolution::Keep);
}

#[test]
fn strip_slot_prefix_multi_digit_boot_n() {
    assert_eq!(
        strip_slot_prefix("ostree=/ostree/boot.10/rhcos/CSUM/0"),
        "rhcos/CSUM/0"
    );
    assert_eq!(
        strip_slot_prefix("ostree=/ostree/boot.0/rhcos/CSUM/0"),
        "rhcos/CSUM/0"
    );
}

#[test]
fn strip_slot_prefix_non_matching_returns_unchanged() {
    // Bash `${_OARG#pattern}`: if the pattern doesn't match at all, the
    // original string is returned unchanged.
    assert_eq!(strip_slot_prefix("ostree=weird-value"), "ostree=weird-value");
}

#[test]
fn no_ostree_token_keeps_cmdline_untouched() {
    let bls = "root=/dev/mapper/root rw console=ttyS0";
    let proc = "ostree=/ostree/boot.0/rhcos/CSUM/0";
    assert_eq!(resolve(bls, proc, DeployProbe::RootAccessible), Resolution::Keep);
}

#[test]
fn same_value_in_bls_and_proc_no_adopt_needed() {
    let bls = "ostree=/ostree/boot.0/rhcos/CSUM/0";
    let proc = "ostree=/ostree/boot.0/rhcos/CSUM/0";
    // _PROC == _OARG already → adopt branch is skipped (nothing to adopt).
    assert_eq!(resolve(bls, proc, DeployProbe::RootAccessible), Resolution::Keep);
}

// ---- Oracle-backed characterization tests (real bash extract) ----

fn run_oracle(cmdline: &str, proc_cmdline: &str, root: &Path) -> String {
    let oracle_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle/slot_resolve.sh");
    let output = std::process::Command::new("bash")
        .arg(&oracle_path)
        .arg(cmdline)
        .arg(proc_cmdline)
        .arg(root)
        .output()
        .expect("failed to run bash oracle");
    assert!(output.status.success(), "oracle script failed: {output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Resolve via Rust and reduce to the same textual form the oracle prints,
/// so both sides can be compared directly.
fn rust_resolved(cmdline: &str, proc_cmdline: &str, probe: DeployProbe) -> String {
    match resolve(cmdline, proc_cmdline, probe) {
        Resolution::Rewrite(new_cmdline) => format!("RESULT {new_cmdline}"),
        Resolution::Keep => format!("RESULT {cmdline}"),
        Resolution::RefuseExit0 => "REFUSE".to_string(),
    }
}

#[test]
fn oracle_adopt_live_slot() {
    let root = tempdir();
    std::fs::create_dir_all(root.join("ostree/deploy")).unwrap();
    std::fs::create_dir_all(root.join("ostree/boot.1/rhcos/CSUM/0")).unwrap();
    let cmdline = "root=/dev/mapper/root rw ostree=/ostree/boot.0/rhcos/CSUM/0 console=ttyS0";
    let proc = "ostree=/ostree/boot.1/rhcos/CSUM/0";

    let oracle = run_oracle(cmdline, proc, &root);
    let rust = rust_resolved(cmdline, proc, DeployProbe::RootAccessible);
    assert_eq!(rust, oracle);
    cleanup(root);
}

#[test]
fn oracle_different_deployment_keeps() {
    let root = tempdir();
    std::fs::create_dir_all(root.join("ostree/deploy")).unwrap();
    std::fs::create_dir_all(root.join("ostree/boot.0/rhcos/CSUM_A/0")).unwrap();
    let cmdline = "root=/dev/mapper/root rw ostree=/ostree/boot.0/rhcos/CSUM_A/0 console=ttyS0";
    let proc = "ostree=/ostree/boot.1/rhcos/CSUM_B/0";

    let oracle = run_oracle(cmdline, proc, &root);
    let rust = rust_resolved(cmdline, proc, DeployProbe::RootAccessible);
    assert_eq!(rust, oracle);
    cleanup(root);
}

#[test]
fn oracle_refuse_absent_slot() {
    let root = tempdir();
    std::fs::create_dir_all(root.join("ostree/deploy")).unwrap();
    let cmdline = "root=/dev/mapper/root rw ostree=/ostree/boot.0/rhcos/GONE/0 console=ttyS0";
    let proc = "ostree=/ostree/boot.0/rhcos/GONE/0";

    let oracle = run_oracle(cmdline, proc, &root);
    let rust = rust_resolved(cmdline, proc, DeployProbe::RootAccessibleDirAbsent);
    assert_eq!(rust, oracle);
    assert_eq!(oracle, "REFUSE");
    cleanup(root);
}

#[test]
fn oracle_initrd_trusts_value() {
    let root = tempdir(); // no /ostree/deploy created anywhere → root inaccessible
    let cmdline = "root=/dev/mapper/root rw ostree=/ostree/boot.0/rhcos/GONE/0 console=ttyS0";
    let proc = "ostree=/ostree/boot.0/rhcos/GONE/0";

    let oracle = run_oracle(cmdline, proc, &root);
    let rust = rust_resolved(cmdline, proc, DeployProbe::RootInaccessible);
    assert_eq!(rust, oracle);
    cleanup(root);
}

#[test]
fn oracle_multi_digit_slot() {
    let root = tempdir();
    std::fs::create_dir_all(root.join("ostree/deploy")).unwrap();
    std::fs::create_dir_all(root.join("ostree/boot.10/rhcos/CSUM/0")).unwrap();
    let cmdline = "ostree=/ostree/boot.2/rhcos/CSUM/0";
    let proc = "ostree=/ostree/boot.10/rhcos/CSUM/0";

    let oracle = run_oracle(cmdline, proc, &root);
    let rust = rust_resolved(cmdline, proc, DeployProbe::RootAccessible);
    assert_eq!(rust, oracle);
    cleanup(root);
}

fn tempdir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "rpi-bls-sync-rs-test-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cleanup(dir: std::path::PathBuf) {
    let _ = std::fs::remove_dir_all(dir);
}
