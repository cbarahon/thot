# Platform P0: viability and threat register

**Status:** partial evidence collected; P0 is **not closed**.  The lightweight
`kvm-alpine` smoke path is viable on the recorded test host.  Nested Linux and
Windows guests, OVA handling, hostile-input containment, resource ceilings,
crash recovery, and cleanup under forced failure remain unqualified.  Do not
use student OVAs or claim production isolation based on this report.

This report is the evidence and security gate for P0 in the
[platform roadmap](platform-roadmap.md). It interprets the target design in
[architecture overview](../architecture/overview.md),
[requirements](../architecture/requirements.md), and
[ADRs 0007, 0008, and 0010](../decisions/README.md). It reports the current
implementation, not an assurance certification.

## Scope and decision boundary

P0 asks whether a professor-managed Linux/KVM host can create an Alpine outer
VM, prepare a reusable project baseline, create disposable per-student clones,
transfer tests and a submission, execute them over VSOCK, return evidence, and
remove student resources. The OVA multiboot profile adds nested virtualization,
disk inspection, and nested Linux/Windows boots. These are separate claims:
the existing smoke run establishes only the first, low-resource path.

P0 may qualify supported hardware and document constraints. It does not enable
arbitrary untrusted OVA execution. Until P2 boundary validation, do not put
student code, OVA images, OAuth credentials, or real submissions on this
prototype host. If nested virtualization or a required isolation control is
unavailable, fail preflight and stop; never substitute Docker or host
execution for a selected KVM platform.

## Evidence collected

### Hosts

| Host | Observations | P0 conclusion |
|---|---|---|
| Workspace `dev-thot` | Debian 13.7; Linux `7.0.14-23-pve`; 4 vCPU; 4.0 GiB RAM; 25 GiB root with about 11 GiB free; no `/dev/kvm`; `virsh`, QEMU, and Docker tools unavailable | Cannot run or qualify the provider here. A KVM-capable remote host is required. |
| Authorized test host `thot-poc-test` (`192.168.1.35`) | Debian 13.7; Linux `6.12.113+deb13-amd64`; 4 vCPU; 3.8 GiB RAM plus 1.7 GiB swap; 30 GiB root with about 18 GiB free; `/dev/kvm` accessible; Intel `vmx` visible; `/sys/module/kvm_intel/parameters/nested` is `Y`; libvirt 11.3.0; QEMU 10.0.13; `virt-host-validate qemu` passes KVM/device/cgroup checks, with IOMMU and secure-guest support unknown | Suitable to repeat the existing low-resource POC. Hardware exposes prerequisites for a nested-KVM experiment, but this is not proof that nested KVM works inside the Alpine guest. This machine is too small to qualify the proposed Windows workload. |

Host measurements are a point-in-time inventory, not minimum requirements or a
laptop recommendation. Linux version, KVM module setting, `vmx`, and `/dev/kvm`
must be checked on the actual host; they must not be assumed from CPU model or
libvirt configuration alone.

### Smoke run

On 2026-11-23, release binaries built from the current workspace were copied to
a clean project copy on the authorized test host. The run used the checked-in
three-student Bash fixture, the pinned Alpine 3.24.1 x86_64 cloud image, a
512 MiB / 1 vCPU outer guest, and a dedicated directory under
`/var/lib/libvirt/images`. The run was started as root because the current
local CLI directly invokes host virtualization tools; this is a prototype
deployment property, not the target least-privilege provider design.

| Run | Outcome | Time and observations |
|---|---|---|
| Fresh image/baseline | Exit 0; student1 10/10, student2 2/10, student3 0/10 | 216.7 seconds wall time including pinned image download, preparation, three sequential boots and cleanup. Student domains were removed after each run. |
| Reused image/baseline | Exit 0; student1 10/10, student2 2/10, student3 0/10 | 128.1 seconds wall time. The image and prepared baseline were reused; student domains were removed after each run. |

