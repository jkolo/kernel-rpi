//! Runs INSIDE a QEMU guest as the sole payload of a minimal busybox
//! initramfs (see tests/qemu/). Exercises the parts of this crate that
//! cannot be unit-tested without root + a real block device: PARTLABEL
//! resolution, real mount(2)/umount(2) via `mounts.rs`, and `StdBootFs` +
//! `sync_write_plan` against a REAL loop-partitioned ext4/vfat disk image
//! (not an arbitrary host directory, unlike tests/std_boot_fs.rs).
//!
//! Deliberately calls `orchestrate::plan_sync` directly with a hardcoded
//! `model_raw` rather than going through `lib.rs::run()` — `plan_sync`
//! takes `model_raw` as plain data, so this needs no `/proc/device-tree`
//! (an aarch64/RPi-only concept never present on the x86_64 QEMU guest this
//! harness runs on); every OTHER input is real, read from the real
//! loop-mounted partitions this harness itself sets up.
//!
//! Prints `PASS:`/`FAIL:` lines and a final `QEMU_HARNESS_RESULT: PASS|FAIL`
//! marker to stdout (captured via QEMU's serial console by the host driver).

use rpi_bls_sync::fatsync::{BootFs, PendingWrite, StdBootFs};
use rpi_bls_sync::{fatsync, mounts, orchestrate};
use std::path::Path;

struct Report {
    failures: Vec<String>,
    passes: u32,
}

impl Report {
    fn check(&mut self, cond: bool, msg: &str) {
        if cond {
            println!("PASS: {msg}");
            self.passes += 1;
        } else {
            println!("FAIL: {msg}");
            self.failures.push(msg.to_string());
        }
    }
}

fn report_and_exit(r: &Report) -> ! {
    println!("SUMMARY: {} passed, {} failed", r.passes, r.failures.len());
    for f in &r.failures {
        println!("QEMU_HARNESS_FAILURE: {f}");
    }
    println!(
        "QEMU_HARNESS_RESULT: {}",
        if r.failures.is_empty() { "PASS" } else { "FAIL" }
    );
    std::process::exit(if r.failures.is_empty() { 0 } else { 1 });
}

