//! T5 (RED phase): failing tests for `slot::target_after_adoption` — the
//! caller-facing helper that closes the T2 caller-contract gap (real I/O
//! must probe the POST-adoption directory, not naively the original BLS
//! one; this pure function tells the caller which `ostree=` value that is,
//! BEFORE any filesystem check happens).

use rpi_bls_sync::slot::target_after_adoption;

#[test]
fn returns_live_slot_when_same_deployment() {
    let bls = "ostree=/ostree/boot.0/rhcos/CSUM/0";
    let proc = "ostree=/ostree/boot.1/rhcos/CSUM/0";
    assert_eq!(
        target_after_adoption(bls, proc),
        Some("ostree=/ostree/boot.1/rhcos/CSUM/0".to_string())
    );
}

#[test]
fn returns_original_bls_when_different_deployment() {
    let bls = "ostree=/ostree/boot.0/rhcos/CSUM_A/0";
    let proc = "ostree=/ostree/boot.1/rhcos/CSUM_B/0";
    assert_eq!(
        target_after_adoption(bls, proc),
        Some("ostree=/ostree/boot.0/rhcos/CSUM_A/0".to_string())
    );
}

#[test]
fn returns_none_when_no_ostree_token() {
    assert_eq!(target_after_adoption("root=/dev/mapper/root", "ostree=/ostree/boot.0/x/y/0"), None);
}

#[test]
fn matches_resolve_internal_decision() {
    // The value target_after_adoption predicts must be exactly the ostree=
    // token embedded in whatever resolve() would produce, for every
    // DeployProbe outcome that doesn't refuse.
    use rpi_bls_sync::slot::{resolve, DeployProbe, Resolution};

    let bls = "root=/dev/mapper/root ostree=/ostree/boot.0/rhcos/CSUM/0 console=ttyS0";
    let proc = "ostree=/ostree/boot.1/rhcos/CSUM/0";
    let target = target_after_adoption(bls, proc).unwrap();

    match resolve(bls, proc, DeployProbe::RootAccessible) {
        Resolution::Rewrite(new_cmdline) => assert!(new_cmdline.contains(&target)),
        other => panic!("expected Rewrite, got {other:?}"),
    }
}
