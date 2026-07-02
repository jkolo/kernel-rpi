#!/usr/bin/env bash
# Byte-identical extracts of rpi-bls-sync.sh lines 101-133 + 213 (cmdline.txt
# assembly: firstboot expansion, dedup, clock inject/strip). Subcommand
# dispatch so each pure operation can be exercised standalone.
set -euo pipefail
export LC_ALL=C

cmd="$1"; shift

case "$cmd" in
    expand_firstboot)
        # Args: BLS_OPTIONS FIRSTBOOT  (mirrors lines 101-109)
        BLS_OPTIONS="$1"
        FIRSTBOOT="$2"
        printf '%s' "$BLS_OPTIONS" \
            | sed "s/\\\$ignition_firstboot/${FIRSTBOOT}/g" \
            | tr -s ' '
        ;;
    dedup)
        # Args: CMDLINE  (mirrors line 126)
        CMDLINE="$1"
        printf '%s' "$CMDLINE" | tr ' ' '\n' | awk 'NF && !seen[$0]++' | tr '\n' ' ' | sed 's/ $//'
        ;;
    inject_clock)
        # Args: CMDLINE CLOCK_USEC  (mirrors lines 130-133, $(date ...) replaced
        # by an explicit arg for determinism)
        CMDLINE="$1"
        CLOCK_USEC="$2"
        CMDLINE=$(printf '%s' "$CMDLINE" | sed 's/systemd\.clock_usec=[^ ]*//g')
        CMDLINE="$CMDLINE systemd.clock_usec=${CLOCK_USEC}"
        printf '%s' "$CMDLINE" | tr -s ' ' | sed 's/^ //;s/ $//'
        ;;
    strip_clock)
        # Args: CMDLINE  (mirrors the _strip_clock() helper, line 213)
        CMDLINE="$1"
        printf '%s' "$CMDLINE" | sed 's/systemd\.clock_usec=[^ ]*//g' | tr -s ' ' | sed 's/^ //;s/ $//'
        ;;
    *)
        echo "unknown subcommand: $cmd" >&2
        exit 2
        ;;
esac
