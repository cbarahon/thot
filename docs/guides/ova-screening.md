# OVA screening with libvirt on KVM-Alpine

**Status (2026-11-25):** the Thot `libvirt-vm` module and its Alpine runtime
dependencies pass a complete lifecycle smoke run using a small synthetic OVA.
The supplied 8.49 GB Arch Linux OVA has not completed an import or VM boot on
the test host. Its OVF describes a 60 GiB disk. The test host's disk has been
expanded to 120 GiB and its ext4 root filesystem now uses the full disk; the
real OVA run is being retried with the updated resource profile. The smoke result
does not establish multiboot, GRUB, EFI, Windows, or shared-data correctness.

The module uses KVM-Alpine as a disposable execution guest and runs libvirt
inside that guest. It does not use the host's libvirt socket from test code.
The tested host did not pass nested `/dev/kvm` into Alpine, so imported VMs ran
with QEMU TCG emulation.

The unified CLI background path was separately smoke-tested on the recovered
`sysadmin@192.168.1.33` Debian 13/KVM host with the three-student Bash fixture.
Run `736e3b95-5d3d-430d-a7c3-ed88ed609095` completed with 10/10, 2/10, and
0/10; `status`, `log`, `report`, `status --watch`, and verified run archive
export/import worked. This confirms the local worker and evidence workflow, not
the supplied OVA evaluation. The test project and results remain under
`/home/sysadmin/thot-improvements/project` on that host. The host's professor
account was added to `kvm`, `libvirt`, and `libvirt-qemu`, and the dedicated
storage directory was granted QEMU access after an initial permission failure.

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

Import carries disk capacity and BIOS/EFI choice where present. The project
caps nested guests at 2 vCPUs and 2048 MiB RAM; if the OVF requests more, Thot
uses those caps and reports requested and applied values.
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
tests run without network access. This example configures the outer Alpine VM
with 4 vCPUs, 6144 MiB RAM, and a sparse 100 GiB virtual disk. Its imported
student VM is capped at 2 vCPUs and 2048 MiB RAM. Reserve at least 100 GiB for the configured
outer guest and enough additional host storage for the temporary OVA staging,
extraction and qcow2 overlays. Thot removes the staged deliverable after its
student tests and stores only provenance and SHA-256 in the result. The test
host's 118 GiB root filesystem has about 82 GiB free after resizing; actual
host usage must be monitored because the outer qcow2 disk is sparse and can
grow as the guest writes data.

Install Thot from a release that includes the current KVM-Alpine provider and
the `thot-alpine-agent` binary, then install the host tools:

```sh
curl -fLO https://github.com/cbarahon/thot/releases/latest/download/thot-install.sh
sudo bash thot-install.sh
sudo apt update
sudo apt install qemu-system-x86 libvirt-daemon-system libvirt-clients virtinst \
  libguestfs-tools cloud-image-utils curl
sudo systemctl enable --now libvirtd
sudo virsh net-start default
sudo virsh net-autostart default
test -c /dev/kvm
sudo env LC_ALL=C virsh net-info default
thot --help
```

The CLI must be able to access system libvirt and its storage. Add your
professor account to the `kvm`, `libvirt`, and `libvirt-qemu` groups during host
setup, then log out and back in. Create the dedicated storage root owned by the
professor and group `libvirt-qemu`, with mode `2770`, as shown in the deployment
guide. The QEMU group access is needed for linked backing images after libvirt
changes their ownership. The installer supplies a system-wide storage root and
temp settings. You can override them in project `runtime`, `.env`, or the
documented environment variables for a specific machine. Do not place VM
storage under a private home directory that the libvirt service account cannot
traverse.

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
60 GiB virtual capacity. Its outer VM resource settings and nested VM limits
are in the project YAML; verify host free space before running.

The preparation script installs Bash, Python, QEMU/libvirt, OVMF, D-Bus, and
PolicyKit. It sets a guest-local PolicyKit rule for the `libvirt` group and
adds the agent's UID 1001 identity. The guest agent starts only a fixed list
of guest-local services for a pinned module requiring `libvirt.system@1` and
adds that group to that process. No host libvirt socket is mounted or
forwarded into Alpine.

## Run and review

Run the CLI against the project directory:

```sh
thot validate
thot run
```

`thot run` returns a run ID while the evaluation continues in the background.
Follow it with `thot status --watch`, inspect technical details with
`thot log --follow`, and display the saved grades with `thot report`. The
report uses per-student grades or `pending infrastructure review`; infrastructure
failures remain pending rather than becoming student zeros. To open an older
run, pass its run ID:

```sh
thot report RUN_ID
thot report RUN_ID \
  --csv "$HOME/ova-screening-kvm-alpine/results/grades.csv"
thot report RUN_ID \
  --html "$HOME/ova-screening-kvm-alpine/results/report.html"
```

Keep the entire project directory to archive or transfer an evaluation. Run
inputs, project snapshots, per-student JSON, captured stdout/stderr artifacts,
hash-chained event journal, and revision metadata live under `results/`.
Infrastructure-pending results have a null grade and must not be interpreted
as student zeros.

The local KVM evaluation uses the same professor-facing `thot run` command
and durable background status/log path as other projects. Existing daemon
projects retain their legacy persisted expedients and detailed report commands.

## Test-host evidence and limits

Test host: `sysadmin@192.168.1.35`, Debian 13.7, 4 CPUs, about 5.8 GiB RAM,
59 GiB root filesystem. The source OVA is
`/home/sysadmin/ArchLinux_Dualboot.ova`, 8,487,809,024 bytes, SHA-256
`3a9195aa6d7f0842f297e065bc04b66ae2da18380da287f5feaa2a273af030b5`.
The OVA was acquired and hash-verified earlier. A previous full-project run
(`5fc5248e-c71b-4988-b608-7e4d1bb07247`) used the older behavior that also
persisted the delivery in its run data; it prepared the 20 GiB Alpine baseline,
then recorded an infrastructure-pending result
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

The supplied OVA was retried after the module fixes, using a 100 GiB Alpine
guest and the matching `0.1.46 protocol=2` agent pin. Run
`cd9ec275-0e79-48ec-87f3-797b31afea85` transferred the complete OVA and reached
`import_ova`, which exposed a standard OVF manifest whitespace format that the
parser rejected. The parser now accepts optional spaces around the manifest
algorithm, filename, and equals sign.

Run `6a7cdddc-7d95-4a15-8e81-9fea4da9a143` retried the supplied OVA with that
parser correction. During extraction and disk conversion, the 59 GiB host
filesystem filled to 100%. To protect the host, the disposable student VM was
stopped; Thot removed its linked overlay, and 26 GiB of host free space was
restored. The interrupted run retains its delivery input and journal but has
no final student report. The first completed run's `0/10` is a parser
compatibility failure, not an evaluation of the student's work.

The current project configuration and agent pin are aligned at version
`0.1.46`, protocol 2. A host with substantially more free disk is still
required before the real OVA import can complete safely. The OVA's OVF declares
a 60 GiB disk and 4 GiB RAM; the real startup and EFI/multiboot behavior,
Windows, and shared-data checks remain unqualified.

See the [platform roadmap](../planning/platform-roadmap.md),
[OVA multiboot plan](../planning/ova-multiboot.md), and
[KVM-Alpine implementation record](../implementation/kvm-alpine.md) for the
remaining work.
