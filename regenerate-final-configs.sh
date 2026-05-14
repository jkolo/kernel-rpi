#!/usr/bin/env bash
# regenerate-final-configs.sh — produce frozen kernel config snapshots.
#
# Output: config-rpi5-final.cfg, config-rpi4-final.cfg (committed snapshots
# used by kernel.spec %prep — full flavor — instead of running merge sequence
# in spec). This is the c9s-style "cp full config + olddefconfig" model,
# adapted to our fragment-merge setup (fragments stay as inputs for audit).
#
# Run after:
#   1. Bumping BASE_DIGEST in ../Makefile (re-extract config-rhcos.cfg first
#      via extract-rhcos-config.sh).
#   2. Editing config-rhcos.cfg, config-rhcos-renames.cfg or config-bcm27xx.cfg.
#   3. Bumping stable_update or rpi_gitshort in kernel.spec.
#
# Dependencies (already in verify-merge.sh prereqs):
#   make, bc, flex, bison, curl, xz, patch, gcc (host, NOT aarch64-gcc —
#   Kconfig is a host tool that does not compile any kernel code).

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Discover values from kernel.spec — keeps generator in sync with build.
KVER_BASE=$(grep -oE '^%define base_sublevel [0-9]+' kernel.spec | awk '{print $3}')
KVER_STABLE=$(grep -oE '^%define stable_update [0-9]+' kernel.spec | awk '{print $3}')
KVER="6.${KVER_BASE}.${KVER_STABLE}"
RPI_GITSHORT=$(grep -oE '^%global rpi_gitshort [a-f0-9]+' kernel.spec | awk '{print $3}')
RPI_PATCH="bcm270x-linux-rpi-6.${KVER_BASE}.y-${RPI_GITSHORT}.patch.xz"

echo "=== regenerate-final-configs.sh ==="
echo "Kernel:      linux-${KVER}"
echo "RPi patch:   ${RPI_PATCH}"
echo

# Sanity: required ingredients must exist
for F in "$RPI_PATCH" 0001-Revert-Use-kernel-command-line-to-disable-memory-cgr.patch \
         config-rhcos.cfg config-rhcos-renames.cfg config-bcm27xx.cfg; do
    [ -f "$SCRIPT_DIR/$F" ] || { echo "ERROR: missing $F" >&2; exit 1; }
done

CACHE="${HOME}/.cache/kernel-rpi-verify"
TARBALL="${CACHE}/linux-${KVER}.tar.xz"
WORK="$(mktemp -d)"
trap "rm -rf $WORK" EXIT

mkdir -p "$CACHE"
if [ ! -f "$TARBALL" ]; then
    echo "Downloading linux-${KVER}.tar.xz..."
    curl -L -o "$TARBALL" "https://www.kernel.org/pub/linux/kernel/v6.x/linux-${KVER}.tar.xz"
fi

echo "Extracting kernel source..."
tar -xf "$TARBALL" -C "$WORK"
cd "$WORK/linux-${KVER}"

echo "Applying RPi mega-patch..."
xz -d -k -c "$SCRIPT_DIR/$RPI_PATCH" | patch -p1 -s
patch -p1 -s < "$SCRIPT_DIR/0001-Revert-Use-kernel-command-line-to-disable-memory-cgr.patch"

# Filter for RPi defconfig (merged LAST, would otherwise override RHCOS):
#   (a) "is not set" entries that RHCOS wants =y/=m → drop so RHCOS wins
#   (b) ARM64 page-size + VA_BITS — RPi defconfig prefers 16K pages (rpi5) /
#       VA_BITS_39 (rpi4) for hw performance, but RHCOS userspace (rhel-coreos
#       aarch64) was built for 4K pages + VA=48. Switching introduces an ABI
#       variable; keep RHCOS's choice (4K + VA=48) for continuity.
#   (c) PREEMPT model — defconfig=PREEMPT (full), RHCOS=PREEMPT_VOLUNTARY.
#       Voluntary is better for syscall-heavy container workloads; keep RHCOS.
#   (d) Module-vs-builtin: drop =y for every symbol RHCOS keeps =m. RPi defconfig
#       builds ethernet/PHY/MMC/NVME/USB-storage/FS as =y, which broke BCMGENET
#       deferred probe ordering (genet vs unimac-mdio) on bootstrap -6 build —
#       no link, no PHY attach. Old -4 spec merged RHCOS AFTER defconfig so
#       RHCOS =m won; here we instead surgically filter so RHCOS server policy
#       (modules loaded lazily by udev once firmware is ready) wins again.
DEFCONFIG_FILTER_STATIC='
/^# CONFIG_\(STRICT_DEVMEM\|LEGACY_PTYS\|UPROBE_EVENTS\|CRYPTO_HW\) is not set$/d
/^CONFIG_ARM64_\(4K\|16K\|64K\)_PAGES=/d
/^# CONFIG_ARM64_\(4K\|16K\|64K\)_PAGES is not set$/d
/^CONFIG_ARM64_VA_BITS_\(39\|42\|47\|48\|52\)=/d
/^# CONFIG_ARM64_VA_BITS_\(39\|42\|47\|48\|52\) is not set$/d
/^CONFIG_PREEMPT\(_NONE\|_VOLUNTARY\|_RT\|=y\)/d
/^# CONFIG_PREEMPT\(_NONE\|_VOLUNTARY\|_RT\| is not set\)/d
'

