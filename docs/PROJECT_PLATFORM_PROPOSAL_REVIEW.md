# Review of the platform proposal

Status: Engineering Review, 2026-11-15. The proposal is specified in [requirements](requirements.md), [architecture](architecture.md), [model](data-model.md), [interfaces](api-spec.md), [plan](implementation-plan.md) and [ADR](decisions/README.md). The decisions are **proposed**, pending prototype and acceptance; This documentation does not install or verify a sandbox.

## Observed and target state

The root README underdescribed the POC by excluding associations and notes; The POC catalog and guide already document them. There is external execution with daemon permissions and SQLite/files status. The workspace also contains previous VirtualBox separation work into `thot-virtualbox-agent`, HTTP JSON v1 tasks, snapshots, and `runs/` folders; Their READMEs describe SSH to a VirtualBox host and shared artifact paths. This provides contracts and separation of responsibilities, but does not isolate the general execution of code from the student. Some legacy documentation retains the previous model: it is kept as a POC reference, with a caveat, until a specific published version is verified.

A local generic engine and a disposable KVM VM are proposed per evaluation unit, with agents, Docker and possible VMs nested inside. All steps in a unit share state; different units are isolated. The project continues to be the teacher's unit of work, archiving, editing and exchange.

## Review of previous agreements

| Theme | Proposed resolution and reason |
|---|---|
| Name | Keep `configuration.thot.yaml`; legacy alias with notice and coexistence rejection |
| Students | Keep YAML priority over CSV with warning and configurable ID field; Independent internal UUID |
| N×N | Maintain groups and various deliverables; make explicit joint unit, role, type and cardinality |
| Routes | Keep originals and names sanitized with suffixes; references by stable ID, not by sanitized name |
| Timestamp | Keep readable date of seconds, but exclusive creation and collision treatment are mandatory even if there is only one teacher |
| Run modifiable | Keep editing and expansion within the same folder; add small revisions to explain which configuration caused each result |
| Skip existing results | Replace the “result exists” rule with fingerprint compatibility; avoid attributing old observations to modified code or tests |
| Shared status | Preserving results does not keep VMs alive; rebuild dependencies for new steps that need them |
| Tests inside the engine | Only pure and limited parsing/evaluation; every action on code, network, disks or processes goes to the laboratory |
| VirtualBox Host Agent | POC transition, no objective border or dependence on the new Linux KVM platform |
| Docker | Within L1 for tools/services; no host Docker nor teacher socket |
| Database | Reconstructable index as objective; do not remove SQLite before migrating and test all state |

Revisions do not turn the project into an immutable repository or force the teacher to manage versions. They allow values ​​to be edited with a verifiable explanation of their effect. If overwriting without any provenance is required, the loss of traceability must be explicitly accepted; It is not the engineering recommendation.

## Difficulties and open decisions

### Sandbox platform questions

The proposed `kvm`, `docker`, and `host` choices are execution boundaries with different assurances, not interchangeable deployment labels. A platform ID must scope tests, inputs, results, resource handles, network policy, and cleanup. The engine can aggregate completed results after platform execution; allowing a test to read another platform's live state would create an implicit bridge and is outside the initial model.

| Question | Why it is difficult | Current recommendation / decision needed |
|---|---|---|
| Is host execution really safe? | Read-only analysis still invokes parsers on hostile files; malformed PDFs/archives can exploit parser bugs or exhaust CPU, memory, and disk. “No student script” is not a complete safety test. | Start with a small allowlist of maintained parsers, strict limits, copied/read-only inputs and no project-defined executables. Decide whether host mode is excluded by default and enabled by administrator policy. |
| Where does Docker run? | Host Docker shares the host kernel and access to the Docker socket can be equivalent to host administration. Docker inside a KVM guest has the VM's outer boundary but costs KVM resources. | Make `isolation_scope: host-kernel|inside-kvm` explicit. Do not call host Docker equivalent to KVM. Decide which scope is supported in phase one. |
| Is Debian 13 a reproducible baseline? | A release name alone does not pin package versions, repositories, updates, kernel, or image contents. `apt` can change over time and needs network access. | Pin VM/image digest, repository snapshot and package versions where possible; record actual inventory and preparation outputs. Decide update cadence and whether preparation is offline. |
| When is the KVM snapshot valid? | A running guest may have pending package writes or services; memory snapshots can preserve secrets or transient state. | Complete preparation, flush filesystems, shut down, snapshot disks only, verify snapshot identity. Decide if suspend/memory snapshots are ever needed. |
| Are linked clones portable and recoverable? | A linked clone depends on its backing image/snapshot and storage format. Parent deletion or mutation corrupts every child; copying only the clone cannot restore it. | Protect parent chain read-only and hash it; export parent chain or label artifacts non-replayable. Choose qcow2/storage semantics and retention policy. |
| What does the guest agent prove? | The agent shares the guest with student code. A student obtaining root may alter it, forge output, or kill reporting. | Treat returned observations as untrusted evidence with transport integrity, not hardware attestation. Decide when a separate observer or second VM is needed. |
| What is the network boundary? | Preparation needs package sources and Git may need private credentials; tests may intentionally configure networks. Egress can leak student data or tokens. | Deny by default, separate acquisition/preparation/test network phases, proxy and scope credentials, block host/LAN/metadata by default. Decide allowed destinations and offline mirror. |
| What is the unit of isolation? | Group work may share a submission; separate students must not inherit each other's process or disk state. | Create one clean environment per student or explicitly configured group with stable unit ID. Decide how late group membership changes trigger reruns. |
| How should resource limits compose? | VM, container, nested VMs, package builds, and artifact capture consume the same host resources. Setting limits only inside the guest does not protect the host. | Enforce outer CPU/RAM/disk/process quotas at host level and inner quotas as additional controls. Decide default caps, admission policy, and behavior under host pressure. |
| How do failures affect grades? | Failed boot, package mirror outage, guest-agent loss, and a student's non-zero exit code have different meanings. | Keep platform diagnostics separate from academic checks; infrastructure failure yields incomplete/pending evaluation until retry or instructor override. Decide retry limits and whether remaining tests run. |
| When is cleanup complete? | A timed-out process can leave descendants, linked overlays, containers, or libvirt domains behind. | Cleanup is a provider operation independent of instructor `finally` steps; verify resource removal, quarantine uncertain leftovers, and block unsafe reuse. Decide operator escalation and retention. |

