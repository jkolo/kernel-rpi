//! Unit tests for `bls::select_default` (BLS default-entry selection).

use rpi_bls_sync::bls::{parse_entries, select_default};

#[test]
fn picks_max_version() {
    let e = parse_entries(&[
        "version 1\nlinux /a\ninitrd /a.img\noptions foo",
        "version 3\nlinux /c\ninitrd /c.img\noptions foo",
        "version 2\nlinux /b\ninitrd /b.img\noptions foo",
    ]);
    assert_eq!(select_default(&e).unwrap().linux, "/c");
}

#[test]
fn tie_keeps_first_in_glob_order() {
    let e = parse_entries(&[
        "version 5\nlinux /first\ninitrd /first.img\noptions foo",
        "version 5\nlinux /second\ninitrd /second.img\noptions foo",
    ]);
    // strict '>' compare (not '>='): the second same-version entry must NOT
    // displace the first.
    assert_eq!(select_default(&e).unwrap().linux, "/first");
}

#[test]
fn non_integer_version_skipped() {
    let e = parse_entries(&[
        "version foo\nlinux /bad\ninitrd /bad.img\noptions foo",
        "version 0\nlinux /good\ninitrd /good.img\noptions foo",
    ]);
    // BEST_VER inits at -1; a present-but-non-integer version is silently
    // skipped, so version 0 wins.
    assert_eq!(select_default(&e).unwrap().linux, "/good");
}

#[test]
fn missing_version_field_defaults_to_zero_and_competes() {
    let e = parse_entries(&["linux /noversion\ninitrd /noversion.img\noptions foo"]);
    assert_eq!(select_default(&e).unwrap().linux, "/noversion");
}

#[test]
fn version_field_first_line_wins() {
    // The version is taken from the FIRST matching line, so a duplicate
    // `version` line later in the file is ignored.
    let e = parse_entries(&["version 1\nversion 9\nlinux /a\ninitrd /a.img\noptions foo"]);
    // Only one entry, so it always wins regardless — the assertion that
    // matters is on the extracted version, exercised via a tie against a
    // second entry with version 2 (which would win if the SECOND version
    // line were used instead of the first).
    let e2 = parse_entries(&[
        "version 1\nversion 9\nlinux /a\ninitrd /a.img\noptions foo",
        "version 2\nlinux /b\ninitrd /b.img\noptions foo",
    ]);
    assert_eq!(e[0].version_text.as_deref(), Some("1"));
    assert_eq!(select_default(&e2).unwrap().linux, "/b");
}

#[test]
fn empty_entry_list_returns_none() {
    let e = parse_entries(&[]);
    assert_eq!(select_default(&e), None);
}

#[test]
fn options_field_captured() {
    let e = parse_entries(&[
        "version 1\nlinux /a\ninitrd /a.img\noptions ostree=/ostree/boot.0/x/y/0 console=ttyS0",
    ]);
    assert_eq!(
        e[0].options,
        "ostree=/ostree/boot.0/x/y/0 console=ttyS0"
    );
}
