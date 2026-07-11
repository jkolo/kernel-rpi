//! T5 (RED phase): end-to-end integration tests for `orchestrate::plan_sync`
//! — the full pipeline (model → BLS selection → cmdline assembly →
//! liveness-first slot resolution → idempotency), covering both
//! InitrdPreLuks and RealRoot contexts.

use rpi_bls_sync::orchestrate::{plan_sync, SyncInputs, SyncPlan};

fn happy_path_inputs() -> SyncInputs<'static> {
    SyncInputs {
        model_raw: "Raspberry Pi 5 Model B Rev 1.0",
        bls_entries_sorted: &[("loader/entries/ostree-1.conf", "title Fedora CoreOS\nversion 1\noptions ostree=/ostree/boot.0/rhcos/CSUM/0 root=/dev/mapper/root rw\nlinux /ostree/boot.0/rhcos/CSUM/0/vmlinuz\ninitrd /ostree/boot.0/rhcos/CSUM/0/initramfs.img")],
        proc_cmdline: "ostree=/ostree/boot.0/rhcos/CSUM/0",
        ignition_firstboot_present: false,
        cmdline_d_contents_ordered: &[],
        deploy_root_prefixes_present: [true, false],
        live_slot_dir_present_prefixes: [true, false],
        bls_slot_dir_present_prefixes: [true, false],
        clock_usec: 123_456,
        current_cmdline_txt: None,
        src_kernel_size: 31_000_000,
        dst_kernel_size: None,
        src_initramfs_size: 55_000_000,
        dst_initramfs_size: None,
    }
}

#[test]
fn happy_path_produces_sync_plan() {
    let inputs = happy_path_inputs();
    match plan_sync(&inputs) {
        SyncPlan::Sync { cmdline_txt, kernel_name } => {
            assert_eq!(kernel_name, "kernel_2712.img");
            assert!(cmdline_txt.contains("ostree=/ostree/boot.0/rhcos/CSUM/0"));
            assert!(cmdline_txt.contains("root=/dev/mapper/root"));
            assert!(cmdline_txt.contains("systemd.clock_usec=123456"));
        }
        other => panic!("expected Sync, got {other:?}"),
    }
}

#[test]
fn unsupported_model_skips() {
    let mut inputs = happy_path_inputs();
    inputs.model_raw = "Some Other Board";
    assert_eq!(plan_sync(&inputs), SyncPlan::Skip("unsupported model"));
}

#[test]
fn no_bls_entries_skips() {
    let mut inputs = happy_path_inputs();
    inputs.bls_entries_sorted = &[];
    assert_eq!(plan_sync(&inputs), SyncPlan::Skip("no BLS entry found"));
}

#[test]
fn already_in_sync_skips() {
    let mut inputs = happy_path_inputs();
    // Pre-compute what the plan WOULD produce, then feed it back as "already
    // synced" (dest sizes match src, current cmdline.txt clock-strips equal).
    let SyncPlan::Sync { cmdline_txt, .. } = plan_sync(&inputs) else {
        panic!("fixture must produce Sync first");
    };
    inputs.dst_kernel_size = Some(inputs.src_kernel_size);
    inputs.dst_initramfs_size = Some(inputs.src_initramfs_size);
    inputs.current_cmdline_txt = Some(Box::leak(cmdline_txt.into_boxed_str()));
    assert_eq!(plan_sync(&inputs), SyncPlan::Skip("already in sync"));
}

#[test]
fn real_root_refuses_when_bls_slot_absent() {
    let mut inputs = happy_path_inputs();
    // proc == bls (no adoption); the BLS slot dir is absent -> refuse.
    inputs.live_slot_dir_present_prefixes = [false, false];
    inputs.bls_slot_dir_present_prefixes = [false, false];
    assert_eq!(plan_sync(&inputs), SyncPlan::RefuseExit0);
}

#[test]
fn initrd_context_syncs_even_when_guard_target_would_be_absent() {
    // Pre-LUKS initrd: deploy root not accessible at all -> guard is
    // skipped entirely, regardless of guard_target_dir_present_prefixes.
    let mut inputs = happy_path_inputs();
    inputs.deploy_root_prefixes_present = [false, false];
    inputs.live_slot_dir_present_prefixes = [false, false];
    inputs.bls_slot_dir_present_prefixes = [false, false];
    match plan_sync(&inputs) {
        SyncPlan::Sync { .. } => {}
        other => panic!("expected Sync (firstboot-safety trust), got {other:?}"),
    }
}

#[test]
fn adopts_live_slot_end_to_end() {
    let mut inputs = happy_path_inputs();
    // BLS still says boot.0, but /proc says the live deployment is at
    // boot.1 of the SAME stateroot/csum/serial -> must adopt boot.1 in the
    // final written cmdline.txt.
    inputs.proc_cmdline = "ostree=/ostree/boot.1/rhcos/CSUM/0";
    match plan_sync(&inputs) {
        SyncPlan::Sync { cmdline_txt, .. } => {
            assert!(cmdline_txt.contains("ostree=/ostree/boot.1/rhcos/CSUM/0"));
            assert!(!cmdline_txt.contains("boot.0"));
        }
        other => panic!("expected Sync, got {other:?}"),
    }
}

#[test]
fn falls_back_to_bls_when_live_pruned_end_to_end() {
    // shutdown-after-finalize (THE FIX): /proc names boot.1 (same deployment)
    // but that slot has been pruned; the finalized BLS slot boot.0 is present
    // -> the written cmdline.txt must keep boot.0, NOT refuse.
    let mut inputs = happy_path_inputs();
    inputs.proc_cmdline = "ostree=/ostree/boot.1/rhcos/CSUM/0";
    inputs.live_slot_dir_present_prefixes = [false, false];
    inputs.bls_slot_dir_present_prefixes = [true, false];
    match plan_sync(&inputs) {
        SyncPlan::Sync { cmdline_txt, .. } => {
            assert!(cmdline_txt.contains("ostree=/ostree/boot.0/rhcos/CSUM/0"));
            assert!(!cmdline_txt.contains("boot.1"));
        }
        other => panic!("expected Sync with BLS boot.0, got {other:?}"),
    }
}

#[test]
fn cmdline_d_extras_are_appended_and_deduped() {
    let mut inputs = happy_path_inputs();
    inputs.cmdline_d_contents_ordered = &["cgroup_no_v1=all root=/dev/mapper/root"];
    match plan_sync(&inputs) {
        SyncPlan::Sync { cmdline_txt, .. } => {
            assert!(cmdline_txt.contains("cgroup_no_v1=all"));
            // root=/dev/mapper/root appears in both BLS options and the
            // cmdline.d extra — dedup must keep only one occurrence.
            assert_eq!(cmdline_txt.matches("root=/dev/mapper/root").count(), 1);
        }
        other => panic!("expected Sync, got {other:?}"),
    }
}
