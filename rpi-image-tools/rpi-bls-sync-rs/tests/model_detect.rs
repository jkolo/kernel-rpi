//! T3 (RED phase): failing tests for `model::parse_model`.
//! Ground truth: rpi-bls-sync.sh lines 19-27.

use rpi_bls_sync::model::{parse_model, KernelName, UnsupportedModel};
use std::path::Path;

#[test]
fn detects_rpi5() {
    assert_eq!(
        parse_model("Raspberry Pi 5 Model B Rev 1.0"),
        Ok(KernelName::Rpi5)
    );
    assert_eq!(KernelName::Rpi5.efi_filename(), "kernel_2712.img");
}

#[test]
fn detects_rpi4() {
    assert_eq!(
        parse_model("Raspberry Pi 4 Model B Rev 1.5"),
        Ok(KernelName::Rpi4)
    );
    assert_eq!(KernelName::Rpi4.efi_filename(), "kernel8.img");
}

#[test]
fn unsupported_model_is_error_not_panic() {
    assert_eq!(
        parse_model("Some Other Board"),
        Err(UnsupportedModel("Some Other Board".to_string()))
    );
}

#[test]
fn strips_embedded_nul() {
    // /proc/device-tree/model is NUL-terminated; the source strips it with
    // `tr -d '\0'` before the case match.
    assert_eq!(
        parse_model("Raspberry Pi 5 Model B Rev 1.0\0"),
        Ok(KernelName::Rpi5)
    );
}

#[test]
fn rpi5_checked_before_rpi4() {
    // Substring match order matters: "Raspberry Pi 5" must be checked
    // BEFORE "Raspberry Pi 4" (rpi-bls-sync.sh lines 21-22 case-arm order).
    // Contrived string containing both substrings proves the order, not
    // just that each is individually detected.
    assert_eq!(
        parse_model("XRaspberry Pi 4XRaspberry Pi 5X"),
        Ok(KernelName::Rpi5)
    );
}

// ---- Oracle-backed characterization tests ----

fn run_oracle(model: &str) -> String {
    let oracle_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle/model.sh");
    let output = std::process::Command::new("bash")
        .arg(&oracle_path)
        .arg(model)
        .output()
        .expect("failed to run bash oracle");
    assert!(output.status.success(), "oracle script failed: {output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn rust_resolved(model: &str) -> String {
    match parse_model(model) {
        Ok(k) => k.efi_filename().to_string(),
        Err(_) => "UNSUPPORTED".to_string(),
    }
}

#[test]
fn oracle_rpi5() {
    let m = "Raspberry Pi 5 Model B Rev 1.0";
    assert_eq!(rust_resolved(m), run_oracle(m));
}

#[test]
fn oracle_rpi4() {
    let m = "Raspberry Pi 4 Model B Rev 1.5";
    assert_eq!(rust_resolved(m), run_oracle(m));
}

#[test]
fn oracle_unsupported() {
    let m = "Some Other Board";
    assert_eq!(rust_resolved(m), run_oracle(m));
}

#[test]
fn oracle_order_matters() {
    let m = "XRaspberry Pi 4XRaspberry Pi 5X";
    assert_eq!(rust_resolved(m), run_oracle(m));
}
