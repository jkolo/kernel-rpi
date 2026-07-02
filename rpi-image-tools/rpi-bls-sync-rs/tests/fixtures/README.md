# Fixtures & oracle

`tests/oracle/*.sh` are byte-identical extracts of the corresponding logic in
`RHCOS-RaspberryPi/dracut/modules.d/99rpi-bls-sync/rpi-bls-sync.sh` (cited by
line number in each script's header comment), runnable standalone against
plain directories/strings — no real mount(2)/root required. `LC_ALL=C` is
pinned to match the Rust port's bytewise sort.

**Deviation from the approved plan (recorded, not silent):** the plan's
primary oracle was a golden corpus captured live from cp1/cp2/w1 over SSH.
The execution environment for this implementation pass has no network route
to the cluster (`tartarus.kolosowscy.pl` does not resolve — no Tinc client
here), so live capture was not possible from this session. Fixtures below
are hand-constructed instead, verified by running the actual bash-extract
oracle scripts against them (predictions confirmed to match real bash
output before any Rust test was written). Capturing the live golden corpus
from a session with cluster network access remains a follow-up — see the
plan file's T0 section.

## bls/ (Faza T1)

Each subdirectory mirrors `$BOOT_MOUNT` layout (`loader/entries/ostree-*.conf`):

- `single/` — one entry, sanity check.
- `multi_version/` — versions 1, 3, 2 → winner is version 3.
- `tie/` — two entries both version 5 → winner is the lexically-first filename
  (`ostree-1.conf`), proving strict `>` (not `>=`) tie-break.
- `non_integer/` — one entry `version foo` (skipped), one `version 0` (wins,
  since missing-or-invalid never lowers `BEST_VER` below its `-1` init but a
  *present* non-integer value is silently skipped, not defaulted).
- `missing_field/` — one entry with no `version` line at all → defaults to
  `0`, still beats the `-1` init and is selected.

Verified via `tests/oracle/bls_select.sh <dir>`.
