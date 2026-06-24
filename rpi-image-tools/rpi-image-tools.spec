# rpi-image-tools — RHCOS image tooling for Raspberry Pi.
#
# Packages the files that are otherwise COPY'd from the build host in
# Containerfile.rpi{5,4}. Goal: enable On-Cluster Build (MachineOSConfig)
# where Containerfile is inline in CR and host COPY is not available.
#
# Contents (main package):
#   - /usr/lib/dracut/modules.d/99rpi-bls-sync/  — EFI ↔ BLS sync at boot/shutdown
#   - /usr/sbin/rpi-bls-sync.sh                  — same script outside dracut module
#   - /usr/lib/systemd/system/rpi-bls-sync.{path,service}
#   - /usr/lib/systemd/system/rpi-bls-sync-shutdown.service
#   - /usr/lib/systemd/system/coreos-rpi-remove-firstboot.service
#   - /usr/lib/systemd/system/ostree-finalize-staged.service.d/10-rpi-bls-sync.conf
#   - /usr/libexec/rpi-eeprom-config             — Python helper for EEPROM config
#   - /etc/cmdline.d/98-rpi-boot.conf            — RPi-common kernel cmdline drop-in
#
# Subpackages (mutually exclusive — only one installed per node):
#   - rpi-image-tools-rpi5: /boot/efi/config.txt for BCM2712 + dependent overlays
#   - rpi-image-tools-rpi4: /boot/efi/config.txt for BCM2711
#
# Build via COPR jkolo/kernel-rpi (Custom source type, see
# copr/copr-rpi-image-tools.sh for the build script).

Name:           rpi-image-tools
Version:        1.0.0
Release:        2%{?dist}
Summary:        RHCOS image tooling for Raspberry Pi (BLS sync, EEPROM, firmware config)

License:        GPLv2+
URL:            https://github.com/jkolo/kernel-rpi
Source0:        %{name}-%{version}.tar.gz

BuildArch:      noarch
Requires:       bash
Requires:       coreutils

%description
RHCOS-on-RaspberryPi support files: BLS↔EFI sync dracut module + systemd units,
coreos-rpi-remove-firstboot helper, rpi-eeprom-config Python tool, and the
RPi-common kernel cmdline drop-in (cgroups). Hardware-specific config.txt for
RPi5 (BCM2712) and RPi4 (BCM2711) ships in separate subpackages.

%package rpi5
Summary:        Boot firmware config for Raspberry Pi 5 (BCM2712)
Requires:       %{name} = %{version}-%{release}
Conflicts:      %{name}-rpi4

%description rpi5
config.txt for Raspberry Pi 5 (BCM2712 SoC, 16k pages, RP1 chip). Mutually
exclusive with rpi-image-tools-rpi4 — installs to /boot/efi/config.txt.

%package rpi4
Summary:        Boot firmware config for Raspberry Pi 4 (BCM2711)
Requires:       %{name} = %{version}-%{release}
Conflicts:      %{name}-rpi5

%description rpi4
config.txt for Raspberry Pi 4 (BCM2711 SoC, 4k pages). Mutually exclusive
with rpi-image-tools-rpi5 — installs to /boot/efi/config.txt.

%prep
%autosetup

%build
# Nothing to compile — pure file install package.

