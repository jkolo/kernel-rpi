//! BLS (Boot Loader Specification) entry parsing + default-entry selection.
//! Ground truth: rpi-bls-sync.sh lines 66-80.
//!
//! Selection is MAX `version` field (BLS spec / GRUB semantics), NOT
//! `sort -V` and NOT mtime. Tie → first entry in (lexical, LC_ALL=C glob)
//! order. A missing `version` line defaults to 0 (still competes); a
//! present-but-non-integer `version` value causes the entry to be SKIPPED
//! entirely (bash: `[ "$_v" -gt "$BEST_VER" ] 2>/dev/null` swallows the
//! "integer expression expected" error as false).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlsEntry {
    pub linux: String,
    pub initrd: String,
    pub options: String,
    /// Raw text of the `version` field's second column, if the line was
    /// present at all. `None` means no `version` line matched (→ compares as 0).
    pub version_text: Option<String>,
}

/// Extract the value of the first line matching `/^{field}[[:space:]]/`,
/// with everything up to and including the first run of whitespace after
/// the field name stripped (awk `$2` = second whitespace-separated column;
/// `sub(/^options +/,"")` for `options` keeps the REST of the line, not just
/// column 2). `exit` semantics: first match wins, later duplicate lines ignored.
fn first_field_line<'a>(contents: &'a str, field: &str) -> Option<&'a str> {
    contents.lines().find_map(|line| {
        let rest = line.strip_prefix(field)?;
        let after_prefix = rest.strip_prefix(char::is_whitespace)?;
        Some(after_prefix.trim_start())
    })
}

/// awk `{ print $2; exit }` semantics: second whitespace-separated column only.
fn first_field_second_column<'a>(contents: &'a str, field: &str) -> Option<&'a str> {
    first_field_line(contents, field).and_then(|rest| rest.split_whitespace().next())
}

pub fn parse_entry(contents: &str) -> BlsEntry {
    BlsEntry {
        linux: first_field_second_column(contents, "linux")
            .unwrap_or("")
            .to_string(),
        initrd: first_field_second_column(contents, "initrd")
            .unwrap_or("")
            .to_string(),
        // `options` keeps the rest of the line (not just column 2): mirrors
        // `sub(/^options +/,""); print` rather than `print $2`.
        options: first_field_line(contents, "options")
            .unwrap_or("")
            .to_string(),
        version_text: first_field_second_column(contents, "version").map(str::to_string),
    }
}

pub fn parse_entries(blocks: &[&str]) -> Vec<BlsEntry> {
    blocks.iter().map(|b| parse_entry(b)).collect()
}

/// Comparison value for one entry's version, mirroring bash: a missing
/// `version` line defaults to 0 (still competes against BEST_VER=-1); a
/// present-but-non-integer value returns `None` (entry is skipped, never
/// updates BEST_VER — `[ "$_v" -gt "$BEST_VER" ] 2>/dev/null` swallows the
/// "integer expression expected" error as false).
fn version_for_compare(entry: &BlsEntry) -> Option<i64> {
    match &entry.version_text {
        None => Some(0),
        Some(v) => v.parse::<i64>().ok(),
    }
}

pub fn select_default(entries: &[BlsEntry]) -> Option<&BlsEntry> {
    const BEST_VER_INIT: i64 = -1;
    let mut best: Option<(&BlsEntry, i64)> = None;
    for entry in entries {
        let Some(v) = version_for_compare(entry) else {
            continue;
        };
        let best_v = best.map_or(BEST_VER_INIT, |(_, bv)| bv);
        if v > best_v {
            best = Some((entry, v));
        }
    }
    best.map(|(e, _)| e)
}