fn main() {
    let mut r = Report { failures: Vec::new(), passes: 0 };

    // --- Real PARTLABEL resolution (symlinks created by the init script
    //     after `losetup --partscan`, mirroring udev's by-partlabel tree). ---
    let boot_dev = mounts::partlabel_device("boot");
    r.check(boot_dev.is_some(), "partlabel_device(boot) resolves via /dev/disk/by-partlabel");
    let efi_dev = mounts::partlabel_device("EFI-SYSTEM");
    r.check(efi_dev.is_some(), "partlabel_device(EFI-SYSTEM) resolves via /dev/disk/by-partlabel");
    let (Some(boot_dev), Some(efi_dev)) = (boot_dev, efi_dev) else {
        println!("FATAL: cannot continue without both partitions resolved");
        report_and_exit(&r);
    };

    // --- Real mount(2) via rustix ---
    std::fs::create_dir_all("/mnt/boot").unwrap();
    std::fs::create_dir_all("/mnt/efi").unwrap();
    r.check(!mounts::is_mountpoint(Path::new("/mnt/boot")), "is_mountpoint(/mnt/boot) false before mount");
    r.check(
        mounts::mount_readonly(&boot_dev, Path::new("/mnt/boot"), "ext4").is_ok(),
        "mount_readonly(boot, ext4) succeeds against real loop partition",
    );
    r.check(mounts::is_mountpoint(Path::new("/mnt/boot")), "is_mountpoint(/mnt/boot) true after mount");
    r.check(
        mounts::mount_writable(&efi_dev, Path::new("/mnt/efi"), "vfat").is_ok(),
        "mount_writable(EFI-SYSTEM, vfat) succeeds against real loop partition",
    );

    // --- Real BLS entry + source kernel/initramfs read from the real ext4 mount ---
    let bls_path = "/mnt/boot/loader/entries/ostree-1.conf";
    let bls_contents = std::fs::read_to_string(bls_path).unwrap_or_default();
    r.check(!bls_contents.is_empty(), "BLS entry readable from real ext4 loop-mount");
    r.check(
        bls_contents.contains("ostree=/ostree/boot.0"),
        "BLS entry has the expected ostree= slot",
    );

    let kernel_src = "/mnt/boot/ostree/boot.0/rhcos/CSUM/0/vmlinuz";
    let initrd_src = "/mnt/boot/ostree/boot.0/rhcos/CSUM/0/initramfs.img";
    let kernel_bytes = std::fs::read(kernel_src).unwrap_or_default();
    let initrd_bytes = std::fs::read(initrd_src).unwrap_or_default();
    r.check(!kernel_bytes.is_empty(), "source kernel readable from real ext4 mount");
    r.check(!initrd_bytes.is_empty(), "source initramfs readable from real ext4 mount");

    let proc_cmdline = std::fs::read_to_string("/proc/cmdline").unwrap_or_default();

    // --- Scenario A: fresh sync onto a REAL (empty) vfat partition ---
    let entries = [(bls_path, bls_contents.as_str())];
    let mut fs = StdBootFs::new("/mnt/efi");
    let current_cmdline_txt1 = fs.read_to_string("cmdline.txt");
    let inputs = orchestrate::SyncInputs {
        model_raw: "Raspberry Pi 5 Model B Rev 1.0",
        bls_entries_sorted: &entries,
        proc_cmdline: &proc_cmdline,
        ignition_firstboot_present: false,
        cmdline_d_contents_ordered: &[],
        deploy_root_prefixes_present: [false, false],
        guard_target_dir_present_prefixes: [false, false],
        clock_usec: 1_700_000_000_000_000,
        current_cmdline_txt: current_cmdline_txt1.as_deref(),
        src_kernel_size: kernel_bytes.len() as u64,
        dst_kernel_size: fs.file_size("kernel_2712.img"),
        src_initramfs_size: initrd_bytes.len() as u64,
        dst_initramfs_size: fs.file_size("initramfs.img"),
    };

    match orchestrate::plan_sync(&inputs) {
        orchestrate::SyncPlan::Sync { cmdline_txt, kernel_name } => {
            r.check(kernel_name == "kernel_2712.img", "plan_sync selects kernel_2712.img for rpi5");
            r.check(
                cmdline_txt.contains("ostree=/ostree/boot.0"),
                "resolved cmdline.txt carries the expected ostree=",
            );

            let large = [
                PendingWrite::Large { dest: kernel_name, contents: &kernel_bytes },
                PendingWrite::Large { dest: "initramfs.img", contents: &initrd_bytes },
            ];
            let write_result =
                fatsync::sync_write_plan(&mut fs, &[], &large, &[], "cmdline.txt", cmdline_txt.as_bytes());
            r.check(write_result.is_ok(), "sync_write_plan succeeds against a REAL vfat mount");
        }
        other => r.check(false, &format!("expected SyncPlan::Sync, got {other:?}")),
    }

    // --- Verify on-disk results via independent reads (fsync already ran inside sync_write_plan) ---
    let written_kernel = std::fs::read("/mnt/efi/kernel_2712.img").unwrap_or_default();
    r.check(written_kernel == kernel_bytes, "kernel_2712.img on real vfat matches source bytes exactly");
    let written_initramfs = std::fs::read("/mnt/efi/initramfs.img").unwrap_or_default();
    r.check(written_initramfs == initrd_bytes, "initramfs.img on real vfat matches source bytes exactly");
    let written_cmdline = std::fs::read_to_string("/mnt/efi/cmdline.txt").unwrap_or_default();
    r.check(
        written_cmdline.contains("ostree=/ostree/boot.0"),
        "cmdline.txt on real vfat has the expected content",
    );
    r.check(
        !Path::new("/mnt/efi/cmdline.txt.new").exists(),
        "no leftover .new stragglers after a successful sync",
    );

    // --- Scenario B: idempotency — re-run against the now-synced real partition ---
    let current_cmdline_txt2 = std::fs::read_to_string("/mnt/efi/cmdline.txt").ok();
    let dst_kernel_size2 = std::fs::metadata("/mnt/efi/kernel_2712.img").ok().map(|m| m.len());
    let dst_initramfs_size2 = std::fs::metadata("/mnt/efi/initramfs.img").ok().map(|m| m.len());
    let inputs2 = orchestrate::SyncInputs {
        current_cmdline_txt: current_cmdline_txt2.as_deref(),
        dst_kernel_size: dst_kernel_size2,
        dst_initramfs_size: dst_initramfs_size2,
        // Different clock than the first run — must NOT force a resync
        // (needs_sync compares clock-STRIPPED cmdline; a real RTC-less RPi
        // gets a different systemd.clock_usec= every invocation).
        clock_usec: 1_700_000_000_999_999,
        ..inputs
    };
    match orchestrate::plan_sync(&inputs2) {
        orchestrate::SyncPlan::Skip(reason) => {
            r.check(reason == "already in sync", "second run is idempotent (Skip: already in sync)");
        }
        other => r.check(false, &format!("expected Skip(already in sync) on rerun, got {other:?}")),
    }

    // --- Real unmount ---
    r.check(mounts::unmount(Path::new("/mnt/efi")).is_ok(), "unmount(EFI) succeeds");
    r.check(mounts::unmount(Path::new("/mnt/boot")).is_ok(), "unmount(boot) succeeds");
    r.check(!mounts::is_mountpoint(Path::new("/mnt/boot")), "is_mountpoint(/mnt/boot) false after unmount");

    report_and_exit(&r);
}