# Dynamic part: every CONFIG_X=m in RHCOS → strip =y from defconfig so merge
# keeps the module form. Generated once and reused for both rpi5/rpi4.
DEFCONFIG_FILTER_DYNAMIC=$(awk -F= '
    /^CONFIG_[A-Z0-9_]+=m$/ { print "/^" $1 "=y$/d" }
' "$SCRIPT_DIR/config-rhcos.cfg")
DEFCONFIG_FILTER="${DEFCONFIG_FILTER_STATIC}${DEFCONFIG_FILTER_DYNAMIC}"

for VARIANT in rpi5 rpi4; do
    case "$VARIANT" in
        rpi5) DEFCONFIG=bcm2712_defconfig ;;
        rpi4) DEFCONFIG=bcm2711_defconfig ;;
    esac
    OUT="$SCRIPT_DIR/config-${VARIANT}-final.cfg"
    echo
    echo "=== Building $VARIANT snapshot ($DEFCONFIG) ==="

    make -s mrproper
    make ARCH=arm64 -s allnoconfig

    echo "[1/5] merge config-rhcos.cfg (RHCOS server baseline)"
    scripts/kconfig/merge_config.sh -m -r .config "$SCRIPT_DIR/config-rhcos.cfg" > /dev/null

    echo "[2/5] merge config-rhcos-renames.cfg (5.14 -> 6.18 skew bridge)"
    scripts/kconfig/merge_config.sh -m -r .config "$SCRIPT_DIR/config-rhcos-renames.cfg" > /dev/null

    echo "[3/5] merge bcm271X_defconfig (filtered, RPi platform LAST hardware)"
    cp "arch/arm64/configs/${DEFCONFIG}" "$WORK/rpi-defconfig.cfg"
    sed -i "$DEFCONFIG_FILTER" "$WORK/rpi-defconfig.cfg"
    scripts/kconfig/merge_config.sh -m -r .config "$WORK/rpi-defconfig.cfg" > /dev/null

    echo "[4/5] merge config-bcm27xx.cfg (RPi extras)"
    scripts/kconfig/merge_config.sh -m -r .config "$SCRIPT_DIR/config-bcm27xx.cfg" > /dev/null

    echo "[5/5] make olddefconfig (resolve deps)"
    make ARCH=arm64 -s olddefconfig

    {
        echo "# Generated by regenerate-final-configs.sh on $(date -uIs)"
        echo "# Kernel:    linux-${KVER}"
        echo "# RPi patch: ${RPI_PATCH} (rpi-6.${KVER_BASE}.y, ${RPI_GITSHORT})"
        echo "# Variant:   $VARIANT ($DEFCONFIG)"
        echo "# Source ingredient hashes (sha256, first 16 chars):"
        for F in config-rhcos.cfg config-rhcos-renames.cfg config-bcm27xx.cfg; do
            echo "#   $F: $(sha256sum "$SCRIPT_DIR/$F" | cut -c1-16)"
        done
        echo "# DO NOT EDIT MANUALLY — edit fragments and re-run regenerate-final-configs.sh"
        echo
        cat .config
    } > "$OUT"

    echo "  -> wrote $OUT ($(wc -l < "$OUT") lines)"
done

echo
echo "=== Done ==="
echo "Commit config-rpi5-final.cfg + config-rpi4-final.cfg along with any fragment changes."
