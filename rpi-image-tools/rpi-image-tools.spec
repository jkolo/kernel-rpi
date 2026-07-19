# rpi-image-tools — RHCOS image tooling for Raspberry Pi.
#
# Packages the files that are otherwise COPY'd from the build host in
# Containerfile.rpi{5,4}. Goal: enable On-Cluster Build (MachineOSConfig)
# where Containerfile is inline in CR and host COPY is not available.
#
# Contents (main package):
#   - /usr/lib/dracut/modules.d/99rpi-bls-sync/  — EFI ↔ BLS sync at boot/shutdown
#     (module-setup.sh; the sync binary itself is inst_binary'd from
#     /usr/sbin/rpi-bls-sync, not copied from this tree — see below)
#   - /usr/sbin/rpi-bls-sync                     — native Rust binary (rpi-bls-sync/),
#                                                   rewrite of the former bash script
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

# rpi-bls-sync (Cargo.toml [profile.release] strip = true) ships with no
# debug symbols by design (small production initramfs binary) — rpmbuild's
# automatic debuginfo/debugsource generation finds nothing to package and
# errors out on an empty %files list for -debugsource. Skip it entirely;
# standard idiom for RPM-packaged pre-stripped Rust/Go binaries.
%global debug_package %{nil}

Name:           rpi-image-tools
Version:        1.1.0
Release:        5%{?dist}
Summary:        RHCOS image tooling for Raspberry Pi (BLS sync, EEPROM, firmware config)

License:        GPLv2+
URL:            https://github.com/jkolo/kernel-rpi
Source0:        %{name}-%{version}.tar.gz

# rpi-bls-sync is a native Rust binary (glibc-dynamic) — no longer a
# noarch shell-script package. offline `cargo build` needs cargo+rust in
# the mock chroot; vendored crates are added to the SRPM tarball by
# copr/copr-rpi-image-tools.sh (network-enabled SRPM phase) so %build
# stays fully offline, matching every other COPR chroot in this project.
ExclusiveArch:  aarch64
BuildRequires:  cargo
BuildRequires:  rust

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
export CARGO_HOME=%{_builddir}/cargo-home
mkdir -p "$CARGO_HOME"
cd rpi-bls-sync
cargo build --release --offline --locked

