#!/usr/bin/env bash
# Dracut module: 99rpi-bls-sync
# Installs the BLS→EFI sync binary and shutdown service into the initrd.
# Only the shutdown fallback is needed in initrd — the path unit is full-system.

check() {
    # Only install on ARM (Raspberry Pi images).
    [[ "$(uname -m)" == "aarch64" ]] || return 1
    return 0
}

depends() {
    echo "systemd"
}

install() {
    # Native binary (rpi-bls-sync-rs) replaces the old bash+awk script —
    # retires the whole 203/EXEC fragility class (interpreter + shebang mode
    # dependency) rather than mitigating it. inst_binary resolves and drags
    # in the binary's shared-library dependencies (libc, libgcc_s) the same
    # way it would for any other ELF binary.
    inst_binary /usr/sbin/rpi-bls-sync
    # Belt-and-suspenders: guarantee the initramfs copy is executable
    # regardless of whether the source RPM's mode survives packaging
    # (rpi-image-tools.spec installs it -m 0755; a 0644 source here would
    # still hit 203/EXEC since a direct binary execve has no bash-wrapper
    # open/read fallback the way the old script did).
    chmod 0755 "$initdir/usr/sbin/rpi-bls-sync"

    # mount(2) (used directly by the binary — no exec of mount(8)/blkid)
    # does NOT autoload filesystem modules the way mount(8) does; without
    # this, mounting the ext4 boot partition or the vfat EFI partition in
    # initrd fails outright.
    instmods vfat ext4

    # Shutdown/switch-root fallback: runs before any reboot or switch-root.
    inst_simple "$moddir/rpi-bls-sync-shutdown.service" \
        /usr/lib/systemd/system/rpi-bls-sync-shutdown.service
    systemctl --root "$initdir" enable rpi-bls-sync-shutdown.service

    # Firstboot diskful service: runs after ignition-disks + coreos-boot-edit
    # so the updated BLS (boot.0 + LUKS params) is synced to cmdline.txt.
    inst_simple "$moddir/rpi-bls-sync-diskful.service" \
        /usr/lib/systemd/system/rpi-bls-sync-diskful.service
    systemctl --root "$initdir" enable rpi-bls-sync-diskful.service

    # Static DNS in initrd: dracut uses ip=auto (SLAAC) which gives IPv6
    # connectivity, but RDNSS from OPNsense RA is not processed without NM.
    # Without /etc/resolv.conf clevis cannot resolve Tang hostname → LUKS
    # unlock fails. Bake in OPNsense jurek DNS (EUI-64 from MAC, stable).
    mkdir -p "$initdir/etc"
    printf 'nameserver fd34:1114:3800:2689:f690:eaff:fe01:e99\nsearch tartarus.kolosowscy.pl kolosowscy.pl\n' \
        > "$initdir/etc/resolv.conf"
}
