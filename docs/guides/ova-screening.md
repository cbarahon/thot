# OVA multiboot screening: implementation status and test-host walkthrough

**Status: the end-to-end OVA evaluation is not supported by the published Thot
release tested here.** The walkthrough below records what an instructor can
install and configure today, where the workflow stops, and the evidence from a
real run on the test host. A created Alpine VM or an OVA inventory listing is
not proof that the student image was inspected or booted.

This guide distinguishes the published `v0.1.44` installed on the test host
from newer, unpublished workspace changes. Follow the published-release
contracts in this guide when installing from the release installer. The local
KVM-Alpine CLI is a separate path from `thotd` and the legacy VirtualBox agent.

## Current capability boundary

| Stage | Published release behavior | OVA evaluation gap |
|---|---|---|
| Local delivery | `thot deliveries list PROJECT` lists local entries; `thot sandbox run PROJECT` acquires the configured delivery. | Intake is fixed at 128 MiB total and 10 MiB per file. The submitted 8.49 GB OVA is rejected before it is transferred to Alpine. This release has no professor-configurable intake limit. |
| Alpine KVM | Creates a pinned Alpine cloud-image VM and a prepared baseline; creates per-student linked overlays after intake; starts the guest agent over VSOCK; runs `exec` and pinned `posix-shell-script-v1` tests. | The test host run did not reach student clone creation or guest-agent test execution. The local CLI does not submit progress/results to `thotd`. The trial's 8 GiB guest disk is only a bootstrap profile: it cannot hold the OVA and extracted VMDK together, and it is far too small for the declared 60 GiB virtual disk. |
| Preparation | Installs configured Alpine packages and appends configured shell-command strings. | In `v0.1.44`, `preparation.scripts` is a list of command strings, not project-relative script-file objects. Alpine 3.24.1's configured repositories do not provide `virtualbox`. Nested `/dev/kvm` access and VirtualBox are not qualified. A command that hides its own error can still let the provider mark preparation complete. |
| OVA operations | None in the KVM guest test contract. | No OVA import, partition/EFI/filesystem inventory, OS identification, GRUB entry selection, nested guest boot, or shared-data access checks are implemented in this path. |
| Results | The attempted release CLI created an empty UUID-named directory under the project results folder. | It did not write a student report, expedient, grade, or grading report. The legacy daemon report commands do not consume this local KVM run. |
| Legacy VM tests | The daemon has separate VirtualBox and libguestfs actions, operated by the VirtualBox agent. | They are not KVM-Alpine modules, are not configured by this sandbox YAML, and do not cover the full multiboot rubric. |

The planning decisions and implementation gates are in the [OVA multiboot
plan](../planning/ova-multiboot.md) and [platform
roadmap](../planning/platform-roadmap.md).

## Fresh Debian host setup

Use a Debian 13 x86_64 host with KVM enabled. The supplied OVA's OVF declares a
60 GiB virtual disk and 4 GiB RAM; the OVA file itself is about 8.49 GB. Provide
additional space and memory for the Alpine guest, OVA staging, extracted disk,
and any nested guest. The test host used below had 4 CPUs, 5.8 GiB RAM, a 59 GiB
root filesystem, and about 38 GiB free at the end of the attempt. It is not a
qualified host for running the nested multiboot workload.

Install the published release and the KVM host tools:

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
thot --help
```

The tested installer installed `thot`, `thotd`, the static Alpine guest agent,
and `thot-virtualbox-agent`; it did not install VirtualBox. The KVM command
needs root privileges and writable libvirt storage. Put `THOT_KVM_ALPINE_ROOT`
under a directory that the `libvirt-qemu` account can traverse, for example
`/var/lib/libvirt/images/thot-ova-screening`. The private project directory
under `/home/sysadmin` was not accessible to libvirt and could not be used as
the VM storage root.

The release installer is distinct from `make deploy`: `make deploy` publishes
a release and is not an installation command. The host's tested release was
`v0.1.44`; `thot-alpine-agent --version` reported `0.1.44 protocol=2`. The
installer preserved the pre-existing daemon configuration in
`/home/sysadmin/thot-preinstall-backup-20261123/`.

## Create the professor project

Create a private project directory. The OVA remains in the local delivery
folder; it does not need to be copied into the project.

```text
ova-screening-kvm-alpine/
├── configuration.thot.yaml
├── students.yaml
├── assignments.yaml
├── extern/
│   ├── preparation/
│   │   └── install-virtualbox.sh
│   └── tests/
│       └── check-vbox-and-ova.sh
└── results/
```

For the example OVA, `/home/sysadmin/ArchLinux_Dualboot.ova`, use one roster
entry and one assignment:

```yaml
# students.yaml
schema_version: thot.dev/v2
identity_field: student_id
students:
  - student_id: student1
    name: OVA screening candidate