During the run, QEMU used `-accel kvm`, `-cpu host`, a 512 MiB guest memory
limit, one vCPU, and VSOCK. The student domain had no emulated NIC. A sample
`ps` observation showed about 67–84 MiB QEMU RSS while a guest was active;
host `free` showed 2.7–3.5 GiB available in sampled moments. These are
snapshots, not peak measurements. No valid CPU peak, guest memory peak, clone
latency, maximum disk growth, or Windows/nested performance measurement exists
yet. The raw synthetic result JSON and build fingerprints are preserved in
the [P0 smoke evidence record](evidence/platform-p0-smoke-20261123/README.md).

The first attempt placed the sandbox root under `/tmp` and failed before any
guest ran: the provider's free-space preflight requires 3 GiB, while that
filesystem did not have 3 GiB available. Repointing to the dedicated libvirt
image volume passed preflight. The CLI currently reports the raw storage
constraint but does not offer a professor-friendly capacity explanation or
estimate; record this as a usability gap and retain the preflight.

After both runs, `virsh list --all` contained no student domains. The 2 GiB
virtual-size template disk allocated about 184 MiB, and the 2 GiB virtual-size
prepared baseline allocated about 12.5 MiB; the image cache occupied about
184 MiB. This reports the QCOW2 allocated sizes after the test, not a
worst-case overlay growth bound.

The smoke run does **not** demonstrate a libvirt template VM, snapshot-based
baseline, template immutability, host resource ceilings, nesting inside the
Alpine guest, Linux or Windows boot of a submitted OVA, shared-data tests,
network policy against a hostile guest, timeout/process-tree termination,
forced-crash cleanup, restart reconciliation, artifact durability, or engine
expedient persistence. The current clone is a QCOW2 overlay from a prepared
disk and the agent is injected per student. See the
[implementation status](../implementation/kvm-alpine.md).

## Threat register

Treat every student deliverable, repository, archive, OVA descriptor/disk,
guest operating system, and test output as hostile. Instructor scripts and
test modules are trusted only after their source, digest, and declared
privileges are reviewed. The controls below are gates, not claims that the
current POC already implements them.

