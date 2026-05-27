#!/usr/bin/env bash
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

# --- Find lowest-numbered BLS entry (= default next boot) ---
BLS=$(ls "$BOOT_MOUNT/loader/entries/ostree-"*.conf 2>/dev/null | sort -V | head -1 || true)
if [ ! -f "$BLS" ]; then
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

# --- Inject fresh wall-clock (RPi has no RTC) ---
# Remove any existing systemd.clock_usec= first, then append a fresh value.
CMDLINE=$(printf '%s' "$CMDLINE" | sed 's/systemd\.clock_usec=[^ ]*//g')
CMDLINE="$CMDLINE systemd.clock_usec=$(date +%s%6N)"

CMDLINE=$(printf '%s' "$CMDLINE" | tr -s ' ' | sed 's/^ //;s/ $//')

# --- Verify ostree= path and correct boot slot if mismatched ---
# bootc-image-writer may write BLS with ostree=/ostree/boot.N/... but deploy
# the filesystem into boot.M (M≠N). In initrd Boot A the root is LUKS-encrypted
# and inaccessible, so when the path is not found anywhere fall back to
# /proc/cmdline (which contains the ostree= that successfully booted this kernel).
_OARG=$(printf '%s' "$CMDLINE" | grep -o 'ostree=[^ ]*' || true)
if [ -n "$_OARG" ]; then
    _OPATH="${_OARG#ostree=}"          # /ostree/boot.N/stateroot/hash/serial
    _ODIR=$(dirname "$_OPATH")         # /ostree/boot.N/stateroot/hash
    _FOUND=0
    for _PFX in "" "/sysroot"; do
        [ -d "${_PFX}${_ODIR}" ] && { _FOUND=1; break; }
    done
    if [ "$_FOUND" = "0" ]; then
        _HASH=$(basename "$_ODIR")
        _SR=$(basename "$(dirname "$_ODIR")")
        _SER=$(basename "$_OPATH")
        _FIXED=""
        for _SLOT in 0 1; do
            _CAND="/ostree/boot.${_SLOT}/${_SR}/${_HASH}"
            for _PFX in "" "/sysroot"; do
                if [ -d "${_PFX}${_CAND}" ]; then
                    _FIXED="ostree=${_CAND}/${_SER}"
                    break 2
                fi
            done
        done
        if [ -z "$_FIXED" ]; then
            # Root inaccessible (LUKS not opened) — preserve current boot's path
            _PROC=$(grep -o 'ostree=[^ ]*' /proc/cmdline 2>/dev/null || true)
            if [ -n "$_PROC" ] && [ "$_PROC" != "$_OARG" ]; then
                _FIXED="$_PROC"
                echo "rpi-bls-sync: ostree dir not on filesystem, using /proc/cmdline: ${_FIXED}" >&2
            fi
        else
            echo "rpi-bls-sync: corrected ostree boot slot: ${_OARG} → ${_FIXED}" >&2
        fi
        [ -n "$_FIXED" ] && CMDLINE=$(printf '%s' "$CMDLINE" | sed "s|${_OARG}|${_FIXED}|g")
    fi
fi
unset _OARG _OPATH _ODIR _FOUND _PFX _HASH _SR _SER _SLOT _CAND _FIXED _PROC

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
CURRENT_CMDLINE=$(tr -d '\n' <"$EFI_MOUNT/cmdline.txt" 2>/dev/null || echo "")
[ "$CURRENT_CMDLINE" = "$CMDLINE" ] || NEED_SYNC=1

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
