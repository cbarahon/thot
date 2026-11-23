# P4–P7 initial implementation evidence

Date: 2026-11-23

## Implemented slice

- P5: `thot.dev/v3` supports multiple independent weighted evaluation items.
  Evaluation preserves completed item grades when other items depend on
  infrastructure-pending observations. V2 projects retain the legacy single
  rubric. ADR-0013 records the contract.
- P6: `thot sandbox report RUN_DIR` verifies the committed run before showing
  a summary. `--json`, `--csv`, and `--html FILE` provide machine-readable,
  spreadsheet, and offline report output. The HTML renderer escapes untrusted
  content and retains Unicode.
- P7: `thot sandbox export RUN_DIR ARCHIVE.zip` archives a complete verified
  run without recompressing already-compressed submissions. `thot sandbox
  import ARCHIVE.zip DEST_DIR` extracts to a new private directory, rejects
  unsafe members and bounded-size violations, then verifies the run. ADR-0014
  records this evidence-only archive contract.
- P4: existing contracts scope tasks and observations to one platform and
  require same-platform task dependencies. The current KVM CLI already runs
  sequential tests in one fresh per-student clone and cleans that clone before
  moving on. Multiple-platform dispatch and typed OVA operation execution are
  not implemented.

## Validation

- `cargo check -p thot-core -p thot-engine -p thot-kvm-alpine -p thot-cli` —
  passed.
- `cargo test -p thot-core --test sandbox_contracts` — 4 passed.
- `cargo test -p thot-kvm-alpine --lib` — 3 passed, including propagation of
  infrastructure-pending state through dependent skips.
- `cargo test -p thot-engine sandbox::tests` — 3 passed.
- `cargo test -p thot-engine --test sandbox_repository` — 4 passed, including
  run archive export/import, post-import verification, and archive traversal /
  compressed-member rejection.
- `cargo test -p thot-cli sandbox_commands::tests` — 1 passed, covering
  multiple evaluation items, Unicode and HTML escaping.
- A previously completed three-student KVM-Alpine run
  (`5e84f6ee-f275-43e5-8f1d-bb61e07ee51d`) was copied from the test host,
  verified, rendered as table/CSV, exported to ZIP, imported into a separate
  folder and rendered again. Marks remained 10/10, 2/10 and 0/10.

No workspace-wide tests or Clippy run were performed.

## OVA qualification status

The provided `ArchLinux_Dualboot.ova` was fingerprinted and its tar directory
and OVF descriptor inspected without extracting the VMDK or booting it. The
descriptor declares one stream-optimized VMDK with a 60 GiB virtual capacity,
4 GiB guest RAM, one vCPU, EFI, and one NAT adapter. This is metadata, not
partition or boot evidence. The test host has 4 CPUs, about 5.8 GiB RAM and
about 39 GiB free root storage. It cannot credibly qualify this nested Windows
guest with sufficient outer-VM and host headroom. No OVA parser, import or
guest boot test was run.

The activity is therefore not closed. Remaining hard gates include choosing
and proving a nested hypervisor in Alpine, guest `/dev/kvm` exposure and
unprivileged adapter policy, large OVA staging beyond the current 2 GiB smoke
profile, a bounded clone-local OVA/VMDK inspection module, GRUB selection and
OS identity confirmation, shared-data probes, live Google Drive transfer,
provider cleanup/recovery, and qualification on adequate hardware. Typed OVA
operation names already present in the internal contracts are not executable
features.
