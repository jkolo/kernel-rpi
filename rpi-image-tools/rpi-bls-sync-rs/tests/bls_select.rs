//! T1 (RED phase): failing tests for `bls::select_default`.
//! Ground truth: rpi-bls-sync.sh lines 66-72. See tests/fixtures/README.md.

use rpi_bls_sync::bls::{parse_entries, select_default};
use std::path::Path;

// ---- Pure unit tests (approved plan sketches, Option-adjusted) ----

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
    // skipped (bash: `[ "$_v" -gt "$BEST_VER" ] 2>/dev/null` swallows the
    // "integer expression expected" error as false), so version 0 wins.
    assert_eq!(select_default(&e).unwrap().linux, "/good");
}

#[test]
fn missing_version_field_defaults_to_zero_and_competes() {
    let e = parse_entries(&["linux /noversion\ninitrd /noversion.img\noptions foo"]);
    assert_eq!(select_default(&e).unwrap().linux, "/noversion");
}

#[test]
fn version_field_first_line_wins() {
    // awk '/^version[[:space:]]/ { print $2; exit }' — exits after the FIRST
    // matching line, so a duplicate `version` line later in the file is ignored.
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

// ---- Oracle-backed characterization tests (real bash extract, tests/oracle/bls_select.sh) ----

fn fixtures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bls")
}

/// Read `loader/entries/ostree-*.conf` from `dir`, sorted bytewise (LC_ALL=C
/// equivalent), mirroring both the bash oracle's glob order and the order
/// the real `mounts`/orchestration layer must feed into `select_default`.
fn read_entries_sorted(dir: &Path) -> Vec<(String, String)> {
    let entries_dir = dir.join("loader/entries");
    let mut paths: Vec<_> = std::fs::read_dir(&entries_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("ostree-") && n.ends_with(".conf"))
        })
        .collect();
    paths.sort_by(|a, b| a.as_os_str().cmp(b.as_os_str()));
    paths
        .into_iter()
        .map(|p| {
            let contents = std::fs::read_to_string(&p).unwrap();
            (p.to_string_lossy().into_owned(), contents)
        })
        .collect()
}

fn assert_matches_oracle(fixture_name: &str) {
    let dir = fixtures_dir().join(fixture_name);
    let pairs = read_entries_sorted(&dir);
    let blocks: Vec<&str> = pairs.iter().map(|(_, c)| c.as_str()).collect();
    let paths: Vec<&str> = pairs.iter().map(|(p, _)| p.as_str()).collect();

    let parsed = parse_entries(&blocks);
    let winner = select_default(&parsed);
    let winner_idx = winner.and_then(|w| parsed.iter().position(|e| std::ptr::eq(e, w)));

    // Run the real bash oracle for ground truth.
    let oracle_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle/bls_select.sh");
    let output = std::process::Command::new("bash")
        .arg(&oracle_path)
        .arg(&dir)
        .output()
        .expect("failed to run bash oracle");
    assert!(output.status.success(), "oracle script failed: {output:?}");
    let oracle_winner = String::from_utf8_lossy(&output.stdout).trim().to_string();

    let rust_winner_path = winner_idx.map(|i| paths[i]).unwrap_or("");
    assert_eq!(
        rust_winner_path, oracle_winner,
        "fixture {fixture_name}: Rust selected {rust_winner_path:?}, bash oracle selected {oracle_winner:?}"
    );
}

#[test]
fn oracle_multi_version() {
    assert_matches_oracle("multi_version");
}

#[test]
fn oracle_tie() {
    assert_matches_oracle("tie");
}

#[test]
fn oracle_non_integer() {
    assert_matches_oracle("non_integer");
}

#[test]
fn oracle_missing_field() {
    assert_matches_oracle("missing_field");
}

#[test]
fn oracle_single() {
    assert_matches_oracle("single");
}
