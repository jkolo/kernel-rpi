//! Unit tests for `slot::resolve` — liveness-first `ostree=` boot-slot
//! re-resolution that fixes the write-then-prune emergency-boot bugs (both the
//! day-2 update race and the shutdown-after-finalize race).

use rpi_bls_sync::slot::{resolve, strip_slot_prefix, DeployProbe, Resolution};

/// Concise `DeployProbe` builder: (root_accessible, live_present, bls_present).
fn probe(root: bool, live: bool, bls: bool) -> DeployProbe {
    DeployProbe {
        root_accessible: root,
        live_slot_present: live,
        bls_slot_present: bls,
    }
}

#[test]
fn adopts_live_slot_when_present_same_deployment() {
    // day-2 race: BLS says boot.0, /proc says the same deployment lives at
    // boot.1, and boot.1 is present -> prefer the live slot.
    let bls = "root=/dev/mapper/root rw ostree=/ostree/boot.0/rhcos/CSUM/0 console=ttyS0";
    let proc = "ostree=/ostree/boot.1/rhcos/CSUM/0";
    assert_eq!(
        resolve(bls, proc, probe(true, true, true)),
        Resolution::Rewrite(
            "root=/dev/mapper/root rw ostree=/ostree/boot.1/rhcos/CSUM/0 console=ttyS0".to_string()
        )
    );
}

#[test]
fn falls_back_to_bls_when_live_slot_pruned() {
    // shutdown-after-finalize (THE FIX): /proc still names the OLD live slot
    // boot.1, but ostree-finalize-staged swapped the bootversion and pruned
    // boot.1; the finalized BLS slot boot.0 is present -> write BLS boot.0
    // rather than refusing (which would leave a stale cmdline.txt and brick
    // the next boot).
    let bls = "root=/dev/mapper/root rw ostree=/ostree/boot.0/rhcos/CSUM/0 console=ttyS0";
    let proc = "ostree=/ostree/boot.1/rhcos/CSUM/0";
    assert_eq!(
        resolve(bls, proc, probe(true, /*live*/ false, /*bls*/ true)),
        Resolution::Keep
    );
}

#[test]
fn refuses_when_both_candidate_slots_absent() {
    // Adoption candidate exists (same deployment, different slot) but NEITHER
    // the live nor the BLS slot dir is present -> refuse rather than write a
    // slot that will vanish.
    let bls = "ostree=/ostree/boot.0/rhcos/CSUM/0";
    let proc = "ostree=/ostree/boot.1/rhcos/CSUM/0";
    assert_eq!(
        resolve(bls, proc, probe(true, false, false)),
        Resolution::RefuseExit0
    );
}

#[test]
fn keeps_when_different_deployment() {
    // Different stateroot/csum/serial -> no adoption; the BLS value is kept as
    // long as its own dir is present.
    let bls = "ostree=/ostree/boot.0/rhcos/CSUM_A/0";
    let proc = "ostree=/ostree/boot.1/rhcos/CSUM_B/0";
    assert_eq!(resolve(bls, proc, probe(true, false, true)), Resolution::Keep);
}

#[test]
fn refuses_absent_bls_slot_in_realroot() {
    // No adoption (bls == proc), BLS slot dir absent -> refuse.
    let bls = "ostree=/ostree/boot.0/rhcos/GONE/0";
    let proc = "ostree=/ostree/boot.0/rhcos/GONE/0";
    assert_eq!(
        resolve(bls, proc, probe(true, false, false)),
        Resolution::RefuseExit0
    );
}

#[test]
fn initrd_trusts_value_no_refuse() {
    // Pre-LUKS initrd: deploy root not accessible -> guard skipped entirely
    // (firstboot-safety); must NEVER RefuseExit0 even if presence flags are
    // false.
    let bls = "ostree=/ostree/boot.0/rhcos/GONE/0";
    let proc = "ostree=/ostree/boot.0/rhcos/GONE/0";
    let r = resolve(bls, proc, probe(false, false, false));
    assert_ne!(r, Resolution::RefuseExit0);
    assert_eq!(r, Resolution::Keep);
}

#[test]
fn initrd_adopts_live_slot_trusting_presence_flags() {
    // Pre-LUKS initrd with an adoption candidate: adopt the live slot on trust
    // even though presence can't be verified.
    let bls = "root=/dev/mapper/root ostree=/ostree/boot.0/rhcos/CSUM/0";
    let proc = "ostree=/ostree/boot.1/rhcos/CSUM/0";
    assert_eq!(
        resolve(bls, proc, probe(false, false, false)),
        Resolution::Rewrite("root=/dev/mapper/root ostree=/ostree/boot.1/rhcos/CSUM/0".to_string())
    );
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
    assert_eq!(strip_slot_prefix("ostree=weird-value"), "ostree=weird-value");
}

#[test]
fn no_ostree_token_keeps_cmdline_untouched() {
    let bls = "root=/dev/mapper/root rw console=ttyS0";
    let proc = "ostree=/ostree/boot.0/rhcos/CSUM/0";
    assert_eq!(resolve(bls, proc, probe(true, true, true)), Resolution::Keep);
}

#[test]
fn same_value_in_bls_and_proc_no_adopt_needed() {
    let bls = "ostree=/ostree/boot.0/rhcos/CSUM/0";
    let proc = "ostree=/ostree/boot.0/rhcos/CSUM/0";
    // _PROC == _OARG already -> adopt branch skipped; BLS slot present -> Keep.
    assert_eq!(resolve(bls, proc, probe(true, false, true)), Resolution::Keep);
}
