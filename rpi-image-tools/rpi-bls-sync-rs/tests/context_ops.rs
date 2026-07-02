//! T4 (RED phase): failing tests for `context::probe_deploy_root` /
//! `to_deploy_probe`. Ground truth: rpi-bls-sync.sh lines 170-172 (`_root_ok`
//! probe: `for _PFX in "" "/sysroot"; do [ -d "${_PFX}/ostree/deploy" ]`).

use rpi_bls_sync::context::{probe_deploy_root, to_deploy_probe, ExecContext};
use rpi_bls_sync::slot::DeployProbe;

#[test]
fn real_root_when_root_prefix_has_deploy() {
    assert_eq!(probe_deploy_root(&[true, false]), ExecContext::RealRoot);
}

#[test]
fn real_root_when_sysroot_prefix_has_deploy() {
    assert_eq!(probe_deploy_root(&[false, true]), ExecContext::RealRoot);
}

#[test]
fn initrd_pre_luks_when_neither_prefix_has_deploy() {
    assert_eq!(probe_deploy_root(&[false, false]), ExecContext::InitrdPreLuks);
}

#[test]
fn initrd_context_always_root_inaccessible_regardless_of_slot_presence() {
    assert_eq!(
        to_deploy_probe(ExecContext::InitrdPreLuks, true),
        DeployProbe::RootInaccessible
    );
    assert_eq!(
        to_deploy_probe(ExecContext::InitrdPreLuks, false),
        DeployProbe::RootInaccessible
    );
}

#[test]
fn real_root_with_slot_present_is_root_accessible() {
    assert_eq!(
        to_deploy_probe(ExecContext::RealRoot, true),
        DeployProbe::RootAccessible
    );
}

#[test]
fn real_root_with_slot_absent_is_root_accessible_dir_absent() {
    assert_eq!(
        to_deploy_probe(ExecContext::RealRoot, false),
        DeployProbe::RootAccessibleDirAbsent
    );
}