%install
# Dracut module (full directory tree)
mkdir -p %{buildroot}/usr/lib/dracut/modules.d/99rpi-bls-sync
cp -r dracut/modules.d/99rpi-bls-sync/* %{buildroot}/usr/lib/dracut/modules.d/99rpi-bls-sync/

# rpi-bls-sync.sh also outside dracut module for direct invocation
install -D -m 0755 dracut/modules.d/99rpi-bls-sync/rpi-bls-sync.sh \
    %{buildroot}/usr/sbin/rpi-bls-sync.sh

# Systemd units
mkdir -p %{buildroot}/usr/lib/systemd/system
install -m 0644 systemd/rpi-bls-sync.path             %{buildroot}/usr/lib/systemd/system/
install -m 0644 systemd/rpi-bls-sync.service          %{buildroot}/usr/lib/systemd/system/
install -m 0644 systemd/rpi-bls-sync-shutdown.service %{buildroot}/usr/lib/systemd/system/
install -m 0644 systemd/coreos-rpi-remove-firstboot.service %{buildroot}/usr/lib/systemd/system/

# Systemd drop-in for ostree-finalize-staged
mkdir -p %{buildroot}/usr/lib/systemd/system/ostree-finalize-staged.service.d
install -m 0644 systemd/ostree-finalize-staged.service.d/10-rpi-bls-sync.conf \
    %{buildroot}/usr/lib/systemd/system/ostree-finalize-staged.service.d/

# EEPROM config Python helper — /usr/libexec/ is rpm-ostree friendly
# (/usr/local/ is symlinked to /var/usrlocal at runtime, /usr/libexec/ stays in /usr).
install -D -m 0755 scripts/rpi-eeprom-config %{buildroot}/usr/libexec/rpi-eeprom-config

# Kernel cmdline drop-in
install -D -m 0644 config/cmdline.d/98-rpi-boot.conf \
    %{buildroot}/etc/cmdline.d/98-rpi-boot.conf

# Per-hardware config.txt — owned by subpackages
install -D -m 0644 config/config-rpi5.txt %{buildroot}/boot/efi/config-rpi5.txt
install -D -m 0644 config/config-rpi4.txt %{buildroot}/boot/efi/config-rpi4.txt

%post rpi5
# Install RPi5 config.txt as /boot/efi/config.txt
install -D -m 0644 /boot/efi/config-rpi5.txt /boot/efi/config.txt

%post rpi4
# Install RPi4 config.txt as /boot/efi/config.txt
install -D -m 0644 /boot/efi/config-rpi4.txt /boot/efi/config.txt

%files
/usr/lib/dracut/modules.d/99rpi-bls-sync/
/usr/sbin/rpi-bls-sync.sh
/usr/lib/systemd/system/rpi-bls-sync.path
/usr/lib/systemd/system/rpi-bls-sync.service
/usr/lib/systemd/system/rpi-bls-sync-shutdown.service
/usr/lib/systemd/system/coreos-rpi-remove-firstboot.service
/usr/lib/systemd/system/ostree-finalize-staged.service.d/10-rpi-bls-sync.conf
/usr/libexec/rpi-eeprom-config
/etc/cmdline.d/98-rpi-boot.conf

%files rpi5
/boot/efi/config-rpi5.txt

%files rpi4
/boot/efi/config-rpi4.txt

%changelog
* Wed Jun 24 2026 Jerzy Kołosowski <jurek@kolosowscy.pl> - 1.0.0-2
- rpi-bls-sync: liveness-first ostree boot-slot resolution — adopt the
  /proc/cmdline boot-slot whenever the BLS entry refers to the running
  deployment (same stateroot/csum/serial), and refuse to write an ostree=
  whose deployment dir is absent. Fixes the write-then-prune race in which a
  sync wrote a boot.N slot that ostree pruned moments later → next boot's
  ostree-prepare-root failed → dracut emergency (bricked cp1 + w1).
- rpi-bls-sync: dedup duplicated cgroup/swap/console kargs (order-preserving,
  keep-first); clock-agnostic idempotency compare (no flapping w/o RTC).
- module-setup: inst_multiple awk + chmod 0755 the initramfs script as a
  belt-and-suspenders guard against the 203/EXEC (mode-0644) regression.
- Release bump: forces COPR/dnf to ship the rebuilt RPM (avoids NVR de-dup
  silently keeping pre-fix content, the root of the 468dd5c8 image regression).

* Sat May 24 2026 Jerzy Kołosowski <jurek@kolosowscy.pl> - 1.0.0-1
- Initial packaging: extract Containerfile COPY contents into RPM for
  on-cluster build (MachineOSConfig) compatibility. Source files mirrored
  from RHCOS-RaspberryPi/{config,dracut,systemd,scripts} at the time of
  this spec creation; sync from upstream by re-running tarball generator.
