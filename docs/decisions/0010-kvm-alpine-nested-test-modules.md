# ADR-0010: Nested test execution and reusable modules in KVM Alpine

- Status: Accepted (high-level architecture)
- Date: 2026-11-23

## Context

The OVA multiboot project requires disk inspection, nested operating-system
boots, and cross-OS shared-data checks. The current `kvm-alpine` smoke path
executes short shell commands and cannot run Windows or inspect/boot a student
OVA. Reimplementing test logic independently for Alpine, Debian, Docker, and
host execution would duplicate grading behavior and drift across platforms.

## Decision

The project configuration selects `kvm-alpine` and declares the outer Alpine
VM's vCPU, memory, storage, and nested-virtualization requirement. The KVM provider checks
host capability and administrator resource ceilings and creates the configured
Alpine machine when the owned base template does not exist. A professor-owned
preparation phase installs packages from configured Alpine repositories and
runs declared, project-relative instructor scripts for tools that require
another installation path. Preparation is captured and sealed as a project
baseline.

The engine selects the configured provider and supplies project settings.
For each assignment it retrieves the deliverable through the local or Google
Drive delivery agent and submits the bytes, pinned modules and ordered tests
as one provider attempt. The provider prepares or reuses its baseline, creates
the clone, stages the inputs, runs the tests and forwards progress and results.
The engine records the evidence in the student expedient and calculates grades.
The provider tears down its resources on every exit path and reports the
cleanup outcome. Only confirmed completion and cleanup release the engine to
request the next deliverable. This also applies to Docker and host providers,
which manage their own container or execution workspace.

Test functionality is packaged separately from platform lifecycle. The same
versioned module package and assessment logic can run on compatible sandbox
platforms through declared, versioned capability adapters. Platform-specific
adapters may implement process, file, disk, nested-VM, console and guest-probe
operations differently, while preserving their typed result semantics. A
missing capability fails validation/preflight; Thot never silently rewrites a
test or falls back to host execution. The package format, module ABI and
signing policy remain implementation decisions and must be resolved before
the first portable module runtime is released.

The OVA multiboot workload is not the low-resource smoke profile. The
professor chooses Alpine and nested-VM resource values. Thot validates and
records them but does not silently resize the project VM. Nested virtualization
is confined to a disposable student clone; student code must not receive host
libvirt sockets, host paths, OAuth credentials, or access to other students'
resources.

## How existing test names move into a clone

Keep the public test names in the course YAML. Resolve them against an
administrator-installed module catalogue and record the selected digests in
the run manifest. A practical first implementation is a Rust worker with a
versioned JSON request/result interface, built from the same sources for each
supported Linux target. One module bundle can carry musl and GNU executables.
The agent selects the matching build and verifies its digest before launch.

| Professor selects | Shared implementation | Adapter in the execution environment |
|---|---|---|
| `VirtualBox/import_ova` | Validate parameters and describe the imported appliance | Invoke local VBoxManage on the staged OVA. Return a clone-scoped VM handle for subsequent tests. |
| `libguestfs/disk_info` | Parse filesystem records and emit the existing disk observations | Resolve the imported disk handle inside the clone and invoke the installed libguestfs tools. |

Extract the existing disk parser and observation builder into the worker once.
Keep the legacy VirtualBox HTTP/SSH transport in its existing deployment.
In a KVM clone, dispatch directly to the local tool adapter. Docker and host
agents expose only operations allowed by their policies. All implementations
use the same test identifiers, parameter schemas and result fixtures.

Reuse the sealed baseline only while the base image, VM settings, agent,
resolved package inventory and preparation script inputs match. Changes create
a new baseline without modifying the parent of a live clone. After a timeout
or transfer failure, preserve available evidence and still destroy the clone.
Uncertain cleanup pauses the sequence before the next submission.

## Consequences

- The KVM Alpine provider needs a real base-template, project-preparation,
  snapshot, per-deliverable linked-clone and recovery lifecycle.
- `preparation.packages` is package-manager aware (`apk` on Alpine); scripts
  are trusted instructor content, hashed, bounded and run only in preparation.
- OVA test modules must be delivered into the sandbox and use typed Alpine
  platform adapters for libguestfs, VirtualBox/QEMU and guest probes.
- Google Drive large-file intake must provide immutable source identity and
  bounded streaming before it can acquire OVA submissions.
- Common module logic and schemas can be reused across platforms, but
  capabilities and isolation assurance differ. Capability absence is explicit.
- The current 128 MiB delivery cap, `exec`-only task schema and simple local
  POC lifecycle do not satisfy this decision; see the [OVA KVM Alpine gap
  plan](../planning/ova-multiboot.md).

## Alternatives considered

- Keep test execution on the host or in the legacy VirtualBox workflow:
  rejected for this target because it bypasses the requested KVM Alpine
  boundary and does not provide the new portable project workflow.
- Reimplement each test separately for every sandbox platform: rejected
  because assessment semantics would drift and professor projects would become
  platform-specific.
- Give the Alpine sandbox guest arbitrary host libvirt access: rejected
  because it would defeat the outer isolation boundary.
