//! NEW (not in bash): runtime sync of `config.txt` from the active deploy's
//! `<deploy_dir>/boot/efi/config.txt` to the EFI partition, appending the
//! direct-kernel-boot `followkernel` directive if not already present.
//! Idempotency must be checked AFTER the append (double-append guard).

const FOLLOWKERNEL_DIRECTIVE: &str = "initramfs initramfs.img followkernel";

/// Append the direct-kernel-boot block if `config.txt` doesn't already carry
/// `initramfs initramfs.img followkernel`. Idempotent: calling this twice on
/// already-appended content must not append again — checked by the presence
/// of the DIRECTIVE line itself, not the comment text (which may differ
/// between the build-time writer and this runtime one).
pub fn ensure_followkernel(config_txt: &str) -> String {
    if config_txt.contains(FOLLOWKERNEL_DIRECTIVE) {
        return config_txt.to_string();
    }
    format!("{config_txt}\n# Direct kernel boot (rpi-bls-sync):\n{FOLLOWKERNEL_DIRECTIVE}\n")
}
