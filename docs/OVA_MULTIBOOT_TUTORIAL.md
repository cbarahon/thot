# OVA multiboot assessment: professor's tutorial

This guide describes how to assess student OVA files containing EFI, Arch
Linux, Alpine Linux, Windows, and a shared data partition. It covers Thot
installation, project organization, evaluation design, running a project, and
exporting marks.

## Read this before preparing a course

The complete requested assessment is **not supported by the current release**.
Do not use the example configuration below as if it were an executable current
configuration. Thot currently has these relevant capabilities:

| Requirement | Current support | What it proves |
|---|---|---|
| Obtain an OVA from Google Drive | Partial | `thot-delivery` can list/get Drive files for `thot sandbox run` projects. The legacy OVA/VirtualBox workflow used for this assessment still reads from a local folder; it does not use the Drive delivery agent. Download the OVA into the configured local delivery folder or use a trusted Drive sync tool. |
| Inspect virtual disk partitions and filesystems | Partial | `libguestfs/disk_info` reports devices, partitions, filesystems, sizes, labels, UUIDs, and filesystem usage for supported disk images. It does not reliably identify an EFI System Partition, Windows installation, Arch Linux root, Alpine root, or a GPT partition's type GUID. |
| Detect installed Linux root | Partial | `libguestfs/linux_rootfs_info` detects Linux root information, but it is not a multi-OS inventory or Windows detector. |
| Import and boot an OVA | Partial | `VirtualBox/import_ova` and `VirtualBox/start_vm` import and start an owned VM. A successful start proves only that the VM starts to its default boot path. |
| Prove each GRUB menu entry boots | Not supported | There is no built-in multiboot menu selection or per-entry boot verifier. |
| Prove all operating systems can read/write the shared partition | Not supported | No built-in test boots each OS and verifies a common shared-data marker. A partition merely existing does not prove that each OS can access it. |
| Grade with checks, rubrics, two evaluation items, and report exports | Supported | The legacy project engine supports checks, rubric aggregation, evaluation items, and CSV/PDF reports. |

The existing `extern/<path>` test can run a professor-provided program, but it
runs as a child of `thotd` with the daemon account's permissions. It is trusted
host code; it is **not** a safe way to run arbitrary student scripts. A
multiboot verifier would need to control VirtualBox, inspect untrusted disk
images, boot several guest operating systems, and transfer a shared test
marker. Adding a large external script would put that complex logic in the
least isolated execution path and make results harder to validate. The
recommended implementation is a small set of maintained internal tests behind
the VirtualBox agent, with strict timeouts, bounded output, explicit guest
access, and structured evidence:

1. An offline `VirtualBox/ova_layout` test: inspect the OVA manifest and all
   disks; report partition table, partition type GUIDs, labels, filesystems,
   and detected OS signatures. It must distinguish “not detected” from
   “inspection failed.”
2. A `VirtualBox/multiboot_shared_data` test: boot each configured GRUB entry
   in a fresh VM state, verify the expected OS identity, mount the expected
   shared partition, and read/write/verify a run-specific marker. It must
   record which entry was tested, the evidence from the guest, timeouts, and
   cleanup status. It must reset to a clean VM state between OS boots.

These should be internal tests because their host-side operations and
student-image parsing can be audited, bounded, and evolved with Thot's
ownership and cleanup rules. A professor script may still supply the *guest
commands* as data, but must not become an unrestricted host orchestration
program. The current project format cannot describe this workflow safely or
portably. The configuration in the “Target project configuration” section is
a proposed contract for the tests above, not something the current binary
accepts.

## 1. Prepare a fresh Linux work laptop

Use a supported x86_64 Linux installation with a desktop session or
administrator access to a VirtualBox host. Enable hardware virtualization
(Intel VT-x or AMD-V) in firmware. Keep enough free disk space for each OVA,
extracted virtual disks, and temporary VM copies. A practical starting point
is 100 GB free and 8 GB RAM; the right amount depends on the submitted VM sizes
and the number of sequential runs.

Install the current Thot release using [the deployment guide](../deploy/README.md):

```sh
curl -fLO https://github.com/cbarahon/thot/releases/latest/download/thot-install.sh
sudo bash thot-install.sh
```

