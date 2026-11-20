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