%install
# Dracut module (full directory tree — module-setup.sh + the two
# initrd-context systemd units; the sync binary itself is inst_binary'd
# from /usr/sbin/rpi-bls-sync at initrd-build time, not copied here)
mkdir -p %{buildroot}/usr/lib/dracut/modules.d/99rpi-bls-sync
cp -r dracut/modules.d/99rpi-bls-sync/* %{buildroot}/usr/lib/dracut/modules.d/99rpi-bls-sync/

# Native sync binary (+x load-bearing — direct execve, no bash-wrapper
# open/read fallback the way the old script had)
install -D -m 0755 rpi-bls-sync/target/release/rpi-bls-sync \
    %{buildroot}/usr/sbin/rpi-bls-sync

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
%attr(0755,root,root) /usr/sbin/rpi-bls-sync
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
* Sun Jul 19 2026 Jerzy Kołosowski <jurek@kolosowscy.pl> - 1.1.0-5
- rpi-bls-sync: EFI-stub brick fix. locate_efi no longer trusts an existing
  /boot/efi mountpoint by presence alone — on RHCOS-RPi that can be an empty
  read-only stub (the deployment's own /boot/efi via composefs, or an early
  pre-FAT bind) instead of the real EFI-SYSTEM FAT. Trusting the stub made
  reads return nothing and writes hit EACCES, so the FAT was SILENTLY never
  updated → a dead ostree slot in cmdline.txt after the BLS swap → next-boot
  brick (confirmed canary). Now the backing device is compared to the
  EFI-SYSTEM PARTLABEL (parse_mount_device/efi_mount_is_real_fat); a non-FAT
  stub routes to a PARTLABEL temp-mount, and a genuinely missing EFI-SYSTEM
  fails LOUD (exit 1) instead of masquerading as a successful no-op sync.
- rpi-bls-sync: sync DTBs + overlays to the FAT, coupled to the kernel being
  synced. Firstboot seeds the device-tree via build-node-disks.sh but the
  runtime sync only ever touched kernel/initramfs/config.txt/cmdline — a
  day-2 kernel bump shipping new DTBs would leave the FAT with the OLD
  device-tree. Source is the SAME deployment as the kernel
  (`src_kernel.parent()/dtb/{broadcom,overlays}`, verified a sibling of the
  vmlinuz on a live node). Fail-closed: unsupported model or a missing dtb
  dir skips (pre-1.1.0-5 behaviour); a source read error aborts BEFORE the
  cmdline.txt commit so the last-good pointer is preserved. Idempotent by
  size — an in-sync node reads and writes nothing. GPU firmware (rpi4
  start4.elf/…) is intentionally not runtime-synced: it lives outside the
  per-deployment dtb dir and is firmware- not kernel-coupled. NOTE: the
  overlay sync is add/update-only — a bump that REMOVES an overlay leaves the
  stale .dtbo on the FAT (harmless unless config.txt still references it).
- rpi-bls-sync-shutdown.service: added After=ostree-finalize-staged.service.
  The authoritative day-2 sync point is the ExecStopPost drop-in (runs after
  the boot.N swap); this makes the fallback unit's ordering explicit. No
  behaviour change — liveness-first slot resolution already kept the FAT
  correct in either order; this removes the ambiguity and guards against
  future regressions.
* Fri Jul 17 2026 Jerzy Kołosowski <jurek@kolosowscy.pl> - 1.1.0-3
- rpi-bls-sync: config.txt clobber fix (cp-jurek brick 2026-07-17). The
  followkernel presence check is now LINE-anchored (trimmed, non-comment
  lines) — the pristine config-rpi{4,5}.txt documents the directive inside a
  header comment, which the old substring check matched, so the sync wrote
  the pristine copy to the FAT with no real directive → firmware loaded the
  kernel without initramfs → VFS panic on the next boot.
- rpi-bls-sync: config.txt is sourced from the RESOLVED EFI mount, never the
  raw /boot/efi path (context-ambiguous: empty mountpoint stub while /boot
  is mounted, the deployment's pristine copy after late-shutdown unmount).
  Mount-discipline: reads/writes only via partitions verified mounted,
  temp-mounting and unmounting when needed.
- rpi-bls-sync: dedup_tokens splits on all whitespace — cmdline.d trailing
  newlines produced a MULTI-LINE cmdline.txt of which RPi firmware reads
  only the first line (console= and systemd.clock_usec= silently dropped).
- config-rpi{4,5}.txt: reworded header comments so they no longer contain
  the verbatim directive string (defense in depth for older sync binaries).

* Sat Jul 11 2026 Jerzy Kołosowski <jurek@kolosowscy.pl> - 1.1.0-2
- rpi-bls-sync: fix firstboot slot brick (shutdown-after-finalize race). When
  ostree-finalize-staged swaps the bootversion at shutdown and prunes the OLD
  live slot, resolve() no longer refuses (which left a stale boot.N in
  cmdline.txt → next boot's ostree-prepare-root failed → dracut emergency);
  it now falls back to the finalized BLS slot. Existence-driven: prefer the
  live /proc slot if present, else the BLS slot if present, else refuse
  (DeployProbe now carries both candidate slots' on-disk presence).
- rpi-bls-sync: temp mountpoint /tmp → /run so the ostree-finalize-staged
  ExecStopPost hook can mount the boot/EFI partitions at late shutdown, when
  the root fs is already read-only (was "mkdir /tmp: Read-only file system" →
  the sync then silently never ran, the actual cause of the brick).
- Drop the retired bash oracle / QEMU harness from the test tree entirely;
  pure Rust unit tests only. Rename crate dir rpi-bls-sync-rs → rpi-bls-sync.
- Release bump: forces COPR/dnf to ship the rebuilt RPM (NVR de-dup guard).

* Thu Jul 02 2026 Jerzy Kołosowski <jurek@kolosowscy.pl> - 1.1.0-1
- rpi-bls-sync: rewritten from bash to a native Rust binary (rpi-bls-sync/,
  TDD with byte-parity oracle tests against the retired bash script). Fixes
  the whole 203/EXEC initramfs-fragility class at the root (no interpreter,
  no shebang mode-bit dependency) instead of patching around it.
- Adds two new runtime capabilities the bash script never had: config.txt
  followkernel-directive sync and (module-level, not yet wired into the
  runtime orchestration) DTB/overlay/GPU-firmware sync helpers, so an image
  update can refresh the boot partition without a manual reflash.
- Package is no longer noarch: ExclusiveArch aarch64, BuildRequires
  cargo+rust, offline `cargo build --release --offline --locked` (vendored
  crates added to the SRPM tarball by copr-rpi-image-tools.sh). Requires:
  bash/coreutils dropped — the binary is self-contained.
- module-setup.sh: inst_binary instead of inst_script/inst_multiple
  bash+awk; adds instmods vfat ext4 (mount(2) doesn't autoload filesystem
  modules the way mount(8) does, and the binary no longer execs mount(8)).
- Release bump: forces COPR/dnf to ship the rebuilt RPM (avoids NVR de-dup
  silently keeping pre-rewrite content, the root of the 468dd5c8 image
  regression).

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

* Sun May 24 2026 Jerzy Kołosowski <jurek@kolosowscy.pl> - 1.0.0-1
- Initial packaging: extract Containerfile COPY contents into RPM for
  on-cluster build (MachineOSConfig) compatibility. Source files mirrored
  from RHCOS-RaspberryPi/{config,dracut,systemd,scripts} at the time of
  this spec creation; sync from upstream by re-running tarball generator.