Install VirtualBox from the supported package source for the host distribution
and install its matching extension/support packages only if the course image
requires them. Install the libguestfs tools used for offline disk inspection.
For Debian-based hosts, package availability and VirtualBox version depend on
the configured repositories; follow the official VirtualBox instructions for
the selected release, then install:

```sh
sudo apt update
sudo apt install libguestfs-tools
VBoxManage --version
virt-filesystems --version
```

Record the exact VirtualBox version. Thot's VM activity configuration requires
an expected version. Do not expose the local daemon or VirtualBox agent on a
public interface. Keep API and agent binds on loopback, and use a dedicated
unprivileged service account and VirtualBox profile as described in
[`THOT_VM_ACTIVITY_POC.md`](THOT_VM_ACTIVITY_POC.md) and
[`deploy/README.md`](../deploy/README.md). Imported student VMs are untrusted.
Do not enable shared folders, clipboard, drag-and-drop, USB passthrough, or
unnecessary network access. A laptop used for personal work is a poor place to
run hostile student images; use a dedicated work profile or a separate
assessment host.

The release installer creates the service and agent configuration, but it
does not install VirtualBox. Configure the host and service according to the
deployment guide before starting a run. The daemon defaults to loopback port
8081. Start/check the services with:

```sh
sudo systemctl status thotd thot-virtualbox-agent
curl --fail http://127.0.0.1:8081/health
```

The agent's configured SSH target is for the VirtualBox host integration; do
not point it at an untrusted student's guest. Never put guest passwords or
OAuth tokens in the project repository.

## 2. Make the Google Drive submissions available

Create a course folder in the professor's Drive and share it only with the
professor and the intended graders. Keep one OVA per student and choose a
stable filename that identifies the student without relying on the OVA's
internal VM name, for example:

```text
drive-course/
├── student-001.ova
├── student-002.ova
└── student-003.ova
```

For this legacy OVA workflow, download the files with the browser or use a
trusted, read-only Google Drive synchronization tool to copy them into the
local project delivery folder. Verify the expected files and available disk
space before starting. The `gdrive` delivery agent is currently connected to
the newer `thot sandbox run` project model, while OVA import and VirtualBox
tests run through the separate legacy `thot run create` service. These two
paths are not integrated yet. Do not configure `delivery_agent: gdrive` in the
legacy `configuration.yaml`; it will not make OVA runs read from Drive.

For ordinary non-OVA projects, Google Drive authorization is documented in
[`DELIVERY_AGENTS.md`](DELIVERY_AGENTS.md). That flow requests read-only Drive
access and stores the professor's token outside the project folder.

## 3. Create the course project

Use a project directory with the legacy `configuration.yaml`, roster,
assignment mapping, and local delivery folder:

```text
arch-multiboot-course/
├── configuration.yaml
├── students.csv
├── assignments.yaml
├── delivery/
│   ├── student-001.ova
│   ├── student-002.ova
│   └── student-003.ova
└── extern/
    └── tests/                 # only if using a reviewed instructor test
```

`students.csv` should include a unique `external_id`, `name`, and `email` for
each student. `assignments.yaml` maps each `student_external_id` to the exact
`delivery_filename`. A simple mapping is:

```yaml
assignments:
  - student_external_id: student-001
    delivery_filename: student-001.ova
  - student_external_id: student-002
    delivery_filename: student-002.ova
```

Use `thot assignments get` to inspect mappings and
`thot assignments import assignments.yaml` to replace them. The daemon must
be able to read the project and the local delivery folder. Paths in
`configuration.yaml` are resolved from the project directory. Use
`concurrency: 1`: VirtualBox import, shared host ports, and guest access are
not suitable for parallel student runs in the current implementation.

## 4. What can be run with the current release

The following is a **partial screening configuration**, not a complete test of
the stated requirements. It uses currently implemented test references to
check that an OVA imports, that libguestfs can enumerate disk contents, and
that VirtualBox can start the VM. The disk folder assumes one supported disk
image per imported VM. `disk_inventory` does not assert the required operating
systems or partition roles, and `vm_starts` does not select each GRUB entry.
There is intentionally no claim that this proves shared-data access.

