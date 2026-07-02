#!/usr/bin/env bash
# Byte-identical extract of rpi-bls-sync.sh lines 151-182 — liveness-first
# ostree= slot re-resolution (the write-then-prune emergency-boot fix).
#
# Args:
#   $1 = CMDLINE          — pre-resolution cmdline string (BLS-derived)
#   $2 = PROC_CMDLINE      — simulated /proc/cmdline contents
#   $3 = ROOT              — real directory standing in for "/", so
#                            "$ROOT/ostree/deploy" and "$ROOT/sysroot/..."
#                            can be created/omitted per test case instead of
#                            needing actual root-owned mounts.
#
# Prints one line:
#   REFUSE                       — refuse to sync, keep last-good cmdline.txt
#   RESULT <resolved cmdline>    — the cmdline to write (possibly rewritten)
set -euo pipefail
export LC_ALL=C

CMDLINE="$1"
PROC_CMDLINE="$2"
ROOT="$3"

_OARG=$(printf '%s' "$CMDLINE" | grep -o 'ostree=[^ ]*' || true)
if [ -n "$_OARG" ]; then
    _OREST="${_OARG#ostree=/ostree/boot.*/}"
    _PROC=$(printf '%s' "$PROC_CMDLINE" | grep -o 'ostree=[^ ]*' | head -1 || true)
    if [ -n "$_PROC" ] && [ "$_PROC" != "$_OARG" ]; then
        _PREST="${_PROC#ostree=/ostree/boot.*/}"
        if [ "$_PREST" = "$_OREST" ]; then
            CMDLINE=$(printf '%s' "$CMDLINE" | sed "s|${_OARG}|${_PROC}|g")
            _OARG="$_PROC"
        fi
    fi
    _GDIR=$(dirname "${_OARG#ostree=}")
    _root_ok=0
    for _PFX in "" "/sysroot"; do [ -d "${ROOT}${_PFX}/ostree/deploy" ] && { _root_ok=1; break; }; done
    if [ "$_root_ok" = "1" ]; then
        _present=0
        for _PFX in "" "/sysroot"; do [ -d "${ROOT}${_PFX}${_GDIR}" ] && { _present=1; break; }; done
        if [ "$_present" = "0" ]; then
            echo "REFUSE"
            exit 0
        fi
    fi
fi

echo "RESULT $CMDLINE"
