# Implementation status

| Workflow | Available today | Evidence and limits |
|---|---|---|
| Local KVM Alpine CLI | Runs ordered instructor tests in a disposable Alpine guest, stages each local or Drive deliverable, and saves a run journal plus per-student JSON. | The Bash fixture still scores 10/10, 2/10 and 0/10. The portable libvirt OVA module now passes import/start/stop/destroy with a synthetic OVA (10/10); the real 8.49 GB OVA still awaits a sufficiently large host. See the [implementation record](implementation/kvm-alpine.md) and [screening guide](guides/ova-screening.md). |
| Submission intake | Local folders are the sandbox CLI default. The Google Drive agent supports read-only OAuth and ordinary uploaded files. Project `delivery_limits` default to 128 MiB/10,000 files and are configurable up to 64 GiB/100,000 files. | A 20 GiB limit retrieved the 8.49 GB test OVA with a verified SHA-256. See the [delivery guide](guides/delivery-agents.md) and [screening guide](guides/ova-screening.md). |
| Daemon and VirtualBox activities | Use local OVA submissions, project rosters, expedients and grade reports. The HTTP agent controls VirtualBox on its configured host. | The [implemented catalogue](implementation/vm-activity-reference.md) documents supported tests and scoring. The [screening guide](guides/ova-screening.md) covers partial OVA evidence. |
| Drive-to-KVM multiboot | Planning is closed for the first laptop implementation; libvirt-based OVA lifecycle smoke passes. | Real OVA import on a host with enough disk, EFI boot, nested guest runs, OS-specific shared-volume checks and Drive-to-KVM qualification remain. See the [implementation plan](planning/ova-multiboot.md). |

The architecture specifies the provider-managed workflow. The current Alpine CLI implements the narrower smoke path. Consult the implementation record before relying on a proposed provider, protocol or report feature.
