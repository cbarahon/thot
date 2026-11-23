# KVM Alpine P0 smoke evidence — 2026-11-23

This record contains raw synthetic result JSON for two sequential runs of the
checked-in `examples/kvm-alpine-poc` fixture against the authorized Debian 13
KVM test host. No real student submissions were used.

| Run | Result run ID | Wall time | Student scores | State |
|---|---|---:|---|---|
| [Cold image and baseline](cold-run/) | `1c4e962f-a6f1-4e9a-b1b9-978bbb72248c` | 216.7 s | student1 10/10, student2 2/10, student3 0/10 | exit 0 |
| [Reused image and baseline](warm-run/) | `2f80d28f-634b-460b-95e6-d2219c11dc77` | 128.1 s | student1 10/10, student2 2/10, student3 0/10 | exit 0 |

Both used Thot version `0.1.44` built from repository revision
`91ac68facb7a34c1796ae66a8521c7108e487127`:

- CLI SHA-256: `e3acd18fb85d3fb1f4104e6e9f4140f1145b962ad45e10ab36686008a83cf585`
- Alpine guest agent SHA-256: `d77a4fb44ed97c12990a92af091f0879924160c04af205dc98e0eeb71ff8e184`

The first run includes pinned image download, baseline preparation, and three
student runs. The second reused the image and baseline. Both ran sequentially
with one 512 MiB / one-vCPU Alpine VM per student and reported normal cleanup.
The evidence does not establish nested KVM, Windows/OVA support, peak resource
usage, hostile workload containment, or restart recovery. The JSON captures
assessment observations, not host performance samples or proof of guest
integrity.
