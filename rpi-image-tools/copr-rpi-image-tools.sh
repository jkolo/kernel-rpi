#!/bin/bash
# COPR custom build script for rpi-image-tools — files extracted from
# Containerfile.rpi5/rpi4 COPY directives. Enables on-cluster build (OCB
# via MachineOSConfig) where host COPY is unavailable.
#
# Source: https://github.com/jkolo/kernel-rpi (branch dw-6.18.y)
# Layout: rpi-image-tools/ directory at repo root contains:
#   - rpi-image-tools.spec
#   - rpi-image-tools/{config,dracut,systemd,scripts}/ — source files
#
# COPR configuration:
#   Project: jkolo/kernel-rpi
#   Package: rpi-image-tools
#   Chroots: epel-9-aarch64
#   Source type: Custom
#   Script: contents of this file
#   Builddeps: tar gzip rpm-build

set -ex

# Clone kernel spec repository (contains rpi-image-tools/ subdirectory)
git clone --depth=1 -b dw-6.18.y https://github.com/jkolo/kernel-rpi.git
cd kernel-rpi/rpi-image-tools

# Extract version from spec
VERSION=$(grep -E '^Version:' rpi-image-tools.spec | awk '{print $2}')
PKG_NAME="rpi-image-tools-${VERSION}"

# Stage source tree for the tarball.
# Spec uses %autosetup which expects rpi-image-tools-VERSION/ as top-level dir
# containing config/, dracut/, systemd/, scripts/ subdirectories.
TARDIR=$(mktemp -d)
mkdir -p "${TARDIR}/${PKG_NAME}"
cp -r config dracut systemd scripts "${TARDIR}/${PKG_NAME}/"
tar -C "${TARDIR}" -czf "rpi-image-tools-${VERSION}.tar.gz" "${PKG_NAME}"

# Deliver spec + tarball to COPR
cp rpi-image-tools.spec "$COPR_RESULTDIR/"
cp "rpi-image-tools-${VERSION}.tar.gz" "$COPR_RESULTDIR/"

# Cleanup
rm -rf "${TARDIR}"