```yaml
activity_name: Arch, Alpine, Windows multiboot (preliminary screening)
delivery_folder: ./delivery
concurrency: 1
timeout: 600
rubrics_multiplier: 10
evaluation_multiplier: 1

tests:
  - name: import_ova
    description: Import the assigned virtual appliance.
    test: VirtualBox/import_ova
    timeout: 1800
    parameters:
      - vm_name: thot_multiboot
      - vbox_group: /thot_multiboot
      - vm_path: ./vms

  - name: disk_inventory
    description: Inspect imported disk partitions and filesystems offline.
    test: libguestfs/disk_info
    requirements: [import_ova]
    timeout: 600
    parameters:
      - disk_path: ./vms

  - name: linux_rootfs
    description: Detect a Linux root filesystem, if supported by libguestfs.
    test: libguestfs/linux_rootfs_info
    requirements: [import_ova]
    timeout: 600
    parameters:
      - disk_path: ./vms

  - name: vm_starts
    description: Start the imported VM using its configured default boot path.
    test: VirtualBox/start_vm
    requirements: [import_ova]
    timeout: 240
    parameters:
      - vm_name: thot_multiboot

  - name: vm_stops
    description: Request guest shutdown after the default-path startup check.
    test: VirtualBox/stop_vm
    requirements: [vm_starts]
    timeout: 180
    parameters:
      - vm_name: thot_multiboot

checks:
  - name: appliance_imported
    description: VirtualBox imported the assigned OVA.
    check: pass
    test: import_ova

  - name: disk_inspected
    description: libguestfs returned a disk inventory.
    check: pass
    test: disk_inventory

  - name: linux_root_detected
    description: libguestfs identified a Linux root installation.
    check: pass
    test: linux_rootfs

  - name: default_path_starts
    description: The VM starts on its configured default boot path.
    check: pass
    test: vm_starts

  - name: clean_shutdown
    description: The VM stopped after its default-path startup check.
    check: pass
    test: vm_stops

rubrics:
  - name: disk_and_linux_evidence
    description: Import and basic offline disk/Linux evidence.
    checks: [appliance_imported, disk_inspected, linux_root_detected]
    calculation: avg

  - name: default_boot_evidence
    description: Default VM startup and shutdown evidence.
    checks: [default_path_starts, clean_shutdown]
    calculation: avg

evaluation:
  - name: installation_and_disk
    description: Preliminary import and disk inspection score.
    rubrics: [disk_and_linux_evidence]
    calculation: avg

  - name: default_boot
    description: Preliminary default-path startup score.
    rubrics: [default_boot_evidence]
    calculation: avg
```

This yields two evaluation items, each on a 0–10 scale. A failed check gives
zero for that check. An import, inspection, or VM infrastructure error must be
reviewed in the test result; do not treat an infrastructure error as proof of
student failure. Thot's current scoring model can calculate marks from failed
tests, so professors should inspect statuses and diagnostics before publishing
grades.

Before evaluating a class, validate the project with one known-good OVA and
one intentionally invalid OVA. Confirm the VM storage location, import
cleanup, available disk space, and that no student guest can reach host
services. The current host safety controls reduce exposure but do not make
arbitrary OVA files safe.

## 5. Target complete evaluation configuration

Once the two internal tests described above are implemented, add them to the
project and use a configuration like this. This YAML documents the intended
course contract; the test names and parameters shown here are **not implemented
in the current release**. The required `boot_profiles` must map to explicit
GRUB menu entries or stable entry identifiers agreed with students. Partition
identification should use GPT type GUID plus filesystem/label rules, not only
partition position or human-readable names.

