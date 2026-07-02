#!/usr/bin/env bash
# Byte-identical extract of rpi-bls-sync.sh lines 19-27 — RPi model
# detection. Arg $1 = simulated /proc/device-tree/model contents (raw,
# NUL-terminated strings from that file are not representable as a bash
# arg, so callers pass the string without trailing NUL — `tr -d '\0'` is a
# no-op on such input, which is fine: it only strips what's already absent).
set -euo pipefail
export LC_ALL=C

MODEL="$1"
case "$MODEL" in
    *"Raspberry Pi 5"*) echo kernel_2712.img ;;
    *"Raspberry Pi 4"*) echo kernel8.img ;;
    *) echo "UNSUPPORTED" ;;
esac
