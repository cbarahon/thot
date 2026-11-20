# Thot platform requirements

Status: architecture proposal, 2026-11-15. Any new capabilities described here are implemented by this document. Target contracts are called v2 to distinguish them from POC; its first revision is `thot.dev/v2`. See [architecture](architecture.md), [model](data-model.md), [interfaces](api-spec.md) and [plan](implementation-plan.md).

## Purpose and actors

The teacher prepares an activity, incorporates deliveries, executes an evaluation and understands or corrects its results without managing hypervisors. An administrator installs and maintains Linux, images, and policies; It may be the same person, but daily operation should not require that knowledge. The student contributes code, repositories, disks or documents that are considered unreliable. Agents are deterministic execution and observation programs; no AI model is assumed.

The delivery methodology → tests/evidence → checks → rubrics → evaluation → file/reports is preserved. The engine decides the rating. An action can produce a valid observation even if it finds an academic defect; a platform breakdown should never silently become a zero.

## Functional requirements

| ID | Verifiable requirement | Acceptance criteria |
|---|---|---|
| RF-01 | Portable project with YAML configuration, students, associations, deliveries and results | Copying a complete export to another installation allows you to open reports and regrade without the original database or Internet connection |
| RF-02 | Versioned declarative configuration, commented templates and pre-validation | Errors indicate file, line/field, cause and correction; no invalid entry starts code |
| RF-03 | Import CSV/YAML students with configurable identifier field | Detect empty/duplicate identifiers; YAML takes precedence with warning when CSV coexists |
| RF-04 | N×N associations and deliveries with various typed resources | OVA + PDF groups and deliveries work without duplicate execution; selector with wrong cardinality is rejected before executing |
| RF-05 | Acquire Git HTTPS repositories and local files | Record resolved commit, hashes and portable copy; submodules and LFS require explicit enablement and quotas |
| RF-06 | Common laboratory for all steps of an evaluation unit | One step creates services/VMs and another agent inspects them for stable identifiers in the same lab |
| RF-07 | Git actions, files, processes, Docker, KVM and SSH | Cover the complete scenario of [interfaces](api-spec.md); capture stdout, stderr, code, signal, times and observations |
| RF-08 | Declarative dependencies and unconditional cleanliness | Reject cycles; block dependents according to policy; clean also when canceling, running out of time or losing agent |
| RF-09 | Isolate deliveries from the host and from each other | Student code never runs on the engine/host; the isolation tests of the plan are a condition of publication |
| RF-10 | Effective limits per step, laboratory and run | A loop, fork bomb, or disk full stops without depleting the host pool and leaves a diagnostic |
| RF-11 | Normalized results and verifiable evidence | An agent cannot write files or notes; engine validates schema, size, identity and hashes |
| RF-12 | Requalify without rerun | Changing checks/rubrics produces a revision linked to preserved results |
| RF-13 | Expand a run | Incorporate deliverables/tests reuses only compatible results; new steps that need lost state rebuild dependencies |
| RF-14 | Comprehensible manual intervention | Edit YAML of fixes with author/reason and validate produces a revision; the report distinguishes calculated and corrected grade |
| RF-15 | Records for all students | Show absence of delivery separate from technical error; groups share evidence and keep individual notes |
| RF-16 | Reports and export | Local HTML, Unicode PDF and CSV configurable for teaching platform, with test explanation → grade and without secrets |
| RF-17 | Recovery, cancellation and cleaning | After restarting, an action is not automatically repeated withuncertain effects; waste is identified and reclaimable |
| RF-18 | Simple interface | Validate, run, review, correct, and export are single operations; summary in Spanish with link to technical detail |
| RF-19 | POC Import | Transform files/SQLite without deleting origin, preserving evidence and warning of VirtualBox incompatibilities |
| RF-20 | Import/export with privacy | Full and shareable modes; the second omits students, notes, secrets and deliveries by default |

## Non-functional requirements

| ID | Requirement and initial objective |
|---|---|
| RNF-01 Security | Mandatory KVM as outer border in first release; preflight failure blocks, without degradation to native process |
| RNF-02 Consistency | A single writer per project, atomic writes and recoverable journal; no partial result is presented as final |
| RNF-03 Portability | Relative references under the project and open UTF-8 formats; secrets, sockets and temporary disks outside the export |
| RNF-04 Traceability | Referenced versions, digests, entries, configuration, revisions, corrections and evidence; hashes detect changes, do not certify authenticity |
| RNF-05 Availability | Initial lease of 30 s, heartbeat of 10 s, graceful cancellation of 10 s and forced shutdown before 30 s; check these targets under load |
| RNF-06 Performance | Reserved RAM/CPU/disk support; zero lab RAM overcommitment; measure startup and delivery time before setting an SLA |
| RNF-07 Maintainability | Independent KVM, Docker, SSH and LMS engine using versioned contracts and compliance testing |
| RNF-08 Usability | A pilot teacher completes a template and evaluation cycle without editing libvirt XML, networking, or systemd units |
| RNF-09 Privacy | Default private permissions, out-of-project credentials, configurable deletion/retention, and explicit exports |
| RNF-10 Compatibility | Version/capabilities negotiation; reject unknown major version, unknown keys and unavailable features before running |
| RNF-11 Off-grid operation | After downloading images/dependencies and acquiring repositories, execution and offline query with network denied by default |

## Scope and exclusions

First platform: Linux x86_64, KVM/QEMU and libvirt for external VM; Docker inside it. Nested virtualization for activities that create KVM VMs. No initial support for macOS, Windows, Kubernetes, distributed execution, multiple concurrent teachers, or direct student code execution in host Docker. VirtualBox is preserved in the POC during the migration, with no promise of automatic support.

The protection seeks to contain hostile code and accidental errors; There is no absolute guarantee against kernel or hypervisor vulnerabilities. The installation must be kept up to date and activities that require additional privileges must pass specific acceptance. Evidence generated on a machine controlled by hostile code is not promised to be incorruptible.