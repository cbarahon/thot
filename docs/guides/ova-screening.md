# Screen an OVA with the VirtualBox daemon workflow

This guide describes how to assess student OVA files containing EFI, Arch
Linux, Alpine Linux, Windows, and a shared data partition. It covers Thot
installation, project organization, evaluation design, running a project, and
exporting marks.

For the requested future implementation on the new KVM Alpine architecture,
see the [dedicated gap analysis and delivery plan](../planning/ova-multiboot.md).
This tutorial retains the current legacy VirtualBox workflow analysis and a
partial screening example; it is not the target KVM architecture contract.

## Read this before preparing a course

The complete requested assessment is **not supported by the current release**.
The example runs preliminary checks. Thot currently has these relevant capabilities:

| Requirement | Current support | What it proves |
|---|---|---|
| Obtain an OVA from Google Drive | Partial | `thot-delivery` can list/get Drive files for `thot sandbox run` projects. The legacy OVA/VirtualBox workflow used for this assessment still reads from a local folder; it does not use the Drive delivery agent. Download the OVA into the configured local delivery folder or use a trusted Drive sync tool. |
| Inspect virtual disk partitions and filesystems | Partial | `libguestfs/disk_info` reports devices, partitions, filesystems, sizes, labels, UUIDs, and filesystem usage for supported disk images. It does not reliably identify an EFI System Partition, Windows installation, Arch Linux root, Alpine root, or a GPT partition's type GUID. |
| Detect installed Linux root | Partial | `libguestfs/linux_rootfs_info` detects Linux root information, but it is not a multi-OS inventory or Windows detector. |
| Import and boot an OVA | Partial | `VirtualBox/import_ova` and `VirtualBox/start_vm` import and start an owned VM. A successful start proves only that the VM starts to its default boot path. |
| Prove each GRUB menu entry boots | Not supported | There is no built-in multiboot menu selection or per-entry boot verifier. |
| Prove all operating systems can read/write the shared partition | Not supported | No built-in test boots each OS and verifies a common shared-data marker. A partition merely existing does not prove that each OS can access it. |
| Grade with checks, rubrics, two evaluation items, and report exports | Supported | The legacy project engine supports checks, rubric aggregation, evaluation items, and CSV/PDF reports. |

## 1. Prepare a fresh Linux work laptop

Use a supported x86_64 Linux installation with a desktop session or
administrator access to a VirtualBox host. Enable hardware virtualization
(Intel VT-x or AMD-V) in firmware. Keep enough free disk space for each OVA,
extracted virtual disks, and temporary VM copies. A practical starting point
is 100 GB free and 8 GB RAM; the right amount depends on the submitted VM sizes
and the number of sequential runs.

Install the current Thot release using [the deployment guide](../../deploy/README.md):

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
[VM activity reference](../implementation/vm-activity-reference.md) and
[`deploy/README.md`](../../deploy/README.md). Imported student VMs are untrusted.
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
[delivery agent guide](delivery-agents.md). That flow requests read-only Drive
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

## 5. Run and review the current partial project

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

## Related documentation

- [Implemented functionality catalog](../implementation/vm-activity-reference.md)
- [Delivery agents and Google Drive OAuth](delivery-agents.md)
- [Deployment guide](../../deploy/README.md)
- [KVM Alpine sandbox status](../implementation/kvm-alpine.md)
