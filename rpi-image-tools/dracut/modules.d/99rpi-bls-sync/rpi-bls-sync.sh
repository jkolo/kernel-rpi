#!/usr/bin/bash
# Sync ostree's current default BLS entry (kernel + initramfs + cmdline) to
# /boot/efi/ so that Raspberry Pi firmware (which reads EFI FAT32 directly
# with no UEFI/GRUB chain) boots the correct deployment.
#
# cmdline.txt is a REWRITE from BLS options + /etc/cmdline.d/*.conf —
# identical architecture to GRUB2 on x86 RHCOS. Never a partial patch.
#
# Works in three contexts:
#   1. Full system  — /boot and /boot/efi already mounted
#   2. initrd post-switch-root — /sysroot/boot mounted
#   3. initrd post-ignition-disks (firstboot) — partitions unmounted;
#      script auto-mounts by PARTLABEL
#
# Idempotent: skips work if kernel, initramfs, and cmdline already match.
set -euo pipefail

# --- Detect RPi model ---
MODEL=$(tr -d '\0' </proc/device-tree/model 2>/dev/null || echo "")
case "$MODEL" in
    *"Raspberry Pi 5"*) KERNEL_NAME=kernel_2712.img ;;
    *"Raspberry Pi 4"*) KERNEL_NAME=kernel8.img ;;
    *)
        echo "rpi-bls-sync: unsupported model '${MODEL}', skipping" >&2
        exit 0
        ;;
esac

# --- Locate boot partition (ext4) ---
BOOT_MOUNT=""
BOOT_TMP=""

if mountpoint -q /boot 2>/dev/null; then
    BOOT_MOUNT=/boot
elif mountpoint -q /sysroot/boot 2>/dev/null; then
    BOOT_MOUNT=/sysroot/boot
else
    BOOT_DEV=$(blkid -t PARTLABEL=boot -o device 2>/dev/null || true)
    if [ -z "$BOOT_DEV" ]; then
        echo "rpi-bls-sync: boot partition not found, skipping" >&2
        exit 0
    fi
    BOOT_TMP=$(mktemp -d)
    mount -t ext4 -o ro "$BOOT_DEV" "$BOOT_TMP"
    BOOT_MOUNT="$BOOT_TMP"
fi

EFI_TMP=""
EFI_MOUNT=""

cleanup() {
    [ -n "$BOOT_TMP" ] && { umount "$BOOT_TMP" 2>/dev/null; rmdir "$BOOT_TMP" 2>/dev/null || true; }
    [ -n "$EFI_TMP"  ] && { umount "$EFI_TMP"  2>/dev/null; rmdir "$EFI_TMP"  2>/dev/null || true; }
}
trap cleanup EXIT

# --- Find the DEFAULT BLS entry: highest `version` field (BLS spec) ---
# ostree assigns the default deployment (index 0, booted next) the HIGHEST
# `version`; bootloaders (GRUB on x86 RHCOS) sort entries by version descending
# and boot the first. The filename counter (ostree-N.conf) is NOT the boot order:
# after an in-place update there are 2+ entries and the LOWEST filename is the
# OLDEST deployment. Picking `sort -V | head -1` synced the old OS → the node
# rebooted straight back to the previous image (root cause of a stuck CP roll on
# RPi: rebase staged the new deployment but the EFI cmdline kept pointing at the
# old one). Select by max `version` to match GRUB's default-entry semantics.
BLS=""; BEST_VER=-1
for _e in "$BOOT_MOUNT"/loader/entries/ostree-*.conf; do
    [ -f "$_e" ] || continue
    _v=$(awk '/^version[[:space:]]/ { print $2; exit }' "$_e")
    _v=${_v:-0}
    if [ "$_v" -gt "$BEST_VER" ] 2>/dev/null; then BEST_VER=$_v; BLS=$_e; fi
done
if [ -z "$BLS" ] || [ ! -f "$BLS" ]; then
    echo "rpi-bls-sync: no BLS entry found under $BOOT_MOUNT/loader/entries/" >&2
    exit 0
fi

LINUX_REL=$(awk  '/^linux /  { print $2; exit }' "$BLS")
INITRD_REL=$(awk '/^initrd / { print $2; exit }' "$BLS")
BLS_OPTIONS=$(awk '/^options / { sub(/^options +/,""); print; exit }' "$BLS")

# BLS paths are absolute (/boot/ostree/...).
# When boot is tmp-mounted, prepend mount point.
if [ -n "$BOOT_TMP" ]; then
    SRC_KERNEL="${BOOT_MOUNT}${LINUX_REL}"
    SRC_INITRAMFS="${BOOT_MOUNT}${INITRD_REL}"