```

```yaml
# assignments.yaml
schema_version: thot.dev/v2
assignments:
  - student_id: student1
    deliverable: ArchLinux_Dualboot.ova
```

The following is the shape accepted by the **tested `v0.1.44` release**. It
intentionally omits `delivery_limits`: that release rejects the field as
unknown. Its intake limits are fixed in the executable. `preparation.scripts`
contains shell-command strings in this release; it does not copy or execute a
script file named by a path. The separate script files below are kept as
readable instructor project material, but the tested release cannot load them
through the configuration contract.

```yaml
# configuration.thot.yaml
schema_version: thot.dev/v2
project_id: ova-screening-archlinux-dualboot
name: Arch Linux dualboot OVA screening attempt
language: en
students_file: students.yaml
assignments_file: assignments.yaml
delivery_agent:
  kind: local
  path: /home/sysadmin
sandbox_platforms:
  - id: alpine-ova-screening
    kind: kvm-alpine
    isolation_scope: kvm-guest
    lifecycle:
      template_name: thot-ova-alpine-template
      template_image:
        distribution: Alpine Linux
        release: 3.24.1
        architecture: x86_64
        image_url: https://dl-cdn.alpinelinux.org/alpine/v3.24/releases/cloud/generic_alpine-3.24.1-x86_64-bios-cloudinit-r0.qcow2
        sha512: 8d756f6fc7653daa4fb4e2e213d8a66007bcb1e5a846e28891af62c47b90685c694486c2746099ad99e9e8f5278db76b69d11dfe1e9361aa4c8406df16929a9c
      template_vm:
        vcpu: 1
        memory_mib: 1024
        disk_gib: 8
        firmware: bios
      agent:
        install_from: pinned-static-binary
        version: "0.1.44"
        listen: guest-initiated-only
      project_clone:
        name: thot-ova-alpine-prepared
        linked: true
        source_snapshot: prepared-baseline
      preparation:
        timeout_seconds: 240
        packages: [bash]
        # This release interprets each entry as shell text, not a file path.
        # The command records the package attempt and powers off the guest.
        # It is deliberately not a successful evaluation of VirtualBox setup.
        scripts:
          - "if apk add --no-progress virtualbox >/var/log/thot-virtualbox-install.log 2>&1; then touch /var/lib/thot-preparation-complete; fi; sync; poweroff"
      student_clone:
        name_prefix: thot-ova-screen
        linked: true
        reset_from: prepared-baseline
        cleanup: always
    limits:
      vcpu: 1
      memory_mib: 1024
      disk_gib: 8
      preparation_timeout_seconds: 240
      student_timeout_seconds: 30
      stdout_bytes: 4096
      stderr_bytes: 4096
    network:
      template_install: allow-configured-alpine-mirror-only
      preparation: allow-configured-alpine-mirror-only
      student_tests: deny
    inputs:
      - role: student-deliverable
        copy_to: /work/submission.ova
        read_only: true
      - role: instructor-tests
        path: extern/tests
        copy_to: /work/instructor-tests
        read_only: true
    tests:
      - id: virtualbox-and-ova-presence
        action: exec
        argv: [sh, /work/instructor-tests/check-vbox-and-ova.sh]
        timeout_seconds: 10
        expect_exit_code: 0
        capture: [stdout, stderr, exit_code, duration_ms]
    evaluation:
      syntax_check:
        tests: [virtualbox-and-ova-presence]
      functional_check:
        tests: [virtualbox-and-ova-presence]
      rubric:
        calculation: weighted_sum
        scale: 10
        criteria:
          - check: syntax_check
            points: 0
          - check: functional_check
            points: 10
      infrastructure_errors:
        grade: pending
        never_convert_to_zero: true
