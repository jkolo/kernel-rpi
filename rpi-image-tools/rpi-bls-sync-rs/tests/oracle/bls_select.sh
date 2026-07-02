#!/usr/bin/env bash
# Byte-identical extract of rpi-bls-sync.sh (RHCOS-RaspberryPi/dracut/modules.d/
# 99rpi-bls-sync/rpi-bls-sync.sh) lines 66-72 — the BLS default-entry
# selection algorithm. Runs for real, no mount/root required: $1 is a
# directory laid out like $BOOT_MOUNT (i.e. containing loader/entries/).
#
# LC_ALL=C pinned: glob order (tie-break) is locale-dependent in bash; the
# Rust port sorts bytewise (Ord on &[u8]), so the oracle must match that,
# not whatever locale happens to be active on the dev host or CI runner.
set -euo pipefail
export LC_ALL=C

BOOT_MOUNT="$1"
BLS=""; BEST_VER=-1
for _e in "$BOOT_MOUNT"/loader/entries/ostree-*.conf; do
    [ -f "$_e" ] || continue
    _v=$(awk '/^version[[:space:]]/ { print $2; exit }' "$_e")
    _v=${_v:-0}
    if [ "$_v" -gt "$BEST_VER" ] 2>/dev/null; then BEST_VER=$_v; BLS=$_e; fi
done
printf '%s\n' "$BLS"