else
    SRC_KERNEL="${LINUX_REL}"
    SRC_INITRAMFS="${INITRD_REL}"
fi

if [ ! -f "$SRC_KERNEL" ]; then
    echo "rpi-bls-sync: kernel not found: $SRC_KERNEL" >&2
    exit 1
fi
if [ ! -f "$SRC_INITRAMFS" ]; then
    echo "rpi-bls-sync: initramfs not found: $SRC_INITRAMFS" >&2
    exit 1
fi

# --- Expand $ignition_firstboot ---
if [ -f "$BOOT_MOUNT/ignition.firstboot" ]; then
    FIRSTBOOT="ignition.firstboot=1"
else
    FIRSTBOOT=""
fi
CMDLINE=$(printf '%s' "$BLS_OPTIONS" \
    | sed "s/\\\$ignition_firstboot/${FIRSTBOOT}/g" \
    | tr -s ' ')

# --- Append /etc/cmdline.d/*.conf (same as GRUB2 on x86 RHCOS) ---
# In initrd look in /sysroot/etc as well (ostree deploy).
CMDLINE_EXTRA=""
for f in /etc/cmdline.d/*.conf /sysroot/etc/cmdline.d/*.conf; do
    [ -f "$f" ] && CMDLINE_EXTRA="$CMDLINE_EXTRA $(cat "$f")"
done
CMDLINE="$CMDLINE $CMDLINE_EXTRA"

# --- Dedup kernel args (order-preserving, keep-first whole token) ---
# BLS options= (rpm-ostree kargs) and /etc/cmdline.d/*.conf can carry the SAME
# args (cgroup/swap/console) in BOTH layers → duplicated tokens in cmdline.txt
# (observed: cgroup_no_v1="all" + console=* doubled). Keep the first occurrence
# of each whole token; this preserves DISTINCT console= values and their order
# (the LAST console= is the primary device). NO sort -u, NO dedup-by-key. Runs
# BEFORE the clock_usec append so the per-run timestamp is never a dedup input.
CMDLINE=$(printf '%s' "$CMDLINE" | tr ' ' '\n' | awk 'NF && !seen[$0]++' | tr '\n' ' ' | sed 's/ $//')

# --- Inject fresh wall-clock (RPi has no RTC) ---
# Remove any existing systemd.clock_usec= first, then append a fresh value.
CMDLINE=$(printf '%s' "$CMDLINE" | sed 's/systemd\.clock_usec=[^ ]*//g')
CMDLINE="$CMDLINE systemd.clock_usec=$(date +%s%6N)"

CMDLINE=$(printf '%s' "$CMDLINE" | tr -s ' ' | sed 's/^ //;s/ $//')

# --- Re-resolve ostree= boot-slot against the LIVE deployment (liveness-first) ---
# The boot-slot integer N in /ostree/boot.N/... is a TRANSIENT index that ostree
# flips on bootversion changes and PRUNES — it is NOT part of the deployment's
# stateroot/csum/serial identity. The chosen BLS entry can carry a slot whose
# directory still exists at write-time but is orphaned moments later by ostree's
# prune (write-then-prune race) → the next boot's ostree-prepare-root can't find
# it → dracut emergency. (Root cause of cp1/w1 going to emergency: a sync wrote
# boot.0 while boot.0 dir still existed; ostree then pruned boot.0.)
#
# /proc/cmdline carries the ostree= that ostree-prepare-root ACTUALLY used to
# mount THIS running deployment — authoritative for the slot. So whenever the BLS
# entry refers to the SAME deployment (stateroot/csum/serial) as what is running,
# adopt the LIVE slot from /proc — REGARDLESS of whether the BLS slot dir still
# exists. (The previous existence-gate let the stale slot through whenever its
# dir had not been pruned yet, which is exactly when the race bites.) Then refuse
# to write any ostree= whose dir is verifiably absent.
_OARG=$(printf '%s' "$CMDLINE" | grep -o 'ostree=[^ ]*' || true)
if [ -n "$_OARG" ]; then
    _OREST="${_OARG#ostree=/ostree/boot.*/}"         # stateroot/csum/serial (slot-stripped)
    _PROC=$(grep -o 'ostree=[^ ]*' /proc/cmdline 2>/dev/null | head -1 || true)
    if [ -n "$_PROC" ] && [ "$_PROC" != "$_OARG" ]; then
        _PREST="${_PROC#ostree=/ostree/boot.*/}"     # live stateroot/csum/serial
        if [ "$_PREST" = "$_OREST" ]; then
            # Same deployment, stale slot in BLS → adopt the LIVE boot-slot.
            echo "rpi-bls-sync: adopting live boot-slot ${_OARG} -> ${_PROC}" >&2
            CMDLINE=$(printf '%s' "$CMDLINE" | sed "s|${_OARG}|${_PROC}|g")
            _OARG="$_PROC"
        fi
        # else: BLS refers to a DIFFERENT deployment (legitimately staged
        #       next-boot); keep _OARG and let the existence guard validate it.
    fi
    # Final guard: never write an ostree= whose deployment dir is absent — that
    # bricks the boot. Only enforce when the ostree root is actually accessible
    # (in initrd pre-LUKS we cannot verify; trust the value so firstboot is not
    # broken). On absence, leave the last-good cmdline.txt untouched and exit.
    _GDIR=$(dirname "${_OARG#ostree=}")
    _root_ok=0
    for _PFX in "" "/sysroot"; do [ -d "${_PFX}/ostree/deploy" ] && { _root_ok=1; break; }; done
    if [ "$_root_ok" = "1" ]; then
        _present=0
        for _PFX in "" "/sysroot"; do [ -d "${_PFX}${_GDIR}" ] && { _present=1; break; }; done
        if [ "$_present" = "0" ]; then
            echo "rpi-bls-sync: refusing to write absent ostree slot ${_OARG}; keeping current cmdline.txt" >&2
            exit 0
        fi
    fi
