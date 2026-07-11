//! Unit tests for `model::parse_model` (RPi board → EFI kernel name).

use rpi_bls_sync::model::{parse_model, KernelName, UnsupportedModel};

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
    // /proc/device-tree/model is NUL-terminated; parse_model strips it before
    // the substring match.
    assert_eq!(
        parse_model("Raspberry Pi 5 Model B Rev 1.0\0"),
        Ok(KernelName::Rpi5)
    );
}

#[test]
fn rpi5_checked_before_rpi4() {
    // Substring match order matters: "Raspberry Pi 5" must be checked BEFORE
    // "Raspberry Pi 4". Contrived string containing both substrings proves
    // the order, not just that each is individually detected.
    assert_eq!(
        parse_model("XRaspberry Pi 4XRaspberry Pi 5X"),
        Ok(KernelName::Rpi5)
    );
}
