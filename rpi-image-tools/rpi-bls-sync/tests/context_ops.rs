//! Unit tests for `context::probe_deploy_root` / `to_deploy_probe`
//! (deploy-root accessibility probe + `DeployProbe` assembly).

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
fn initrd_context_is_root_inaccessible_carrying_presence_flags() {
    // The guard is skipped in initrd; the presence flags are carried through
    // (resolve ignores them) but root_accessible must be false.
    assert_eq!(
        to_deploy_probe(ExecContext::InitrdPreLuks, true, false),
        DeployProbe {
            root_accessible: false,
            live_slot_present: true,
            bls_slot_present: false,
        }
    );
    assert_eq!(
        to_deploy_probe(ExecContext::InitrdPreLuks, false, true),
        DeployProbe {
            root_accessible: false,
            live_slot_present: false,
            bls_slot_present: true,
        }
    );
}

#[test]
fn real_root_carries_both_presence_flags() {
    assert_eq!(
        to_deploy_probe(ExecContext::RealRoot, true, false),
        DeployProbe {
            root_accessible: true,
            live_slot_present: true,
            bls_slot_present: false,
        }
    );
    assert_eq!(
        to_deploy_probe(ExecContext::RealRoot, false, true),
        DeployProbe {
            root_accessible: true,
            live_slot_present: false,
            bls_slot_present: true,
        }
    );
}