| ID / threat | Attack or failure | Required control and verification | Current state |
|---|---|---|---|
| T1 Host escape through KVM/QEMU | Malformed OVA, guest kernel, emulated device, or nested guest exploits a host kernel/hypervisor bug | Patched dedicated Linux host; no professor daily-use account/data on the execution host; minimal QEMU device model; no host paths/sockets/device passthrough; test escape attempts against decoys and host canaries; document residual hypervisor risk | Not tested. Current POC runs QEMU on the professor host, often with root CLI privileges. |
| T2 Nested virtualization widens attack surface | Student-controlled Alpine clone can launch further VMs and exercise nested KVM | Nested KVM is opt-in per platform and unavailable unless host preflight confirms it; expose only the required CPU virtualization feature to disposable clone; cap nested VM count/resources; verify child VMs cannot see host devices, libvirt sockets, other clones, or L0 storage; include nested-host kernel in patch policy | Host prerequisites found; nested guest execution not tested. |
| T3 Host file or resource damage | Path traversal, symlinks, malicious qcow backing references, sparse expansion, decompression bombs, disk-full, process/memory/inode exhaustion | Stage under provider-owned root; reject links/traversal/unsupported formats before import; parse with quotas; bound expanded bytes, disk, memory, vCPU, PIDs, CPU time, logs and artifacts; enforce host-level quotas independent of guest requests | Only some path/config/output/time checks exist. Host-wide CPU, PID, disk and inode caps are not qualified. |
| T4 Cross-student contamination | Shared writable baseline, stale overlay, leaked process, reused VSOCK secret, or persistent nested VM contaminates later work | Immutable read-only parent; unique student overlay and session secret; no shared writable mounts; clean up outer and nested domains plus overlays; verify with marker/process/VM decoys across consecutive submissions and after forced termination | Sequential smoke cleanup observed; malicious persistence and crash recovery not tested. |
| T5 Guest-to-host / LAN communication | Student code probes host, metadata services, LAN, DNS, or other student VMs | No student NIC by default; provider firewall denies IPv4 and IPv6 egress/ingress including host/bridge routes; VSOCK is the sole narrow control channel; verify from guest using a network probe and inspect host firewall counters | QEMU command observed without a NIC; exhaustive IPv4/IPv6/host/LAN checks not run. |
| T6 VSOCK spoofing/replay or result forgery | Stale clone or student-controlled guest claims another task's result, replays a message, or edits agent output | Bind session to expected CID, run/platform/student/task IDs, sequence and one-use secret; constant-time authentication; length/hash/identity checks; deduplicate IDs; expire secret; persist before ACK; distinguish authentication from attestation | ADR-0008 specifies this. Current POC implements only a subset; hostile replay/cross-clone tests remain. Guest root can still falsify observations; VSOCK is not attestation. |
| T7 OVA/parser and image-chain abuse | Tar/OVF paths escape staging, huge metadata, malformed XML, chain points outside staging, unsupported backing format | Bounded streaming; safe archive extraction without following links; allowlisted descriptor references contained under staging; depth/size/count ceilings; reject external paths, encryption and unsupported features before VM launch; test malicious fixtures | OVA path is not implemented in v2 sandbox. No hostile OVA tests run. |
| T8 Network/supply-chain compromise | Image, Alpine package, agent, module, mirror, or preparation dependency is changed or malicious | Pin and hash images, agent, module bundles and scripts; verify signatures where available; record package versions and repository identity; allowlist preparation network only; deny student-phase network; rebuild baseline on provenance mismatch | Cloud image SHA-512 pin is checked. Signature verification, package provenance, and end-to-end module provenance are incomplete. |
| T9 Credential disclosure | Google OAuth, host SSH keys, cloud credentials, or service tokens become guest-readable or enter results | OAuth and delivery retrieval stay on host; transfer only immutable selected bytes; no credentials in guest seed, environment, logs or exported results; use per-run short-lived session material; verify by scanning guest inputs/results and token file permissions | Architecture requires host-side OAuth. The smoke project uses local files; secret-scanning of seeds/artifacts not performed. |
| T10 Preparation script abuse | Trusted instructor script has a typo, supply-chain payload, or excessive privilege and poisons every student baseline | Separate one-time preparation phase; explicit confirmation/provenance; digest and timeout per script; network policy; record stdout/stderr/package inventory; fail closed; new immutable baseline for any change | Bash installation preparation passes smoke. Full network, privilege and provenance controls are not qualified. |
| T11 Output/result denial of service or falsification | Unlimited stdout/stderr, malformed JSON, oversized artifacts, truncation ambiguity, agent compromised by student | Frame/blob caps, bounded disk-backed streams with continued pipe draining, schema/version checks, hashes, stream completion flags; result status cannot equal grade; infrastructure failures leave grades pending | Basic bounded process capture and VSOCK fixture pass. Adversarial size, malformed frame, disconnect and persistence tests remain. |
| T12 Cleanup failure and uncertain ownership | Crash leaves a running VM, orphan disk, or wrong similarly named domain is deleted | Opaque provider UUID/manifest ownership; never adopt/delete by display name alone; persistent resource registry and lock; startup reconciliation; uncertain cleanup pauses next deliverable and remains visible | Name/provenance checks and normal cleanup exist in POC; durable provider registry and crash reconciliation do not. |
| T13 Instructor mistakes configuration | Invalid limit, wrong network mode, unsupported capability, or too-small resources produce a misleading zero | Validate all config and host ceilings before side effects; show a readable preflight plan; reject unsupported nested features; keep infrastructure failure separate from assessment; no fallback | V2 parsing and some host checks exist. No nested capability negotiation or human-readable resource estimate. |
| T14 Malicious test module | A module's declared capability hides arbitrary host execution or leaks data | Catalogue-installed, digest-pinned modules; typed capability API; module execution inside sandbox; deny arbitrary host commands and host paths; review/signing policy before distribution | Module packaging/runtime contract is not implemented. Current tests are professor-supplied scripts staged into the guest. |

