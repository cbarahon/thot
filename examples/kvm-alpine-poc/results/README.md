# POC result archives

Each directory is a smoke evaluation archive. It contains one result JSON per student; each result carries its own run ID because this POC currently starts a separate sandbox run for each student. The archive is evidence from the local CLI, not yet an engine-generated Thot expedient.

`2026-11-22-smoke/` records the final remote KVM run with expected scores 10/10, 2/10, and 0/10, including an explicit skipped result for the functional test after the syntax error. `2026-11-22-first-run/` preserves the initial smoke output, which did not serialize skipped tests. Raw VM disks, bootstrap tokens, and cloud-init seed images are intentionally not included.

`2026-11-22-locked-user-retest/` records a repeat of the integrated CLI flow after student processes were moved to a locked UID/GID with supplementary groups cleared. It produced the same 10/10, 2/10, and 0/10 results; all student domains were removed and the result files were returned to the invoking account.

`2026-11-23-delivery-agent-kvm-retest/` records the same three-student workflow after deliverable acquisition was moved behind the local filesystem delivery agent. It produced 10/10, 2/10, and 0/10, and includes the acquired deliverable names, sizes, timestamps, and SHA-256 provenance in each result. The CLI inventory and retrieval commands were also exercised against the example project folder. Google Drive OAuth was not live-tested because this run had no professor OAuth client or consented account.
