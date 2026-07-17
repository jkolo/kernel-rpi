//! cmdline.txt assembly: firstboot expansion, /etc/cmdline.d/*.conf append,
//! order-preserving dedup, and fresh wall-clock injection.
//! Ground truth: rpi-bls-sync.sh lines 101-133.

/// Squeeze runs of spaces into a single space. Mirrors `tr -s ' '` (only the
/// ASCII space character — bash `tr` here, not general whitespace).
fn squeeze_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        if c == ' ' {
            if !prev_space {
                out.push(c);
            }
            prev_space = true;
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out
}

/// Expand `$ignition_firstboot` in BLS options and squeeze whitespace.
/// Mirrors: `sed "s/\$ignition_firstboot/${FIRSTBOOT}/g" | tr -s ' '`.
pub fn expand_firstboot(bls_options: &str, firstboot: bool) -> String {
    let replacement = if firstboot { "ignition.firstboot=1" } else { "" };
    squeeze_spaces(&bls_options.replace("$ignition_firstboot", replacement))
}

/// Order-preserving, keep-first-occurrence whole-token dedup.
/// Mirrors: `tr ' ' '\n' | awk 'NF && !seen[$0]++' | tr '\n' ' ' | sed 's/ $//'`.
/// NOT sort -u, NOT dedup-by-key — distinct `console=` values and their
/// order are preserved (last `console=` remains the primary device).
///
/// Splits on ALL whitespace, not just spaces: /etc/cmdline.d/*.conf contents
/// carry trailing newlines into the assembled string, and a space-only split
/// leaves them embedded in tokens → multi-line cmdline.txt, of which RPi
/// firmware reads only the FIRST line (console= and clock were dropped).
/// The bash ground truth flattened newlines too (awk record processing).
pub fn dedup_tokens(cmdline: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for tok in cmdline.split_whitespace() {
        if seen.insert(tok) {
            out.push(tok);
        }
    }
    out.join(" ")
}

/// Strip `systemd.clock_usec=` for idempotency comparison (does NOT append).
/// Mirrors the `_strip_clock()` helper (line 213):
/// `sed 's/systemd\.clock_usec=[^ ]*//g' | tr -s ' ' | sed 's/^ //;s/ $//'`.
pub fn strip_clock(cmdline: &str) -> String {
    let mut out = String::with_capacity(cmdline.len());
    let mut rest = cmdline;
    const MARKER: &str = "systemd.clock_usec=";
    while let Some(idx) = rest.find(MARKER) {
        out.push_str(&rest[..idx]);
        let after_marker = &rest[idx + MARKER.len()..];
        let value_end = after_marker.find(' ').unwrap_or(after_marker.len());
        rest = &after_marker[value_end..];
    }
    out.push_str(rest);
    squeeze_spaces(&out).trim().to_string()
}

/// Strip any existing `systemd.clock_usec=` token, then append a fresh one
/// as the LAST token. Mirrors lines 130-133.
pub fn inject_clock(cmdline: &str, clock_usec_micros: u64) -> String {
    // Bash strips the old clock (line 130) WITHOUT the squeeze/trim that
    // `_strip_clock()` applies, then appends the new token, THEN squeezes +
    // trims once at the end (line 133) — same end result as squeezing after
    // a raw (unsqueezed) strip, since the append happens before the single
    // final squeeze/trim pass either way.
    let mut out = String::with_capacity(cmdline.len());
    let mut rest = cmdline;
    const MARKER: &str = "systemd.clock_usec=";
    while let Some(idx) = rest.find(MARKER) {
        out.push_str(&rest[..idx]);
        let after_marker = &rest[idx + MARKER.len()..];
        let value_end = after_marker.find(' ').unwrap_or(after_marker.len());
        rest = &after_marker[value_end..];
    }
    out.push_str(rest);
    out.push_str(&format!(" systemd.clock_usec={clock_usec_micros}"));
    squeeze_spaces(&out).trim().to_string()
}