```yaml
activity_name: Arch, Alpine, Windows multiboot assessment
delivery_folder: ./delivery
concurrency: 1
timeout: 1800
rubrics_multiplier: 10
evaluation_multiplier: 1

tests:
  - name: import_ova
    test: VirtualBox/import_ova
    timeout: 1800
    parameters:
      - vm_name: thot_multiboot
      - vbox_group: /thot_multiboot
      - vm_path: ./vms

  - name: ova_layout
    description: Validate EFI, Arch, Alpine, Windows, and shared-data partitions.
    test: VirtualBox/ova_layout
    requirements: [import_ova]
    timeout: 600
    parameters:
      - require_gpt: true
      - efi_type_guid: C12A7328-F81F-11D2-BA4B-00A0C93EC93B
      - linux_root_filesystems: ext4,btrfs
      - windows_filesystems: ntfs
      - shared_partition_label: THOT_SHARED
      - shared_filesystems: exfat,ntfs

  - name: multiboot_shared_data
    description: Boot each configured GRUB entry and verify shared-data read/write.
    test: VirtualBox/multiboot_shared_data
    requirements: [import_ova, ova_layout]
    timeout: 1800
    parameters:
      - boot_profiles: archlinux,alpine,windows
      - shared_partition_label: THOT_SHARED
      - marker_path: /THOT_SHARED/thot-course-probe.txt
      - require_read_after_write: true
      - reset_vm_state_between_profiles: true

checks:
  - name: ova_imported
    description: OVA imports successfully.
    check: pass
    test: import_ova
  - name: efi_partition
    description: An EFI System Partition with the required type is present.
    check: yaml_match
    test: ova_layout
    property: stdout
    parameters:
      - JSONPath: "$.partitions[?(@.role == 'efi' && @.present == true)]"
  - name: archlinux_partition
    description: An Arch Linux root partition is detected.
    check: yaml_match
    test: ova_layout
    property: stdout
    parameters:
      - JSONPath: "$.partitions[?(@.role == 'archlinux' && @.present == true)]"
  - name: alpine_partition
    description: An Alpine Linux root partition is detected.
    check: yaml_match
    test: ova_layout
    property: stdout
    parameters:
      - JSONPath: "$.partitions[?(@.role == 'alpine' && @.present == true)]"
  - name: windows_partition
    description: A Windows installation partition is detected.
    check: yaml_match
    test: ova_layout
    property: stdout
    parameters:
      - JSONPath: "$.partitions[?(@.role == 'windows' && @.present == true)]"
  - name: shared_partition
    description: The shared Linux/Windows data partition is detected.
    check: yaml_match
    test: ova_layout
    property: stdout
    parameters:
      - JSONPath: "$.partitions[?(@.role == 'shared' && @.present == true)]"
  - name: grub_boots_arch
    description: The Arch Linux GRUB entry boots and identifies Arch Linux.
    check: yaml_match
    test: multiboot_shared_data
    property: stdout
    parameters:
      - JSONPath: "$.boot_profiles[?(@.id == 'archlinux' && @.booted == true && @.os_verified == true)]"
  - name: grub_boots_alpine
    description: The Alpine Linux GRUB entry boots and identifies Alpine Linux.
    check: yaml_match
    test: multiboot_shared_data
    property: stdout
    parameters:
      - JSONPath: "$.boot_profiles[?(@.id == 'alpine' && @.booted == true && @.os_verified == true)]"
  - name: grub_boots_windows
    description: The Windows GRUB entry boots and identifies Windows.
    check: yaml_match
    test: multiboot_shared_data
    property: stdout
    parameters:
      - JSONPath: "$.boot_profiles[?(@.id == 'windows' && @.booted == true && @.os_verified == true)]"
  - name: arch_shared_access
    description: Arch reads back the shared marker it wrote.
    check: yaml_match
    test: multiboot_shared_data
    property: stdout
    parameters:
      - JSONPath: "$.shared_access[?(@.os == 'archlinux' && @.write_verified == true && @.read_verified == true)]"
  - name: alpine_shared_access
    description: Alpine reads back the shared marker it wrote.
    check: yaml_match
    test: multiboot_shared_data
    property: stdout
    parameters:
      - JSONPath: "$.shared_access[?(@.os == 'alpine' && @.write_verified == true && @.read_verified == true)]"
  - name: windows_shared_access
    description: Windows reads back the shared marker it wrote.
    check: yaml_match
    test: multiboot_shared_data
    property: stdout
    parameters:
      - JSONPath: "$.shared_access[?(@.os == 'windows' && @.write_verified == true && @.read_verified == true)]"

rubrics:
  - name: partition_layout
    description: Required disk and operating-system partitions.
    checks: [ova_imported, efi_partition, archlinux_partition, alpine_partition, windows_partition, shared_partition]
    calculation: avg
  - name: boot_and_shared_data
    description: Each OS boots from GRUB and accesses shared data.
    checks: [grub_boots_arch, grub_boots_alpine, grub_boots_windows, arch_shared_access, alpine_shared_access, windows_shared_access]
    calculation: avg

evaluation:
  - name: system_structure
    description: OVA and required partition layout.
    rubrics: [partition_layout]
    calculation: avg
  - name: boot_and_data_access
    description: GRUB boot paths and shared-data access from each OS.
    rubrics: [boot_and_shared_data]
    calculation: avg
```