fi
unset _OARG _OREST _PROC _PREST _GDIR _root_ok _present _PFX

# --- Locate EFI partition (FAT32) ---
if mountpoint -q /boot/efi 2>/dev/null; then
    EFI_MOUNT=/boot/efi
elif mountpoint -q /sysroot/boot/efi 2>/dev/null; then
    EFI_MOUNT=/sysroot/boot/efi
else
    EFI_DEV=$(blkid -t PARTLABEL=EFI-SYSTEM -o device 2>/dev/null \
              || blkid -t PARTLABEL="EFI System Partition" -o device 2>/dev/null \
              || true)
    if [ -z "$EFI_DEV" ]; then
        echo "rpi-bls-sync: EFI partition not found" >&2
        exit 1
    fi
    EFI_TMP=$(mktemp -d)
    mount -t vfat "$EFI_DEV" "$EFI_TMP"
    EFI_MOUNT="$EFI_TMP"
fi

# --- Idempotency check ---
# Use file sizes + cmdline comparison (cmp may not exist in minimal initrd).
NEED_SYNC=0
SRC_K_SIZE=$(stat -c%s "$SRC_KERNEL")
SRC_I_SIZE=$(stat -c%s "$SRC_INITRAMFS")
DST_K_SIZE=$(stat -c%s "$EFI_MOUNT/$KERNEL_NAME" 2>/dev/null || echo 0)
DST_I_SIZE=$(stat -c%s "$EFI_MOUNT/initramfs.img" 2>/dev/null || echo 0)
[ "$SRC_K_SIZE" = "$DST_K_SIZE" ] || NEED_SYNC=1
[ "$SRC_I_SIZE" = "$DST_I_SIZE" ] || NEED_SYNC=1
# Compare WITHOUT the per-run systemd.clock_usec= — RPi has no RTC so it changes
# every invocation; comparing it would force a rewrite on every trigger (flapping).
_strip_clock() { printf '%s' "$1" | sed 's/systemd\.clock_usec=[^ ]*//g' | tr -s ' ' | sed 's/^ //;s/ $//'; }
CURRENT_CMDLINE=$(tr -d '\n' <"$EFI_MOUNT/cmdline.txt" 2>/dev/null || echo "")
[ "$(_strip_clock "$CURRENT_CMDLINE")" = "$(_strip_clock "$CMDLINE")" ] || NEED_SYNC=1

if [ "$NEED_SYNC" = "0" ]; then
    echo "rpi-bls-sync: already in sync, no work needed"
    exit 0
fi

# --- Clean up stale .new files from any previous interrupted sync ---
# (EFI FAT32 is ~127M; kernel 31M + initramfs 55M leaves no room for .new copies)
rm -f "$EFI_MOUNT/$KERNEL_NAME.new" "$EFI_MOUNT/initramfs.img.new" "$EFI_MOUNT/cmdline.txt.new"

# --- Direct update (not atomic — FAT32 partition has no space for old+new simultaneously) ---
cp -f --preserve=mode "$SRC_KERNEL"    "$EFI_MOUNT/$KERNEL_NAME"
cp -f --preserve=mode "$SRC_INITRAMFS" "$EFI_MOUNT/initramfs.img"
printf '%s\n' "$CMDLINE" > "$EFI_MOUNT/cmdline.txt"
sync "$EFI_MOUNT"

echo "rpi-bls-sync: synced ${KERNEL_NAME} (${SRC_K_SIZE}B), initramfs.img (${SRC_I_SIZE}B)"
echo "rpi-bls-sync: cmdline.txt = $CMDLINE"
