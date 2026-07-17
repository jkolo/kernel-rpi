//! NEW (not in bash): runtime sync of `config.txt` from the active deploy's
//! `<deploy_dir>/boot/efi/config.txt` to the EFI partition, appending the
//! direct-kernel-boot `followkernel` directive if not already present.
//! Idempotency must be checked AFTER the append (double-append guard).

const FOLLOWKERNEL_DIRECTIVE: &str = "initramfs initramfs.img followkernel";

/// Append the direct-kernel-boot block if `config.txt` doesn't already carry
/// `initramfs initramfs.img followkernel`. Idempotent: calling this twice on
/// already-appended content must not append again.
///
/// The check is LINE-anchored (trimmed, non-comment lines only) — a plain
/// substring check matches the directive quoted inside the documentation
/// header of the pristine config-rpi{4,5}.txt, which is exactly what this
/// function sees during late shutdown when /boot is already unmounted and
/// /boot/efi/config.txt resolves to the deployment's pristine copy. The
/// substring match then skipped the append and the FAT lost its initramfs
/// directive → kernel booted without initrd → VFS panic (cp-jurek, 2026-07-17).
pub fn ensure_followkernel(config_txt: &str) -> String {
    let present = config_txt.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.starts_with('#') && trimmed == FOLLOWKERNEL_DIRECTIVE
    });
    if present {
        return config_txt.to_string();
    }
    format!("{config_txt}\n# Direct kernel boot (rpi-bls-sync):\n{FOLLOWKERNEL_DIRECTIVE}\n")
}
