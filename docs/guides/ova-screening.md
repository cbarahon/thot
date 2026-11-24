# OVA screening with libvirt on KVM-Alpine

**Status (2026-11-24):** the Thot `libvirt-vm` module and its Alpine runtime
dependencies pass a complete lifecycle smoke run using a small synthetic OVA.
The supplied 8.49 GB Arch Linux OVA has not completed an import or VM boot on
the test host. Its OVF describes a 60 GiB disk and the host currently lacks
enough free disk space to stage another complete run safely. The smoke result
does not establish multiboot, GRUB, EFI, Windows, or shared-data correctness.

The module uses KVM-Alpine as a disposable execution guest and runs libvirt
inside that guest. It does not use the host's libvirt socket from test code.
The tested host did not pass nested `/dev/kvm` into Alpine, so imported VMs ran
with QEMU TCG emulation.

## What this workflow currently checks

The example project runs four ordered tests:

1. `libvirt/import_ova`: safely extracts an OVA, verifies any included
   manifest, reads the OVF, converts VDI/VMDK disks to qcow2, and defines an
   offline libvirt domain from the supported OVF properties.
2. `libvirt/start_vm`: starts the defined domain.
3. `libvirt/stop_vm`: requests ACPI shutdown and forces a bounded stop if the
   guest does not shut down.
4. `libvirt/destroy_vm`: verifies Thot ownership, undefines the domain, and
   removes its private workspace.

Import carries CPU, memory, disk capacity, and BIOS/EFI choice where present.
EFI uses fresh OVMF variables; VirtualBox NVRAM is not reused. Network
interfaces are intentionally omitted. This module does not inspect partitions,
identify installed operating systems, select GRUB entries, boot each OS, or
verify shared-data access. Those remain future tests.

The module uses conservative limits: one OVF descriptor, at most 64 regular
archive members, 20 GiB OVA and extracted-data limits, up to 128 GiB combined
virtual disk capacity, and bounded CPU/RAM. The current importer reserves
space conservatively for the OVA, extracted disk, and full virtual disk before
conversion. Unsupported or oversized artifacts produce captured test errors;
insufficient platform capacity is recorded as infrastructure-pending rather
than as a student failure.

## Fresh Debian 13 host

Use an x86_64 Debian 13 host with working KVM (`/dev/kvm`), libvirt and a
default NAT network for image and package preparation. The outer Alpine guest
needs network access during template installation and preparation. Student
tests run without network access. Reserve at least 100 GiB for the configured
outer guest and enough additional host storage for the OVA, project run input,
and qcow2 overlays. The test host's 59 GiB root filesystem is too small for
this real OVA profile.

Install Thot from a release that includes the current KVM-Alpine provider and
the `thot-alpine-agent` binary, then install the host tools:

```sh
curl -fLO https://github.com/cbarahon/thot/releases/latest/download/thot-install.sh
sudo bash thot-install.sh
sudo apt update
sudo apt install qemu-system-x86 libvirt-daemon-system libvirt-clients virtinst \
  libguestfs-tools cloud-image-utils curl
sudo systemctl enable --now libvirtd
sudo virsh net-autostart default
test -c /dev/kvm
sudo virsh net-list --all
thot sandbox --help
```

The CLI must be able to access system libvirt and its storage. In this
implementation, run it with the privileges needed by the local KVM provider.
Set `THOT_KVM_ALPINE_ROOT` to a libvirt-accessible directory (for example
`/var/lib/libvirt/images/thot-ova-screening`), and set `TMPDIR` there as well
if `/tmp` is a small tmpfs. Do not place VM storage under a private home
directory that the libvirt service account cannot traverse.

## Create the professor project

Start from the complete example project in
[`examples/ova-screening-kvm-alpine`](../../examples/ova-screening-kvm-alpine/):

```sh
cp -a examples/ova-screening-kvm-alpine "$HOME/ova-screening-kvm-alpine"
cd "$HOME/ova-screening-kvm-alpine"
mkdir -p deliveries results
```

The example already contains the roster, assignment, local delivery agent,
preparation script, pinned module and four test definitions. Put the OVA in
the configured local delivery folder, or change `delivery_agent.path` to the
professor's folder:

```sh
cp /path/to/ArchLinux_Dualboot.ova deliveries/
```

`delivery_limits.max_bytes` is set to 20 GiB so the 8.49 GB OVA can be
acquired. The example requests a 100 GiB Alpine disk because the OVF disk has a
60 GiB virtual capacity; verify actual host free space before running. The
project should not be started on the 59 GiB test host.

