# Implementation plan

Status: Proposed planning, no code changes in this release. Each phase requires acceptance before enabling the next; No schedule is set without measuring the nesting prototype. Dependencies: P0 → P1 → P2 → P3 → P4 → P5 → P6 → P7. P1 and P6 reporting investigation may move forward while hardware is certified; no hostile integration is enabled before P2.

| Phase | Orderly work | Dependency/testable output |
|---|---|---|
| P0 — Viability and threats | Inventory current code; prototype configured Alpine outer VM with nested KVM, project baseline and per-student clones; boot nested Linux and Windows guests; test Alpine package/script preparation, test module transfer, VSOCK, network and watchdog; measure CPU/RAM/disk/boot on target laptop class | Record host/kernel/libvirt/QEMU/Alpine versions and resource measurements; confirm professor-selected outer/inner resource fields and nested capability. No hostile OVA use before boundary validation; reject, never fallback, if nested support is unavailable |
| P1 — Contracts and persistence | Publish shared project/provider/preparation schema, module package ID/version/digest/capability contract, Drive selector/provenance, typed OVA tests, valid/invalid fixtures, results/artifacts and evaluation model; folder repository, lock, journal and revision commit | Round-trip project/run package; compatible module fixture shared by two agents; reject missing capability and invalid resource values before VM creation; recover and reject collision/concurrent write |
| P2 — Secure providers | Implement KVM Alpine template and prepared baseline lifecycle, nested KVM exposure inside disposable Alpine clone, professor resource settings with host ceilings, child VM restrictions, Docker provider and host adapter allowlist; outer quotas/network policy/leases/cleanup | Verify each platform boundary independently; prove nested VM only exists within assigned Alpine clone, and host/other students remain unreachable; never downgrade KVM selection to Docker or host execution |
| P3 — Preparation, test modules and agents | Alpine `apk` package installation plus hashed professor scripts; Debian/Docker equivalents; transferable versioned test packages and capability adapters; agent protocol with ACK/blobs, sequential tests, timeouts/cancellation; Git/Drive acquisition without secrets in guest | Preparation failure blocks only that platform; required package/tool inventory recorded; module digest/capabilities verified; disconnect does not duplicate tasks; agent has no grade authority |
| P4 — Independent platform workflow | Platform IDs in test DAG, KVM linked clone per student/group with nested tasks, Docker container per student/group, read-only host actions, platform-local resource registry and result scoping | RF-21–25; reject cross-platform dependencies and live references; prove each student's writable state is fresh and removed, including all nested VMs and images |
| P5 — Evaluation and editing | Adapt current checks/rubrics to v2 results, statistical barrier, N×N files, non-delivery, revisions, corrections and expansion with state reconstruction | RF-12–15; same notes on POC fixtures when semantics are compatible; technical failure leaves pending; correction does not lose calculated value |
| P6 — Teaching experience | Templates, explanatory validation, summary and offline HTML, Unicode PDF, configurable CSV, private import/export, backup/retention and installation guide | RF-16/18/20; pilot with teacher without help from hypervisor commands; names with correct accents and symbols in PDF and copy/search |
| P7 — Migration and publishing | SQLite/POC files/files importer, evidence and notes verification, report comparison, rollback and installation packages; document gradual withdrawal of legacy | RF-19; restore backup and reopen migrated project without original DB; pilot approval and all safety gates |

## OVA multiboot use case on KVM Alpine

The generic phases above do not by themselves define the OVA/Google Drive
workflow. Use the dedicated [OVA multiboot KVM Alpine gap plan](ova-multiboot.md)
for its architecture decisions, schema extensions, Drive large-file transfer,
safe OVA inspection, nested VM topology, per-OS GRUB boot, cross-OS shared-data
verification, grading and acceptance matrix. That plan refines the generic P0
through P7 sequence and is a release gate for claiming support of this use
case.

## Minimum validation matrix

Future unit tests will cover validation, selectors/cardinality, loops, footprinting, evaluation, and reviews. The contract ones will share fixtures between engine and agents and will check for unknown messages, incorrect hash, repeated/different payload, late replay and truncation. Integrations will use real Linux/KVM and synthetic repositories without personal data.

The security acceptance must attempt to: read/delete a decoy file from the host; access host Docker/libvirt, LAN, metadata and other lab over IPv4/IPv6; escape via links/traversal; exhaust RAM, processes, disk and inodes; send excessive logs/blobs and malicious JSON/YAML; compromise a container and verify its stated boundary. Separately test KVM snapshot/linked-clone parent integrity, concurrent clone isolation and cleanup after forced termination; verify Docker host-kernel scope is clearly reported and host actions cannot execute project scripts. Acquisition should test redirects/DNS to blocked destinations and malicious Git. Check firewall from inside each guest/container even with root, not just by inspecting configuration.

Inject engine/supervisor/agent crash, host reboot, project disk full, and abort during L2 creation. Verify that an incomplete final grade is not published, an uncertain operation is not automatically repeated, partial evidence remains, and pending cleaning is visible. Test recovery at the point before and after each revision commit.

The teaching test uses a correct activity, another that is academically incorrect and another with a technical error: the teacher must distinguish them, consult evidence, correct a grade and export. Check group delivery, multiple resources per student, late delivery, and new test that requires already destroyed VM.

## Lossless migration

1. Freeze and backup POC DB, projects and artifacts; register a specific version, without attributing a VM validation to a build.
2. Import to new folder, preserve original IDs as metadata and map routes. Never modify/delete origin automatically.
3. Convert configuration/students/associations and results through explicit mapping. A legacy `passed` state retains its semantics; No observations or guarantees of isolation are invented.
4. Compare students, associations, counts, hashes, notes and links. Mark missing artifacts and non-reproducible entries.
5. Offer preview of v2 and correct incompatibilities. An OVA or VBox script is not automatically converted to KVM; will require new image/activity or will remain in explicit legacy deployment.
6. Change the active project only after validation. Rollback returns to the previous backup/installation; does not execute a destructive downgrade.

## Later work and absorbed elements

This plan replaces `ROADMAP.md` and `TODO.md`: concentrates tasks to avoid conflicting priorities. Pending PDF font research should compare Rust libraries, Unicode embedding, licensing, size, search/copy, and integration cost before replacing the generator.

After P7, and only by request: guided web interface; Moodle synchronization and LTI 1.3 evaluation; multi-user authentication/roles/audit; remote provider with mTLS, distributed scheduler, object storage and quotas per organization; other hypervisors; reviewed/signed test packages and catalogue; analysis between cohorts with privacy. Hosting, marketplace and AI help are exploratory, not commitments. AI would require opt-in and teacher review, no hidden automatic grading.
