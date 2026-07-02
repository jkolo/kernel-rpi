#!/usr/bin/env bash
# Real loop-device / mount(2) integration test for rpi-bls-sync-rs, run
# inside a disposable QEMU guest (as root, in the guest — never on the
# host). Closes the gap flagged in lib.rs/mounts.rs: "mount(2)/umount(2)
# ... have NO automated test coverage — this sandboxed dev environment has
# no root and no loop/real block devices."
#
# What this exercises for real (not faked/mocked):
#   - mounts::partlabel_device()  — /dev/disk/by-partlabel/<label> resolve
#   - mounts::mount_readonly/mount_writable/unmount() — real mount(2)/umount(2)
#   - mounts::is_mountpoint()     — real /proc/mounts parsing
#   - fatsync::StdBootFs + sync_write_plan() — real writes to a REAL FAT32
#     filesystem (not an arbitrary host directory, unlike tests/std_boot_fs.rs)
#   - orchestrate::plan_sync()    — the full pipeline, fed real data read
#     from a real ext4-mounted BLS entry
#
# Deliberately calls plan_sync directly with a hardcoded model_raw rather
# than going through lib.rs::run() — plan_sync takes model_raw as plain
# data, so no aarch64/device-tree emulation is needed; x86_64 QEMU + the
# host's own kernel suffices. See examples/qemu_harness.rs for the guest
# payload and initramfs-root/init for the guest-side setup.
#
# Usage: tests/qemu/run.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$SCRIPT_DIR/.build"
INITRAMFS_ROOT="$BUILD_DIR/initramfs-root"
SERIAL_LOG="$BUILD_DIR/serial.log"

KERNEL="$(ls /usr/lib/modules/*-lts/vmlinuz 2>/dev/null | head -1)"
MODDIR="$(dirname "$KERNEL")"
if [ -z "$KERNEL" ]; then
    echo "ERROR: no linux-lts kernel found under /usr/lib/modules/*-lts/" >&2
    exit 1
fi

rm -rf "$BUILD_DIR"
mkdir -p "$INITRAMFS_ROOT"/{bin,lib64,proc,sys,dev,mnt,tmp}

echo "==> [1/6] Building rpi-bls-sync-rs release binary + qemu_harness example"
(cd "$CRATE_DIR" && cargo build --release --example qemu_harness >/dev/null)

echo "==> [2/6] Assembling GPT test disk image (boot=ext4, EFI-SYSTEM=vfat) — unprivileged, no host mount/loop"
DISK_BUILD="$BUILD_DIR/disk-build"
mkdir -p "$DISK_BUILD/boot-content/loader/entries" "$DISK_BUILD/boot-content/ostree/boot.0/rhcos/CSUM/0"
cd "$DISK_BUILD"

cat > boot-content/loader/entries/ostree-1.conf <<'EOF'
title Fedora CoreOS (QEMU loop-device test)
version 1
options ostree=/ostree/boot.0/rhcos/CSUM/0 root=/dev/mapper/root rw systemd.unified_cgroup_hierarchy=1
linux /ostree/boot.0/rhcos/CSUM/0/vmlinuz
initrd /ostree/boot.0/rhcos/CSUM/0/initramfs.img
EOF
python3 -c "
with open('boot-content/ostree/boot.0/rhcos/CSUM/0/vmlinuz', 'wb') as f:
    f.write(b'FAKE-KERNEL-CONTENT-FOR-QEMU-LOOP-DEVICE-TEST-' * 200)
with open('boot-content/ostree/boot.0/rhcos/CSUM/0/initramfs.img', 'wb') as f:
    f.write(b'FAKE-INITRAMFS-CONTENT-FOR-QEMU-LOOP-DEVICE-TEST-' * 300)
"

mke2fs -F -q -t ext4 -L boot -d boot-content boot.img 16M
mkfs.vfat -F 32 -n EFISYS -C efi.img 16384 >/dev/null 2>&1

truncate -s 40M combined.img
sgdisk -og combined.img >/dev/null
sgdisk --new=1:2048:+16M --typecode=1:8300 --change-name=1:boot combined.img >/dev/null
sgdisk --new=2:0:+16M     --typecode=2:ef00 --change-name=2:EFI-SYSTEM combined.img >/dev/null
dd if=boot.img of=combined.img bs=512 seek=2048  conv=notrunc status=none
dd if=efi.img   of=combined.img bs=512 seek=34816 conv=notrunc status=none
sgdisk -p combined.img

echo "==> [3/6] Assembling busybox+loop/fat/vfat-module initramfs"
cd "$INITRAMFS_ROOT"
cp /usr/bin/busybox bin/busybox
cp "$CRATE_DIR/target/release/examples/qemu_harness" bin/qemu_harness
cp /usr/lib/libgcc_s.so.1 lib64/libgcc_s.so.1
cp /usr/lib/libc.so.6 lib64/libc.so.6
cp /usr/lib/ld-linux-x86-64.so.2 lib64/ld-linux-x86-64.so.2
cp "$DISK_BUILD/combined.img" combined.img
zstd -d -c "$MODDIR/kernel/drivers/block/loop.ko.zst" > loop.ko
zstd -d -c "$MODDIR/kernel/fs/fat/fat.ko.zst"          > fat.ko
zstd -d -c "$MODDIR/kernel/fs/fat/vfat.ko.zst"          > vfat.ko
cp "$SCRIPT_DIR/init" init
chmod +x init

echo "==> [4/6] Building cpio.gz initramfs archive"
find . | cpio -o -H newc 2>/dev/null | gzip -1 > "$BUILD_DIR/initramfs.cpio.gz"

echo "==> [5/6] Booting QEMU guest (real root, real loop-mounted GPT partitions)"
cd "$BUILD_DIR"
qemu-system-x86_64 \
    -kernel "$KERNEL" \
    -initrd initramfs.cpio.gz \
    -append "console=ttyS0 panic=-1 quiet" \
    -m 512M \
    -display none \
    -monitor none \
    -serial "file:$SERIAL_LOG" \
    -no-reboot \
    -cpu max

echo "==> [6/6] Verifying result"
if [ ! -f "$SERIAL_LOG" ]; then
    echo "ERROR: no serial log produced" >&2
    exit 1
fi

echo "--- guest serial output ---"
tr -d '\r' < "$SERIAL_LOG"
echo "---------------------------"

# The serial console line-ends with \r\n; strip \r before matching so the
# end anchor is meaningful (a bare `grep PASS` would also match a printed
# QEMU_HARNESS_FAILURE line quoting this string, so keep the anchors).
if tr -d '\r' < "$SERIAL_LOG" | grep -q "^QEMU_HARNESS_RESULT: PASS$"; then
    echo "QEMU loop-device integration test: PASS"
    exit 0
else
    echo "QEMU loop-device integration test: FAIL (see log above)"
    exit 1
fi