See [ADR-0007](decisions/0007-sandbox-platforms.md) for the proposed platform contract and validation gates.

| ID/priority | Difficulty or decision | Proposal / alternatives | Evidence of closure and phase |
|---|---|---|---|
| O-01 Blocker | Nesting varies by CPU, firmware and VM vendor | KVM L1/L2 as initial profile; require certified equipment or limit activity | Boot two L2, Docker and SSH with quotas and measure degradation; P0 |
| O-02 Blocker | Actual minimum supervisor/libvirt privileges | Dedicated service with allowlist operations; choose specific connection/polkit/LSM | Demonstrate that the engine/guest user does not control foreign domains or inject devices; P0/P2 |
| O-03 Blocker | Agents can share trust with the student's root in L1 | Ordinary profile separates containers; nested reports reduced confidence; possible observer in another future VM | Define what academic claims are accepted under hostile root and test falsification; P0/P4 |
| O-04 Blocker | Git egress/dependencies with TLS, DNS and IPv6 | Proxy outside L1, allowlist and read tokens; offline after acquisition | SSRF testing, redirection, LFS/submodules and no secrets; P2/P3 |
| O-05 High | What images and tools to keep | Image by digest, reproducible manifest, updates and rollback | Choose distribution/versions, responsible and patch policy; P0/P2 |
| O-06 High | Reusable Results vs Live Status | Traces and reconstruction; VM snapshots initially excluded | Show plan before expanding and actual cost of reconstruction; P4/P5 |
| O-07 High | Semantics of errors and fairness of notes | Pending due to infrastructure, zero only due to explicit academic rule | Professor validates reports with borderline cases and lack of delivery; P5/P6 |
| O-08 High | Quotas and performance in teaching teams | One initial active unit, admission by reservation; subsequent attendance measure | Measure corpus of activities, set maximums and minimum published hardware; P0/P4 |
| O-09 High | Integrity vs. manual editing | YAML fixes and hotfixes; hashes detect accidents, without cryptographic attestation | Try modifying result/configuration and recover without falsifying origin; P1/P5 |
| O-10 High | Storage of tickets and large discs | Full export by default; explicit and non-reproducible light | Measure sizes, retention and copy time; decide safe deduplication; P1/P6 |
| O-11 High | VirtualBox/OVA Activity Compatibility | Separate legacy; teacher migration to KVM, without promise of conversion | Matrix by activity and image/boot limitations; P7 |
| O-12 High | Folder permissions and persistence failures | Explicit user/ACL, a local writer, journal, and atomic commit | Disk full/power outage/reindexing and export test; P1/P2 |
| O-13 High | Reliable L2 SSH Boot Created by Student | Pinned resource record obtained by trusted provisioning channel | Resolve activities that do not deliver keys or readiness; without disabling global verification; P4 |
| O-14 Medium |Accessible interface and complex YAML | Guided templates and validation; Initial CLI, later web | Pilot decides whether CLI is sufficient for first delivery or guided interface is gate; P6 |
| O-15 Medium | Unicode and LMS Reports | Licensed embedded fonts and configurable column export | Comparison of libraries and real Moodle import test; P6 |
| O-16 Future | Remote Server | Maintain contracts, add mTLS, tenancy and storage | New ADR and threats before opening remote access; after P7 |

## Conditions to continue

Do not announce “safe to execute any delivery” or eliminate the existing deployment simply by writing the design. P0 must resolve O-01–03 and set the verifiable scope; P2 must demonstrate exterior isolation before testing hostile code. Final validation requires real Linux/KVM hardware and a pilot teacher. Unit testing and compilation are no substitute for that evidence.