```

Example human-readable preparation and test scripts:

```sh
#!/bin/sh
# extern/preparation/install-virtualbox.sh
set -eu
apk add --no-progress virtualbox
command -v VBoxManage
VBoxManage --version
```

```sh
#!/bin/sh
# extern/tests/check-vbox-and-ova.sh
set -eu
test -s /work/submission.ova
command -v VBoxManage >/dev/null
printf 'OVA-present-and-VirtualBox-available\n'
```

These scripts express the intended checks. The published release configuration
shown above does not load them from those paths, and the OVA cannot pass the
release's intake limit in any case. Do not treat a configured test that never
ran as a pass or a student failure.

## Run and interpret the attempt

Use a VM storage root accessible to libvirt and retain the project results:

```sh
sudo env THOT_KVM_ALPINE_ROOT=/var/lib/libvirt/images/thot-ova-screening \
  thot sandbox run /path/to/ova-screening-kvm-alpine
```

The local-delivery inventory can be inspected with:

```sh
thot deliveries list /path/to/ova-screening-kvm-alpine
```

The release's `thot --help` only advertises `thot sandbox run`; do not rely on
newer workspace commands such as `thot sandbox report`, `export`, or `import`
unless those commands are included in the release actually installed. Likewise,
`thot run create` selects the legacy daemon/VirtualBox workflow and is not a
substitute for this KVM command.

### Test-host trial record

The professor-style trial was performed on `sysadmin@192.168.1.35` on 23 Nov
2026. The installer checksum verification succeeded; `thotd` and
`thot-virtualbox-agent` were active afterward. Host KVM prerequisites were
available, `/dev/kvm` existed, and libvirt's `default` NAT network was active.
No VirtualBox host executable was installed. The source OVA measured
8,487,809,024 bytes.

The project and retained diagnostics are at
`/home/sysadmin/ova-screening-kvm-alpine`. The VM storage and preserved
template/prepared-baseline disks are under
`/var/lib/libvirt/images/thot-ova-screening`. Run output is in the project's
`results/` folder. **Nothing was cleaned after the trial.** The guest is powered
off and no student clone is running.

Observed sequence and result:

1. The first configuration was rejected because `delivery_limits` is unknown
   to the installed release. The next shape was rejected because this release
   expects preparation script strings, not `{path: ...}` objects.
2. A project runtime under the private home directory failed at libvirt file
   access. Moving the runtime to `/var/lib/libvirt/images` allowed the Alpine
   template and project-preparation VM to start.
3. The first preparation command tried to execute the relative path
   `extern/preparation/install-virtualbox.sh` inside Alpine. Cloud-init logged
   exit code 127 because the release did not transfer that file. That failed
   baseline and the host logs were retained.
4. With the release-compatible inline command, Alpine logged:
   `virtualbox (no such package)` and `VBoxManage` was absent. The configured
   shell command masked the `apk add` failure while shutting down, so the
   provider accepted the preparation marker. The retained guest log makes this
   false-positive preparation result visible. A professor script must
   propagate failure; the release should also expose and persist each
   preparation command's status before this workflow is used for grading.
5. The engine then stopped at intake with:
   `deliverable "ArchLinux_Dualboot.ova" exceeds the 128 MiB intake limit`.
   No OVA bytes were copied to the guest. No student overlay, OVA import,
   partition inspection, GRUB boot, shared-data check, test result, expedient,
   or grade report was produced. The UUID directory under project `results/`
   is empty; the captured CLI failure and guest/libvirt diagnostics are beside
   it.

Useful preserved evidence includes `results/cli-run-attempt-06.log`,
`results/guest-virtualbox-install.log`,
`results/guest-cloud-init-output.log`, `results/guest-runcmd.sh`,
`results/libvirt-prepare.log`, and
`results/attempt-04-incomplete-baseline.qcow2`. The last file is a 517 MiB
incomplete baseline retained from the earlier timeout. The current Alpine
template and baseline remain in the KVM storage root and can be inspected with
`sudo guestfish` or `sudo qemu-img info`.

This trial verifies only that the release can download/verify the Alpine image,
create a libvirt template, boot a preparation VM, and reach the local-delivery
intake check. It does **not** verify VirtualBox installation, OVA inspection,
student test execution, progress streaming, expedient generation, or grading.
The multiboot use case should remain blocked until large-file intake/staging,
reliable preparation status, VirtualBox or an alternative OVA test module,
nested virtualization policy, test-result persistence/report integration,
and the actual EFI/GRUB/OS/shared-data checks are implemented and qualified.