The test must produce structured JSON/YAML on stdout with stable fields and
independent evidence for each check. A single overall exit code is not enough:
it would hide which GRUB entry or OS failed. A boot timeout, libguestfs error,
or cleanup failure must be recorded as infrastructure state and must not be
converted into a zero academic mark without professor review.

## 6. Run and review the current partial project

With the daemon and VirtualBox agent configured, change to the project folder
and check the configured local inventory:

```sh
cd "$HOME/arch-multiboot-course"
thot assignments get
thot run create "$PWD"
thot run list
thot run status RUN_ID
```

Use the run ID printed by `thot run create`. A run imports each assigned OVA,
performs the configured tests in order, captures stdout/stderr/exit codes and
diagnostics, calculates checks/rubrics/evaluation items, and removes
Thot-owned VM resources during cleanup. If import or cleanup fails, stop and
inspect the result before rerunning; do not manually delete VM files while
Thot still records ownership.

Inspect a student's expedient and the class matrices:

```sh
thot run expedient EXPEDIENT_ID
thot report expedients RUN_ID
thot report test_stats RUN_ID
thot report checks_stats RUN_ID
thot report rubric_stats RUN_ID
```

For marks suitable for spreadsheets, export the evaluation items as CSV:

```sh
thot report grades --evaluation-items --csv RUN_ID > "$PWD/grades.csv"
```

To create the professor summary PDF, individual student reports, and a
Moodle-ready CSV in one step:

```sh
thot report grades --all RUN_ID --output-dir "$PWD/grades/RUN_ID"
```

Review failed, timed-out, skipped, and error statuses before releasing grades.
Use the student PDFs for per-student evidence and `professor_grades.pdf` for
the cohort overview. Keep the OVA source, project configuration, and exported
reports together when archiving the course. The reports include test
observations and scoring inputs; retain them according to the institution's
student-data policy.

## 7. Closure checklist for full support

The end-to-end scenario is ready for a course only after the following work is
implemented and tested on the professor's target host:

- Integrate the read-only Google Drive delivery agent with legacy OVA runs, or
  document a supported Drive sync/import command that verifies filenames,
  sizes, and SHA-256 before a run.
- Add bounded OVA manifest/disk extraction and reliable EFI/GPT/OS/shared
  partition classification, including multiple-disk appliances.
- Add a deterministic way to select each student GRUB menu entry, boot the
  corresponding OS, and prove OS identity.
- Prove shared-data write/read across independent clean boots of Arch, Alpine,
  and Windows; define filesystem choice, mount rules, marker location, and
  whether Windows fast startup/hibernation is rejected.
- Make each boot attempt recoverable and ensure VM snapshot/reset/cleanup after
  power failure, timeout, malformed OVA, and agent disconnect.
- Verify all guest networking is isolated from the laptop and that results
  preserve per-OS stdout, stderr, exit status, timeout, platform errors,
  cleanup status, and evidence hashes.
- Run a known-good and deliberately broken fixture on the exact supported
  VirtualBox, libguestfs, and Linux versions; then run a small professor pilot
  before grading a full class.

Until those checks pass, the partial configuration above is suitable only for
basic OVA import, disk inventory, and default-path startup screening. It must
not be described as verifying the complete multiboot/shared-data assignment.

## Related documentation

- [Implemented functionality catalog](IMPLEMENTED_CATALOG.md)
- [Legacy VM activity POC and host safety](THOT_VM_ACTIVITY_POC.md)
- [Delivery agents and Google Drive OAuth](DELIVERY_AGENTS.md)
- [Deployment guide](../deploy/README.md)
- [KVM Alpine sandbox status](KVM_ALPINE_POC.md)
