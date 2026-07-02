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

/// Given the execution context and whether the target slot dir exists under
/// either prefix, build the `DeployProbe` that `slot::resolve` expects. See
/// the `DeployProbe` doc comment in `slot.rs` for the caller contract on
/// which directory `slot_dir_present` must reflect (the post-adoption
/// target, not naively the original BLS one).
pub fn to_deploy_probe(ctx: ExecContext, slot_dir_present: bool) -> DeployProbe {
    match ctx {
        ExecContext::InitrdPreLuks => DeployProbe::RootInaccessible,
        ExecContext::RealRoot if slot_dir_present => DeployProbe::RootAccessible,
        ExecContext::RealRoot => DeployProbe::RootAccessibleDirAbsent,
    }
}
