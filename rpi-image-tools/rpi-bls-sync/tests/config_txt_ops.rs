//! T4 (RED phase): failing tests for `config_txt::ensure_followkernel`.
//! NEW capability (no bash runtime equivalent) — mirrors the build-time
//! append block in bootstrap-cluster/scripts/build-node-disks.sh:216-220
//! (`initramfs initramfs.img followkernel` directive), adapted for
//! idempotent runtime application.

use rpi_bls_sync::config_txt::ensure_followkernel;

#[test]
fn appends_directive_when_absent() {
    let input = "dtoverlay=vc4-kms-v3d\narm_64bit=1\n";
    let result = ensure_followkernel(input);
    assert!(result.starts_with(input));
    assert!(result.contains("initramfs initramfs.img followkernel"));
}

#[test]
fn idempotent_no_double_append() {
    let input = "dtoverlay=vc4-kms-v3d\narm_64bit=1\n";
    let once = ensure_followkernel(input);
    let twice = ensure_followkernel(&once);
    assert_eq!(once, twice);
    assert_eq!(
        once.matches("initramfs initramfs.img followkernel").count(),
        1
    );
}

#[test]
fn already_present_from_build_time_is_noop() {
    // A disk built via build-node-disks.sh already carries the directive —
    // the runtime sync must not double it.
    let input = "arm_64bit=1\n\n# Direct kernel boot (added by build-node-disks.sh post_process_efi):\ninitramfs initramfs.img followkernel\n";
    assert_eq!(ensure_followkernel(input), input);
}
