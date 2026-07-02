//! T3 (RED phase): failing tests for `cmdline` dedup/clock/firstboot.
//! Ground truth: rpi-bls-sync.sh lines 101-133, 213.

use rpi_bls_sync::cmdline::{dedup_tokens, expand_firstboot, inject_clock, strip_clock};
use std::path::Path;

// ---- Pure unit tests ----

#[test]
fn expand_firstboot_substitutes_marker() {
    assert_eq!(
        expand_firstboot("root=/dev/mapper/root $ignition_firstboot console=ttyS0", true),
        "root=/dev/mapper/root ignition.firstboot=1 console=ttyS0"
    );
}

#[test]
fn expand_firstboot_empty_when_not_firstboot() {
    // Substituting with "" then squeezing (`tr -s ' '`) collapses the
    // resulting double space, but does NOT trim leading/trailing spaces at
    // this stage (that only happens after the full cmdline is assembled).
    assert_eq!(
        expand_firstboot("root=/dev/mapper/root $ignition_firstboot console=ttyS0", false),
        "root=/dev/mapper/root console=ttyS0"
    );
}

#[test]
fn dedup_keeps_first_occurrence_drops_later_duplicates() {
    assert_eq!(
        dedup_tokens("console=ttyS0 cgroup_no_v1=all console=ttyS0 foo=bar cgroup_no_v1=all"),
        "console=ttyS0 cgroup_no_v1=all foo=bar"
    );
}

#[test]
fn dedup_preserves_distinct_console_values_and_order() {
    // Two DIFFERENT console= values must both survive, in order — this is
    // NOT dedup-by-key, only whole-token dedup.
    assert_eq!(
        dedup_tokens("console=tty1 console=ttyS0"),
        "console=tty1 console=ttyS0"
    );
}

#[test]
fn dedup_drops_empty_tokens_from_double_spaces() {
    assert_eq!(dedup_tokens("a  b"), "a b");
}

#[test]
fn inject_clock_replaces_existing_and_appends_last() {
    assert_eq!(
        inject_clock(
            "root=/dev/mapper/root systemd.clock_usec=999 console=ttyS0",
            123_456_789_012
        ),
        "root=/dev/mapper/root console=ttyS0 systemd.clock_usec=123456789012"
    );
}

#[test]
fn inject_clock_appends_when_absent() {
    assert_eq!(
        inject_clock("root=/dev/mapper/root console=ttyS0", 42),
        "root=/dev/mapper/root console=ttyS0 systemd.clock_usec=42"
    );
}

#[test]
fn strip_clock_removes_token_and_squeezes() {
    assert_eq!(
        strip_clock("root=/dev/mapper/root systemd.clock_usec=999 console=ttyS0"),
        "root=/dev/mapper/root console=ttyS0"
    );
}

#[test]
fn strip_clock_is_noop_when_absent() {
    assert_eq!(
        strip_clock("root=/dev/mapper/root console=ttyS0"),
        "root=/dev/mapper/root console=ttyS0"
    );
}

// ---- Oracle-backed characterization tests ----

fn run_oracle(subcmd: &str, args: &[&str]) -> String {
    let oracle_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle/cmdline_ops.sh");
    let output = std::process::Command::new("bash")
        .arg(&oracle_path)
        .arg(subcmd)
        .args(args)
        .output()
        .expect("failed to run bash oracle");
    assert!(output.status.success(), "oracle script failed: {output:?}");
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn oracle_expand_firstboot_true() {
    let input = "root=/dev/mapper/root $ignition_firstboot console=ttyS0";
    let oracle = run_oracle("expand_firstboot", &[input, "ignition.firstboot=1"]);
    assert_eq!(expand_firstboot(input, true), oracle);
}

#[test]
fn oracle_expand_firstboot_false() {
    let input = "root=/dev/mapper/root $ignition_firstboot console=ttyS0";
    let oracle = run_oracle("expand_firstboot", &[input, ""]);
    assert_eq!(expand_firstboot(input, false), oracle);
}

#[test]
fn oracle_dedup() {
    let input = "console=ttyS0 cgroup_no_v1=all console=ttyS0 foo=bar cgroup_no_v1=all";
    let oracle = run_oracle("dedup", &[input]);
    assert_eq!(dedup_tokens(input), oracle);
}

#[test]
fn oracle_inject_clock() {
    let input = "root=/dev/mapper/root systemd.clock_usec=999 console=ttyS0";
    let oracle = run_oracle("inject_clock", &[input, "123456789012"]);
    assert_eq!(inject_clock(input, 123_456_789_012), oracle);
}

#[test]
fn oracle_strip_clock() {
    let input = "root=/dev/mapper/root systemd.clock_usec=999 console=ttyS0";
    let oracle = run_oracle("strip_clock", &[input]);
    assert_eq!(strip_clock(input), oracle);
}
