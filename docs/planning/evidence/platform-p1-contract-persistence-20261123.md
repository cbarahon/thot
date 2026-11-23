# P1 contracts and persistence validation

Date: 2026-11-23

## Scope

Focused validation of the P1 shared contracts and run-folder persistence, plus
the existing three-student KVM-Alpine smoke activity on the designated
disposable test host. This is not the P0 nested Windows/OVA qualification and
does not claim multiboot readiness.

## Local checks

- `cargo fmt --all -- --check` — passed.
- `cargo check -p thot-core -p thot-runner -p thot-engine -p thot-delivery -p thot-kvm-alpine -p thot-alpine-agent -p thot-cli` — passed.
- `cargo test -p thot-core --test sandbox_contracts` — 3 passed: rejects
  over-ceiling resources and unavailable required nesting, rejects a missing
  module capability, and rejects an unknown unversioned manifest field.
- `cargo test -p thot-engine --test shared_module_contract --test sandbox_repository`
  — 3 passed: shared manifest/digest fixture, run input/artifact/revision
  verification, writer collision and interrupted-run recovery.
- `cargo test -p thot-alpine-agent --test shared_module_contract` — 1 passed.
- `cargo build -p thot-cli --release` and the statically linked
  `thot-alpine-agent` musl release build — passed.

No workspace-wide test suite was run.

## KVM-Alpine smoke

The release CLI and guest agent ran the existing POC project on the
disposable KVM host. The latest run ID was
`66975935-edb9-496a-bc11-a65bfd87aaff`; the CLI returned successfully and
reported student results `10/10`, `2/10`, and `0/10`. It created a complete
`thot.run/v1` manifest with a committed revision. Revision commit invokes
`ProjectRunRepository::verify_run`, which checked the configuration snapshot,
journal, evidence hashes and result paths before success was reported.

After completion, the run folder and files were owned by the invoking
professor account and had modes `0700` for directories and `0600` for files.
The `results/` directory was `0700`, the writer lock was `0600`, and the
professor account could write both package and lock. No student clone domains
remained registered after the run.

## Not exercised

- Google OAuth, Shared Drive, revision-specific download, large-file streaming,
  and actual OVA intake.
- Windows OVA boot, nested KVM, GRUB selection, cross-OS shared-data checks,
  hostile archives/scripts, network-escape probes, resource exhaustion, and
  provider-trait conformance.
- Portability/editing across operating systems, archive import/export, or
  network filesystems.
