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

#[test]
fn directive_inside_comment_does_not_count_as_present() {
    // Regression: the PRISTINE config-rpi5.txt from the deployment tree
    // documents the mechanism in a header comment that contains the exact
    // directive string. During late shutdown /boot is already unmounted, so
    // /boot/efi/config.txt resolves to that pristine copy — a substring
    // check then skips the append and the FAT ends up with no initramfs
    // directive at all → firmware boots the kernel without initramfs →
    // VFS panic (cp-jurek brick, 2026-07-17).
    let input = "# build-all-node-disks.sh post_process_efi places kernel_2712.img + initramfs.img\n# on this EFI partition and appends `initramfs initramfs.img followkernel` here.\n\n[all]\narm_64bit=1\n";
    let result = ensure_followkernel(input);
    assert!(result.starts_with(input));
    let directive_lines = result
        .lines()
        .filter(|l| l.trim() == "initramfs initramfs.img followkernel")
        .count();
    assert_eq!(directive_lines, 1, "pristine config must gain exactly one real directive line");
}

#[test]
fn indented_directive_line_counts_as_present() {
    let input = "arm_64bit=1\n  initramfs initramfs.img followkernel\n";
    assert_eq!(ensure_followkernel(input), input);
}
