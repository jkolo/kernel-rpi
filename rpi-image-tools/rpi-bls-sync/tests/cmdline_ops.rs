//! Unit tests for `cmdline` dedup/clock/firstboot helpers.

use rpi_bls_sync::cmdline::{dedup_tokens, expand_firstboot, inject_clock, strip_clock};

#[test]
fn expand_firstboot_substitutes_marker() {
    assert_eq!(
        expand_firstboot("root=/dev/mapper/root $ignition_firstboot console=ttyS0", true),
        "root=/dev/mapper/root ignition.firstboot=1 console=ttyS0"
    );
}

#[test]
fn expand_firstboot_empty_when_not_firstboot() {
    // Substituting with "" then squeezing collapses the resulting double
    // space, but does NOT trim leading/trailing spaces at this stage (that
    // only happens after the full cmdline is assembled).
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
