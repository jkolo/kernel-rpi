//! Liveness-first `ostree=` boot-slot re-resolution.
//!
//! The boot-slot integer N in `/ostree/boot.N/...` is a TRANSIENT index that
//! ostree flips and PRUNES on bootversion changes — it is NOT part of a
//! deployment's stateroot/csum/serial identity. Two distinct races can leave
//! a written `cmdline.txt` pointing at a slot whose directory has been pruned,
//! which makes the next boot's `ostree-prepare-root` fail into dracut
//! emergency:
//!
//!   - **day-2 update race**: the chosen BLS entry names a slot whose dir
//!     still exists at write-time but is orphaned moments later by ostree's
//!     prune. The running deployment's `/proc/cmdline` slot is the stable one.
//!   - **shutdown-after-finalize**: `ostree-finalize-staged` swaps the
//!     bootversion at shutdown and prunes the OLD slot the system is still
//!     running from; here `/proc/cmdline` names the now-pruned slot and the
//!     freshly finalized BLS slot is the authoritative one.
//!
//! Because the two races pull in opposite directions, neither `/proc` nor the
//! BLS entry is always right. The robust rule is existence-driven: for the
//! same deployment, write whichever candidate slot's dir currently EXISTS,
//! preferring the live `/proc` slot when both are present (day-2 race), and
//! never write a slot whose dir is verifiably absent.

/// Existence facts gathered by the real I/O layer that the slot guard needs.
///
/// See the module doc for why BOTH candidate slots' on-disk presence matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeployProbe {
    /// Deploy root accessible (real-root). When false (pre-LUKS initrd) the
    /// existence guard is skipped entirely — firstboot-safety, trust the
    /// value — and the two `*_present` flags are ignored.
    pub root_accessible: bool,
    /// The live (`/proc`) slot's deploy dir exists. Consulted only when the
    /// BLS and live tokens name the SAME deployment via a DIFFERENT slot.
    pub live_slot_present: bool,
    /// The BLS slot's deploy dir exists — the authoritative fall-back when the
    /// live slot has been pruned (shutdown-after-finalize).
    pub bls_slot_present: bool,
}

// CALLER CONTRACT: `resolve` is pure — it cannot touch the filesystem. The
// real I/O layer must probe the deploy dirs of BOTH candidate slots and pass
// the results in `DeployProbe`:
//   - `bls_slot_present`  = dir of the BLS `ostree=` token exists
//                           (see `bls_slot_token`).
//   - `live_slot_present` = dir of the `/proc` `ostree=` token exists (only
//                           meaningful when it names the same deployment via a
//                           different slot; see `target_after_adoption`).
// A caller that probes only one of the two would re-introduce a variant of the
// write-then-prune emergency-boot bug this module exists to fix.

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
/// leaving `stateroot/csum/serial`. If the input doesn't start with the fixed
/// `ostree=/ostree/boot.` prefix, or has no `/` after it, the input is
/// returned unchanged.
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
fn extract_ostree_token(cmdline: &str) -> Option<&str> {
    cmdline
        .split_whitespace()
        .find(|tok| tok.starts_with("ostree="))
}

/// The BLS `ostree=` token as-is (the finalized-target candidate), or None if
/// the BLS-derived cmdline carries no `ostree=` token. The real I/O layer uses
/// this to know which directory to probe for `DeployProbe::bls_slot_present`.
pub fn bls_slot_token(bls_cmdline: &str) -> Option<String> {
    extract_ostree_token(bls_cmdline).map(str::to_string)
}

/// The `ostree=` token that WOULD be written if the live slot is adopted — the
/// live-adopted value when adoption applies, otherwise the original BLS value.
/// Callers use this to know WHICH directory to probe for
/// `DeployProbe::live_slot_present`. Returns `None` if `bls_cmdline` carries no
/// `ostree=` token at all.
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

/// Resolve the `ostree=` token to write into cmdline.txt. See the module doc
/// and the `DeployProbe` caller contract above.
pub fn resolve(bls_cmdline: &str, proc_cmdline: &str, probe: DeployProbe) -> Resolution {
    let Some(o_arg) = extract_ostree_token(bls_cmdline) else {
        // No ostree= token in the BLS-derived cmdline at all: no adoption, no
        // guard, cmdline passes through untouched.
        return Resolution::Keep;
    };

    // Adoption candidate: the live /proc slot, IFF it names the SAME
    // deployment (stateroot/csum/serial) as BLS via a DIFFERENT slot integer.
    let live = extract_ostree_token(proc_cmdline)
        .filter(|p| *p != o_arg && strip_slot_prefix(p) == strip_slot_prefix(o_arg));

    if !probe.root_accessible {
        // Pre-LUKS initrd: deploy dirs are not inspectable. Firstboot-safety —
        // trust the value, adopting the live slot when there is a candidate.
        return match live {
            Some(p) => Resolution::Rewrite(bls_cmdline.replace(o_arg, p)),
            None => Resolution::Keep,
        };
    }

    // Real root: write the slot whose deploy dir EXISTS.
    match live {
        // Same deployment, different slot. Prefer the live slot while it is
        // still present (day-2 race). If it has been pruned, fall back to the
        // finalized BLS slot (shutdown-after-finalize). If neither is present,
        // refuse rather than write a slot that will vanish.
        Some(p) if probe.live_slot_present => Resolution::Rewrite(bls_cmdline.replace(o_arg, p)),
        Some(_) if probe.bls_slot_present => Resolution::Keep,
        Some(_) => Resolution::RefuseExit0,
        // No adoption (same value, different deployment, or no /proc token):
        // the target is the BLS slot as-is — validated by its own presence.
        None if probe.bls_slot_present => Resolution::Keep,
        None => Resolution::RefuseExit0,
    }
}
