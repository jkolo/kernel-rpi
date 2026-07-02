//! Liveness-first `ostree=` boot-slot re-resolution.
//! Ground truth: rpi-bls-sync.sh lines 135-182.
//!
//! The boot-slot integer N in `/ostree/boot.N/...` is a TRANSIENT index that
//! ostree flips and PRUNES on bootversion changes — it is NOT part of a
//! deployment's stateroot/csum/serial identity. The chosen BLS entry can
//! carry a slot whose directory still exists at write-time but is orphaned
//! moments later by ostree's prune (write-then-prune race) → next boot's
//! ostree-prepare-root can't find it → dracut emergency.
//!
//! `/proc/cmdline` carries the `ostree=` that ostree-prepare-root ACTUALLY
//! used for the running deployment — authoritative for the slot. Whenever
//! the BLS entry refers to the SAME deployment as what is running, adopt the
//! LIVE slot from `/proc`, regardless of whether the BLS slot dir still
//! exists. Then refuse to write any `ostree=` whose dir is verifiably absent.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeployProbe {
    /// Deploy root accessible (real-root) and the slot directory exists.
    RootAccessible,
    /// Deploy root accessible (real-root) and the slot directory is absent.
    RootAccessibleDirAbsent,
    /// Deploy root NOT accessible (pre-LUKS initrd) — guard is skipped,
    /// firstboot-safety: never RefuseExit0.
    RootInaccessible,
}

// CALLER CONTRACT (matters for T4/T5 — the mounts/orchestration layer that
// will actually perform the I/O existence checks feeding `DeployProbe`):
// bash computes `_GDIR` (the directory existence-checked by the guard) from
// `_OARG` AFTER the adopt decision — i.e. from the LIVE slot's path when
// adoption happens, from the ORIGINAL BLS path when it doesn't. `resolve()`
// is pure and cannot do that I/O check itself, so the caller MUST determine
// which path the guard should check. In practice this is nearly always the
// live slot (adoption happens) or the original BLS entry (it doesn't) — a
// caller that always probes the ORIGINAL BLS path regardless of adoption
// would silently re-introduce a variant of the write-then-prune bug this
// module exists to fix. `strip_slot_prefix` + a same-deployment comparison
// (as `resolve` performs internally) is exactly what a real caller needs to
// replicate to pick the correct path to probe before calling `resolve`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Adopt the live slot from /proc/cmdline; cmdline.txt's `ostree=` is rewritten.
    Rewrite(String),
    /// BLS `ostree=` is kept as-is (different deployment, or no live adoption needed).
    Keep,
    /// Refuse to sync at all — leave the last-good cmdline.txt untouched (exit 0).
    RefuseExit0,
}

/// Strip the `/ostree/boot.N/` slot prefix from an `ostree=` cmdline token,
/// leaving `stateroot/csum/serial`. Mirrors bash `${_OARG#ostree=/ostree/boot.*/}`
/// (shell `#` glob: SHORTEST match of `ostree=/ostree/boot.*/` — i.e. the
/// first `/` encountered after `boot.` closes the match). If the input
/// doesn't start with the fixed `ostree=/ostree/boot.` prefix, or has no
/// `/` after it, the pattern doesn't match at all and bash `#pattern`
/// returns the input unchanged — replicated here.
pub fn strip_slot_prefix(ostree_arg: &str) -> String {
    const PREFIX: &str = "ostree=/ostree/boot.";
    let Some(after_prefix) = ostree_arg.strip_prefix(PREFIX) else {
        return ostree_arg.to_string();
    };
    match after_prefix.find('/') {
        Some(slash_idx) => after_prefix[slash_idx + 1..].to_string(),
        None => ostree_arg.to_string(),
    }
}

/// First `ostree=[^ ]*` whitespace-delimited token in `cmdline`, if any.
/// Mirrors `grep -o 'ostree=[^ ]*'` (BLS side takes it as-is; the /proc side
/// additionally pipes through `head -1`, but both reduce to "first match" on
/// a cmdline that in practice carries at most one `ostree=` token).
fn extract_ostree_token(cmdline: &str) -> Option<&str> {
    cmdline
        .split_whitespace()
        .find(|tok| tok.starts_with("ostree="))
}

/// The `ostree=` token that WOULD be written, before the existence guard is
/// applied — the live-adopted value when adoption applies, otherwise the
/// original BLS value. Callers (the real I/O layer) use this to know WHICH
/// directory to probe via actual filesystem access before building the
/// `DeployProbe` to pass to `resolve`. Returns `None` if `bls_cmdline`
/// carries no `ostree=` token at all (mirrors `resolve`'s early Keep).
pub fn target_after_adoption(bls_cmdline: &str, proc_cmdline: &str) -> Option<String> {
    let o_arg = extract_ostree_token(bls_cmdline)?;
    if let Some(p_arg) = extract_ostree_token(proc_cmdline)
        && p_arg != o_arg
    {
        let o_rest = strip_slot_prefix(o_arg);
        let p_rest = strip_slot_prefix(p_arg);
        if p_rest == o_rest {
            return Some(p_arg.to_string());
        }
    }
    Some(o_arg.to_string())
}

/// Resolve the `ostree=` token to write into cmdline.txt. See the
/// `DeployProbe` doc comment above for the caller contract on what `probe`
/// must reflect.
pub fn resolve(bls_cmdline: &str, proc_cmdline: &str, probe: DeployProbe) -> Resolution {
    let Some(o_arg) = extract_ostree_token(bls_cmdline) else {
        // No ostree= token in the BLS-derived cmdline at all: bash's
        // `if [ -n "$_OARG" ]; then ... fi` block is skipped entirely — no
        // adoption, no guard, cmdline passes through untouched.
        return Resolution::Keep;
    };

    let mut adopted_arg: Option<&str> = None;
    if let Some(p_arg) = extract_ostree_token(proc_cmdline)
        && p_arg != o_arg
    {
        let o_rest = strip_slot_prefix(o_arg);
        let p_rest = strip_slot_prefix(p_arg);
        if p_rest == o_rest {
            // Same deployment, stale slot in BLS -> adopt the live slot.
            adopted_arg = Some(p_arg);
        }
        // else: BLS refers to a DIFFERENT deployment (legitimately staged
        // next-boot); keep o_arg and let the guard validate it.
    }

    // Final guard: only enforced when the ostree root is actually
    // accessible (pre-LUKS initrd: trust the value, firstboot-safety).
    if probe == DeployProbe::RootAccessibleDirAbsent {
        return Resolution::RefuseExit0;
    }

    match adopted_arg {
        Some(new_arg) => Resolution::Rewrite(bls_cmdline.replace(o_arg, new_arg)),
        None => Resolution::Keep,
    }
}
