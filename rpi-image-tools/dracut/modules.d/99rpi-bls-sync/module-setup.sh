#!/usr/bin/env bash
# Dracut module: 99rpi-bls-sync
# Installs the BLS→EFI sync script and shutdown service into the initrd.
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
    inst_script "$moddir/rpi-bls-sync.sh" /usr/sbin/rpi-bls-sync.sh

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