The preparation script installs Bash, Python, QEMU/libvirt, OVMF, D-Bus, and
PolicyKit. It sets a guest-local PolicyKit rule for the `libvirt` group and
adds the agent's UID 1001 identity. The guest agent starts only a fixed list
of guest-local services for a pinned module requiring `libvirt.system@1` and
adds that group to that process. No host libvirt socket is mounted or
forwarded into Alpine.

## Run and review

Run the CLI against the project directory:

```sh
sudo env \
  THOT_KVM_ALPINE_ROOT=/var/lib/libvirt/images/thot-ova-screening \
  TMPDIR=/var/lib/libvirt/images/thot-ova-screening/thot-tmp \
  thot sandbox run "$HOME/ova-screening-kvm-alpine"
```

The command reports a per-student grade or `pending infrastructure review`.
Use the run ID printed by Thot to inspect reports:

```sh
thot sandbox report "$HOME/ova-screening-kvm-alpine/results/RUN_ID"
thot sandbox report "$HOME/ova-screening-kvm-alpine/results/RUN_ID" \
  --csv "$HOME/ova-screening-kvm-alpine/results/grades.csv"
thot sandbox report "$HOME/ova-screening-kvm-alpine/results/RUN_ID" \
  --html "$HOME/ova-screening-kvm-alpine/results/report.html"
```

Keep the entire project directory to archive or transfer an evaluation. Run
inputs, project snapshots, per-student JSON, captured stdout/stderr artifacts,
hash-chained event journal, and revision metadata live under `results/`.
Infrastructure-pending results have a null grade and must not be interpreted
as student zeros.

The current local KVM CLI stores sandbox reports itself; this path does not
create a legacy daemon expedient. Do not use the legacy `thot run create`
command for this KVM project.

## Test-host evidence and limits

Test host: `sysadmin@192.168.1.35`, Debian 13.7, 4 CPUs, about 5.8 GiB RAM,
59 GiB root filesystem. The source OVA is
`/home/sysadmin/ArchLinux_Dualboot.ova`, 8,487,809,024 bytes, SHA-256
`3a9195aa6d7f0842f297e065bc04b66ae2da18380da287f5feaa2a273af030b5`.
The OVA was acquired and hash-verified earlier. A previous full-project run
(`5fc5248e-c71b-4988-b608-7e4d1bb07247`) prepared the 20 GiB Alpine baseline
and persisted the delivery, then recorded an infrastructure-pending result
because the offline student clone did not have an available libvirt socket.
That run did not import the OVA.

The following issues were found and corrected during synthetic smoke runs:

- Alpine `virtproxyd` required network in its OpenRC dependencies although
  student clones are offline. Preparation changes it to an optional
  dependency.
- Guest-local libvirt requires D-Bus and PolicyKit. Preparation now installs
  and enables both.
- PolicyKit needs a named UID 1001 identity and an explicit guest-local
  authorization rule for the `libvirt` group.
- The guest agent starts D-Bus, PolicyKit, and the modular libvirt daemons for
  the pinned capability, then grants that module process only the `libvirt`
  supplementary group.
- Libvirt rejected a `graphics type="none"` element, a conflict between two
  boot-order declarations, and Python `Path` objects in the module JSON result.
  All were corrected.

Final synthetic lifecycle smoke run:

- Run ID: `d8bf796b-5e8c-4e58-aead-9f76a831963a`.
- Fixture: generated 70 KiB OVA containing one 64 MiB VMDK and a minimal OVF.
- `import_ova`, `start_vm`, `stop_vm`, and `destroy_vm` all exited 0; result
  grade was 10/10, assessment complete, cleanup clean.
- Shutdown used bounded forced stop because the synthetic disk has no bootable
  operating system to process ACPI.
- `virsh list --all` was empty after cleanup. The run and fixture are retained
  under `/home/sysadmin/ova-screening-kvm-alpine/results/`.

The actual supplied OVA has not been rerun after these fixes. The host had
about 11 GiB free after retaining prior OVA copies, run inputs, smoke baselines,
and logs, which is insufficient for another 8.49 GiB run input plus the
student-clone staging copy. Its declared 60 GiB disk also cannot be imported
into the earlier 20 GiB outer guest. This is a host-capacity limitation, not a
student result. A properly sized host still needs a real run to qualify OVF
compatibility, EFI boot, multiboot behavior, nested acceleration, Windows, and
shared-data checks.

See the [platform roadmap](../planning/platform-roadmap.md),
[OVA multiboot plan](../planning/ova-multiboot.md), and
[KVM-Alpine implementation record](../implementation/kvm-alpine.md) for the
remaining work.
