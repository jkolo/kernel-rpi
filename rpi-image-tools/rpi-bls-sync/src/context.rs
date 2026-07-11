//! Execution-context classification. The ONLY switch controlling whether the
//! slot-refusal guard is enforced is data-driven (deploy-root accessibility),
//! never a CLI flag — mirrors bash's `_root_ok` probe (lines 170-172).

use crate::slot::DeployProbe;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecContext {
    /// Pre-LUKS initrd: deploy root not yet accessible. Guard is skipped —
    /// trust the `ostree=` value (firstboot-safety); ADOPT still runs.
    InitrdPreLuks,
    /// Real root (full system or post-switch-root initrd): deploy root
    /// accessible. Guard is enforced.
    RealRoot,
}

/// Probe `/ostree/deploy` under both `""` and `"/sysroot"` prefixes.
/// Mirrors: `for _PFX in "" "/sysroot"; do [ -d "${_PFX}/ostree/deploy" ] ...`.
pub fn probe_deploy_root(prefixes_have_ostree_deploy: &[bool]) -> ExecContext {
    if prefixes_have_ostree_deploy.iter().any(|&present| present) {
        ExecContext::RealRoot
    } else {
        ExecContext::InitrdPreLuks
    }
}

/// Build the `DeployProbe` that `slot::resolve` expects from the execution
/// context and the on-disk presence of BOTH candidate slot dirs. See the
/// `DeployProbe` caller contract in `slot.rs`: the live slot is the `/proc`
/// token's dir (only meaningful when adopting), the BLS slot is the BLS
/// token's dir. In `InitrdPreLuks` the guard is skipped, so the presence
/// flags are carried through but ignored by `resolve`.
pub fn to_deploy_probe(
    ctx: ExecContext,
    live_slot_present: bool,
    bls_slot_present: bool,
) -> DeployProbe {
    DeployProbe {
        root_accessible: matches!(ctx, ExecContext::RealRoot),
        live_slot_present,
        bls_slot_present,
    }
}
