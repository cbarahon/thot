# Implementation status

| Workflow | Available today | Evidence and limits |
|---|---|---|
| Local KVM Alpine CLI | Runs ordered instructor tests in a disposable Alpine guest, stages each local or Drive deliverable, and saves a run journal plus per-student JSON. | The Bash fixture still scores 10/10, 2/10 and 0/10. The portable libvirt OVA module now passes import/start/stop/destroy with a synthetic OVA (10/10); the real 8.49 GB OVA still awaits a sufficiently large host. See the [implementation record](implementation/kvm-alpine.md) and [screening guide](guides/ova-screening.md). |
| Submission intake | Local folders are the sandbox CLI default. The Google Drive agent implements read-only OAuth and ordinary uploaded files; builds can bundle a developer-owned OAuth client for individual professor consent. Live Drive OAuth and transfer remain untested. Project `delivery_limits` default to 128 MiB/10,000 files and are configurable up to 64 GiB/100,000 files. | A 20 GiB limit retrieved the 8.49 GB test OVA from a local source with a verified SHA-256; this is not Drive evidence. See the [delivery guide](guides/delivery-agents.md) and [screening guide](guides/ova-screening.md). |
| Daemon and VirtualBox activities | Use local OVA submissions, project rosters, expedients and grade reports. The HTTP agent controls VirtualBox on its configured host. | The [implemented catalogue](implementation/vm-activity-reference.md) documents supported tests and scoring. The [screening guide](guides/ova-screening.md) covers partial OVA evidence. |
| Drive-to-KVM multiboot | Planning is closed for the first laptop implementation; libvirt-based OVA lifecycle smoke passes. | Real OVA import on a host with enough disk, EFI boot, nested guest runs, OS-specific shared-volume checks and Drive-to-KVM qualification remain. See the [implementation plan](planning/ova-multiboot.md). |

The architecture specifies the provider-managed workflow. The current Alpine CLI implements the narrower smoke path. Consult the implementation record before relying on a proposed provider, protocol or report feature.

## Local workflow migration target

[ADR-0017](decisions/0017-local-evaluation-cli.md) selects a unified background `thot run`, on-demand workers, durable live progress and timeouts, layered configuration with one resource declaration, and native Rust test modules. Initial CLI, configuration, progress, and evidence changes have been implemented and smoke-tested. The [architecture](architecture/local-evaluation.md) distinguishes these delivered slices from remaining targets; the [migration backlog](planning/local-evaluation-migration.md) tracks acceptance criteria. Full real-OVA and multiboot qualification remains open.

## Migration implementation in progress

The CLI now routes sandbox projects through a detached worker and provides
`thot status`, `thot status --watch`, `thot log`, and `thot log --follow`.
Project runtime defaults can be installed system-wide or per user, overridden
by project runtime settings, `.env`, and process environment. The Alpine agent
reports task heartbeats; provider operations have bounded command timeouts.
Compact platform profiles resolve into the existing validated internal
configuration. The OVA example now uses the explicit v4 version marker, while
v2 and v3 remain accepted. Directory deliverables are persisted as read-only
trees with digest verification.

Workspace tests pass. On the recovered Debian 13/KVM host, a fresh background
smoke evaluation completed all three students: 10/10, 2/10, and 0/10, with
clean clone cleanup. `status`, `log`, `report`, and `status --watch` worked;
verified run export/import preserved the run ID and integrity checks. No VM
remained defined after completion. Host setup required adding the professor to
the `libvirt-qemu` group and granting that group access to the dedicated
storage root; the deployment guide now includes this. A separate cleanup removed
three abandoned 8.49 GB OVA staging copies (about 24 GiB total) from prior
interrupted runs; the source OVA and reusable Alpine baseline were preserved.
New staging directories carry an owner PID and are swept after that process
exits. This cleanup was verified with an abandoned-directory fixture, followed
by another full background smoke run (`92362743-f4e7-4d41-9f7e-ed56e32b9d65`)
that again scored 10/10, 2/10, and 0/10 with clean VM cleanup. The real OVA and
full multiboot qualification remain untested. Safe cancellation, service-manager
supervision of sandbox workers, richer progress events, and the general native
module ABI remain incomplete.

The Alpine guest can also execute digest-verified Linux x86-64 ELF modules when their manifest requests `module.linux-x86_64-elf@1`; this is the first native-binary path, not yet the general JSON event/result ABI or cross-platform catalogue described in the target architecture.
