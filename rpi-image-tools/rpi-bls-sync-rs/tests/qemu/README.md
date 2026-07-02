# QEMU loop-device integration test

`cargo test` cannot exercise real `mount(2)`/`umount(2)` or a real loop
device — that needs root plus a real (or loop) block device, neither of
which a sandboxed dev environment has. This directory closes that gap by
running the untested I/O layer for real, inside a disposable QEMU guest
(root only inside the guest — never on the host).

## Run it

```
tests/qemu/run.sh
```

Needs on the host (all unprivileged): `qemu-system-x86_64`, `busybox`
(static), a `linux-lts` kernel under `/usr/lib/modules/*-lts/` with
loadable `loop`/`fat`/`vfat` modules, `sgdisk`, `mke2fs`, `mkfs.vfat`,
`zstd`, `cpio`. No host root, no host loop devices, no host mounts.

## What it does

1. Builds `rpi-bls-sync-rs` in release mode plus the
   `examples/qemu_harness.rs` binary (same crate, links `rpi_bls_sync`
   directly).
2. Assembles a GPT disk image (`boot`=ext4 with a real BLS entry + fake
   kernel/initramfs, `EFI-SYSTEM`=vfat, empty) — entirely unprivileged on
   the host via `sgdisk` + `mke2fs -d` + `dd` (no host mount/loop needed to
   populate the partitions).
3. Builds a minimal busybox initramfs containing that disk image as a
   plain file, the compiled harness binary + its 3 shared libs, and the
   `loop`/`fat`/`vfat` kernel modules (not built into the host kernel on
   this distro — must be `insmod`'d explicitly).
4. Boots the guest. `init` (this directory) loads the modules,
   `losetup -P`-attaches `/combined.img` (a genuine loop device — the same
   technique this repo's own `build-node-disks.sh`/`build-base-raw.sh` use
   to manipulate RHCOS disk images), manually creates the
   `/dev/disk/by-partlabel/{boot,EFI-SYSTEM}` symlinks (no real udev here),
   and runs `qemu_harness`.
5. `qemu_harness` calls `orchestrate::plan_sync` directly (hardcoded
   `model_raw` — `plan_sync` takes it as plain data, so no
   aarch64/device-tree emulation is needed) with REAL data read from the
   loop-mounted ext4 partition, then executes the result via REAL
   `mounts::mount_readonly`/`mount_writable` and `fatsync::sync_write_plan`
   + `StdBootFs` against the REAL vfat partition. Prints `PASS:`/`FAIL:`
   lines and a final `QEMU_HARNESS_RESULT: PASS|FAIL` to the serial
   console.
6. `run.sh` captures the serial console to a log, greps for the result
   marker, and exits 0/1 accordingly.

## What it proved (21/21 checks, at the time this was written)

Real `partlabel_device` resolution, real `mount_readonly`/`mount_writable`
via `rustix::mount::mount`, real `is_mountpoint` via `/proc/mounts`, a real
BLS entry read from a real ext4 loop-mount, `plan_sync` end to end, a real
byte-exact kernel/initramfs/cmdline.txt write to a real FAT32 filesystem,
idempotency on a second run, and real `unmount`. Getting here required
fixing two real bugs the *test infrastructure* had to discover (both
kernel modules missing on the host — `loop.ko` and `fat.ko`/`vfat.ko` are
not built into this distro's kernel) — proof the harness genuinely
exercises these paths rather than trivially passing regardless of content.

## What it does NOT cover

- aarch64/RPi hardware specifics (real GPIO/EEPROM/hardware boot) — that's
  the canary w1 boot-proof gate (plan Section E), not this.
- `lib.rs::run()`'s own glue (reading `/proc/device-tree/model`,
  `read_sorted_conf_files`, `resolve_mount`'s PARTLABEL-not-found/
  temp-mount-creation-failure error paths) — this harness calls
  `orchestrate::plan_sync` + the mount/fatsync primitives directly, not
  `run()` itself, specifically to avoid needing a fake device tree.
