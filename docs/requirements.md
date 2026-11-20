# Thot platform requirements

Status: proposed architecture, 2026-11-15. None of the new capabilities described here are implemented. Target contracts use `thot.dev/v2`. The Spanish source is preserved in [es/requirements.md](es/requirements.md).

## Purpose and users

An instructor prepares an activity, imports submissions, runs an assessment, and understands or corrects results without administering hypervisors. An administrator installs and maintains Linux, images, and policies. Students provide untrusted code, repositories, disks, or documents. Agents are deterministic execution and observation programs; AI is not assumed.

Thot preserves the workflow: submission → tests and evidence → checks → rubrics → assessment → expedients and reports. The engine determines grades. A platform failure must never silently become a zero.

## Functional requirements

| ID | Requirement | Acceptance criterion |
|---|---|---|
| FR-01 | Portable project containing configuration, roster, assignments, submissions, and results | Export can be opened elsewhere and reports recalculated without the original database or Internet |
| FR-02 | Versioned descriptive YAML and preflight validation | Errors identify file, field, cause, and remedy; invalid input executes no code |
| FR-03 | Import CSV/YAML roster with configurable identity field | Empty and duplicate IDs are detected; YAML takes precedence with a warning |
| FR-04 | Many-to-many assignment and typed multi-file submissions | Group work and multiple inputs work; wrong selector cardinality fails before execution |
| FR-05 | Acquire HTTPS Git repositories and local files | Resolved commit and hashes are recorded; submodules/LFS require explicit policy and quotas |
| FR-06 | Shared laboratory for all steps in an assessment unit | One step creates services/VMs and later agents inspect them by stable resource IDs |
| FR-07 | Declarative Git, file, process, Docker, KVM, and SSH actions | Capture stdout, stderr, exit code, timing, observations, and artifacts |
| FR-08 | Explicit dependencies and unconditional cleanup | Cycles are rejected; dependents are blocked by policy; cleanup runs on failure, timeout, and cancellation |
| FR-09 | Isolate submissions from host and one another | Student code never runs on the engine host; isolation acceptance tests pass before release |
| FR-10 | Effective step, laboratory, and run quotas | Resource exhaustion is contained and diagnosed |
| FR-11 | Normalized, verifiable results | Agents cannot write expedients or grades; engine validates schemas, sizes, identity, and hashes |
| FR-12 | Regrade from saved evidence | Rule changes create a revision without rerunning student code |
| FR-13 | Extend a run | Reuse only compatible results; rebuild dependencies when live state is needed |
| FR-14 | Understandable manual corrections | Validated YAML corrections record author and reason and distinguish calculated from corrected grades |
| FR-15 | Expedient for every student | Missing work is distinct from platform failure; group evidence may be shared while grades remain individual |
| FR-16 | Export reports | Offline HTML, Unicode PDF, and configurable CSV explain evidence-to-grade calculation |
| FR-17 | Recovery, cancellation, and cleanup | Uncertain side effects are not blindly repeated; leftovers remain identifiable |
| FR-18 | Simple instructor workflow | Validate, run, review, correct, and export are straightforward operations |
| FR-19 | Import the POC | Convert database/files without deleting the source; report incompatibilities |
| FR-20 | Privacy-aware import/export | Complete and shareable modes; shareable mode omits personal data and secrets by default |

## Non-functional requirements

| ID | Requirement |
|---|---|
| NFR-01 | A KVM VM is mandatory as the outer boundary; failed preflight blocks execution without native fallback |
| NFR-02 | One writer per project, atomic writes, recoverable journal; incomplete results are never final |
| NFR-03 | Open UTF-8 formats and relative project paths; secrets and temporary disks stay outside exports |
| NFR-04 | Record versions, hashes, inputs, revisions, corrections, and evidence; hashes detect changes but do not prove authenticity |
| NFR-05 | Initial targets: 30-second lease, 10-second heartbeat, 10-second graceful cancellation, forced stop within 30 seconds |
| NFR-06 | Admit work according to CPU, RAM, and disk reservations; do not overcommit laboratory memory |
| NFR-07 | Keep the engine independent of KVM, Docker, SSH, and LMS through versioned contracts |
| NFR-08 | A pilot instructor can complete the workflow without editing hypervisor configuration |
| NFR-09 | Private permissions by default, external credential management, configurable retention, explicit exports |
| NFR-10 | Negotiate versions/capabilities and reject unknown major versions or unsupported features before execution |
| NFR-11 | Offline execution after inputs and images are acquired; network denied by default |

## Scope

Initial target: maintained Linux x86_64, KVM/QEMU and libvirt for the outer VM, Docker inside it, and nested virtualization for activities that create VMs. No initial macOS, Windows, Kubernetes, distributed execution, multi-instructor concurrency, or direct execution of student code in host Docker. VirtualBox remains a migration-era POC.

The design aims to contain hostile code and mistakes; it cannot guarantee protection against all kernel or hypervisor vulnerabilities.