### Threat assumptions

- The outer KVM host, its kernel, QEMU/libvirt, Thot provider, and the
  instructor's approved test modules are part of the trusted computing base.
- A sandbox guest may be fully controlled by the student. Its result channel
  provides scoped transport and evidence, not proof that the guest is honest.
- KVM reduces exposure compared with host execution but does not eliminate
  hypervisor escapes or hardware side channels. Nested KVM increases the
  emulation and privilege-transition surface and needs a separate qualification.
- Docker is not an acceptable fallback boundary for arbitrary hostile OVA or
  guest execution. Host execution remains limited to reviewed low-risk
  adapters, consistent with ADR-0007.
- A professor's own laptop containing personal or institutional data is a poor
  place to run adversarial nested guests. P0 must say what host hardening is
  required and what residual risk the professor accepts before classroom use.

## P0 exit gates

P0 remains open until evidence is attached for every applicable item below.
Any mandatory gate failure blocks the OVA multiboot scenario.

1. **Reproducible host qualification:** documented preflight checks for CPU
   virtualization, nested mode, `/dev/kvm`, libvirt/QEMU versions, available
   RAM/storage, IOMMU status, and configured host ceilings; fail-closed case
   when any required capability is missing.
2. **Nested Linux proof:** boot a nested Linux guest from inside a disposable
   Alpine clone with explicit outer/inner resource settings. Confirm the
   inner guest can use its assigned virtual disk and console; confirm it
   cannot reach the outer host's libvirt, host files, devices, or another
   student's clone.
3. **Windows resource/boot proof:** qualify one instructor-owned Windows test
   image at the selected outer and inner sizes; record boot time, peak host
   and guest memory, CPU, storage growth, and timeout behavior. A small smoke
   VM result cannot be extrapolated to this workload.
4. **Isolation and threat probes:** decoy host read/write, IPv4/IPv6 host/LAN
   reachability, nested VM cleanup, output flood, fork/process flood, disk
   expansion, timeout and descendant termination, malformed protocol, replay,
   student-to-student persistence, and forced host/provider interruption.
   Record tool output and whether each attack was blocked, detected, or
   unresolved; unresolved high-impact cases block use with hostile work.
5. **Lifecycle and evidence:** repeat clean baseline reuse; verify parent
   immutability, fresh overlays, result hashes and bounded artifacts; inject
   cleanup failure and prove the next deliverable waits; reconcile after
   provider restart without repeating uncertain work.
6. **Capacity envelope:** measure cold setup, baseline preparation, warm
   student run, per-clone disk growth, peak CPU and RAM, and maximum safe
   concurrent work. State a recommended low-resource profile separately
   from the multiboot profile, supported by measurements rather than guessed
   YAML values.
7. **Decision record:** update the roadmap with pass/partial/fail for each
   gate, known host profiles, residual threats and explicit no-go conditions.
   A successful smoke test alone does not close P0.

## Immediate follow-up

1. Finish the warm-run capture and preserve a sanitized P0 log/result set.
2. Add a dedicated nested-virtualization fixture with the smallest viable
   Alpine outer profile and a tiny Linux inner guest; do not use student OVA
   data.
3. Decide whether to continue on the 4 GiB test host. It may prove only the
   Linux nesting mechanism; Windows qualification needs a larger dedicated
   host and sufficient free disk.
4. Implement the fail-closed host capability/resource check and platform
   network policy before any adversarial guest test.
5. Run the threat probes from the exit gates and attach logs before marking
   P0 complete or beginning P2 security claims.
