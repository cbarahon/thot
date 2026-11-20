# Implementation plan

Status: Proposed planning, no code changes in this release. Each phase requires acceptance before enabling the next; No schedule is set without measuring the nesting prototype. Dependencies: P0 → P1 → P2 → P3 → P4 → P5 → P6 → P7. P1 and P6 reporting investigation may move forward while hardware is certified; no hostile integration is enabled before P2.

| Phase | Orderly work | Dependency/testable output |
|---|---|---|
| P0 — Viability and threats | Inventory current changes and actual tests; isolated prototype KVM L1 → Docker and L2; measure CPU/RAM/boot; test libvirt, vsock, network and watchdog permissions | Report reproducible on dedicated host with CPU, distribution, kernel, QEMU/libvirt and Docker; nesting go/no-go decision. If it fails, trim scope, or require other hardware, never run on host |
| P1 — Contracts and persistence | Publish formal schemas and valid/invalid YAML fixtures, tasks, results and packages; fix canonicalization; folder repository, lock, journal and revision commit; reindexing without SQLite | RF-01–05, 11–14; round-trip export/import, move folder, recover after cut and reject collision/concurrent write |
| P2 — Secure provider | Reproducible base image and digest, minimum supervisor, Unix/vsock identity, external quotas, network denied, leases and manifest cleanup; installer and diagnosis | RF-09/10/17; verified isolation matrix on real Linux; no native fallback path |
| P3 — Agents and results | Coordinator, protocol with ACK and blobs, filesystem/exec/Git; Git proxy without secrets in guest; limited capture of streams, timeouts/cancellation; uncertain state and idempotence | RF-05/07/11; disconnect during submission and execution without duplicating task; invalid artifact does not exit staging; private repository does not leak token |
| P4 — Interactive laboratory | Resource logging, DAG, locks, Docker local L1, nested KVM, discovery, and pinned SSH; ordinary/nested profiles | RF-06–08; run scenario Git → make → two L2 → properties → SSH; two simultaneous units without shared ports or data |
| P5 — Evaluation and editing | Adapt current checks/rubrics to v2 results, statistical barrier, N×N files, non-delivery, revisions, corrections and expansion with state reconstruction | RF-12–15; same notes on POC fixtures when semantics are compatible; technical failure leaves pending; correction does not lose calculated value |
| P6 — Teaching experience | Templates, explanatory validation, summary and offline HTML, Unicode PDF, configurable CSV, private import/export, backup/retention and installation guide | RF-16/18/20; pilot with teacher without help from hypervisor commands; names with correct accents and symbols in PDF and copy/search |
| P7 — Migration and publishing | SQLite/POC files/files importer, evidence and notes verification, report comparison, rollback and installation packages; document gradual withdrawal of legacy | RF-19; restore backup and reopen migrated project without original DB; pilot approval and all safety gates |

## Minimum validation matrix

Future unit tests will cover validation, selectors/cardinality, loops, footprinting, evaluation, and reviews. The contract ones will share fixtures between engine and agents and will check for unknown messages, incorrect hash, repeated/different payload, late replay and truncation. Integrations will use real Linux/KVM and synthetic repositories without personal data.

The security acceptance must attempt to: read/delete a decoy file from the host; access host Docker/libvirt, LAN, metadata and other lab over IPv4/IPv6; escape via links/traversal; exhaust RAM, processes, disk and inodes; sending excessive logs/blobs and malicious JSON/YAML; commit a container and check that it is still within L1. The acquisition should test redirects/DNS to blocked destinations and malicious Git. Check firewall from the guest even if you have root, not just check configuration.

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