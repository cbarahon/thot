# ADR-0011: P1 sandbox contracts and durable run evidence

- Status: Accepted for the initial P1 implementation
- Date: 2026-11-23

## Context

The KVM-Alpine smoke implementation needs a stable boundary between the trusted
Thot engine and platform-owned VM/container lifecycle. It also needs a portable
record of exactly what was acquired and observed. The existing smoke YAML and
per-student JSON reports do not define those contracts or provide a run-level
integrity and recovery model. OVA multiboot requires files far larger than the
old 128 MiB intake ceiling, and Google Drive listings need stable provenance
beyond filenames.

## Decisions

1. **Keep the current professor YAML and the internal provider contract
   separate.** `thot.dev/v2` remains the current Alpine smoke project format.
   `thot_core::sandbox_contracts` introduces a distinct
   `SANDBOX_CONTRACT_VERSION = 1`; changing the Rust provider contract does not
   silently change the project YAML schema. Contract and manifest versions are
   exact integers/identities: readers reject unsupported versions and unknown
   fields. Additive fields require a newly versioned contract/manifest and a
   declared default/migration; a change to meaning or required fields requires
   a new version. The `thot.run/v1` folder schema is likewise exact and is not
   yet promised to be forward-compatible across releases.
2. **The provider owns sandbox resources.** The async
   `thot_runner::sandbox_provider::SandboxProvider` port owns template,
   baseline, per-student environment, nested resources and cleanup. The engine
   submits a complete student-unit request and records evidence; it never
   creates or deletes hypervisor objects directly through this contract.
3. **Use distinct opaque typed handles.** Template, prepared baseline, and
   student environment identities are separate types. Handles carry provider,
   resource ID, generation digest and ownership ID; configuration does not
   supply a host path or an authoritative VM handle.
4. **Check capability and resource admission before provisioning.** Provider
   reports include capability ID/version pairs, nested-virtualization state and
   administrator resource ceilings. Required unavailable nesting, an
   unsupported capability, or an over-ceiling request fails closed. No
   implicit Docker/host fallback is allowed.
5. **Pin module metadata by identity and digest.** Manifest v1 contains
   module ID, immutable version, package SHA-256, typed capability versions,
   and safe relative asset paths with size and SHA-256. The shared fixture
   proves metadata/digest validation only. No executable package ABI, module
   runtime, signature trust root, or dependency policy is selected here.
6. **Keep operation status separate from academic assessment.** Execution
   outcome uses `completed`, `timed_out`, `resource_exceeded`, `platform_error`,
   `cancelled`, `interrupted`, or `unsupported`. A completed process can have
   a non-zero exit code. The engine's assessment outcome is a separate typed
   value (`passed`, `failed`, `skipped`, or `pending`). Skips are represented
   as an explicit assessment state/reason rather than fabricated process
   execution.
7. **Select deliveries by exact name or stable provider ID.** Duplicate exact
   names are an error; project assignments can use `deliverable_id` when the
   source exposes provider IDs. Drive provenance records file ID, revision or
   version, MIME type and provider checksum when offered, and an independent
   SHA-256 of acquired bytes. Provider checksum algorithms are recorded
   explicitly; Drive MD5 is not treated as the Thot content digest.
8. **Make intake quotas explicit and stream large files.** `delivery_limits`
   defaults to 128 MiB and 10,000 files, preserving existing project behavior.
   The validated maximum is 64 GiB and 100,000 files. Google Drive bytes are
   streamed to private staging while hashing; the full OVA is not buffered in
   memory. The quota is an intake bound only: adequate free disk, guest disk,
   transfer time, backup space, and provider storage remain operator/project
   sizing responsibilities.
9. **Persist a portable, single-writer run folder.**
   `ProjectRunRepository` snapshots the configuration/roster/assignments,
   stores immutable acquired input, records provenance, content-addresses
   captured streams and artifacts, appends a fsynced hash-chained JSONL journal,
   and publishes a revision after evidence is durable. Starting another writer
   fails while a run repository holds the lock. Reopening marks unfinalized
   runs interrupted; the system does not replay uncertain provider work.
   Verification detects accidental changes but is not a signature or proof
   against a folder owner who can rewrite all metadata. When invoked through
   `sudo`, the run tree, results directory and writer lock return to the
   invoking professor's UID/GID on commit and error cleanup. Files and
   directories are restricted to owner-only permissions because the archive
   includes student submissions.

## Consequences

- The P1 types and folder repository can be exercised without claiming that a
  production KVM provider, Docker provider, or module runtime exists.
- The current KVM-Alpine application is not yet wired through the provider
  trait; the provider lifecycle extraction is subsequent provider work.
- Run JSON, journal and revision records are an initial storage API. Their
  versioning/forward-compatibility policy and recovery/concurrency acceptance
  evidence still need qualification before declaring P1 fully closed.
- Larger OVA intake is possible only when the professor configures suitable
  quotas and the host has enough disk. This does not establish that the
  sandbox can boot Windows or that nested virtualization is isolated.
- The fixture in `examples/sandbox-contracts/` is metadata-only and must not be
  mistaken for a runnable or trusted module package.

## Remaining decisions

- Define migration tooling and retention of readers for older run-folder
  versions before stable archive/import support.
- Choose an executable module format and sandboxing runtime; define trust,
  signing, revocation and dependency distribution.
- Specify bounded/restartable artifact transfer for a remote provider. The
  present provider trait is an in-process async Rust contract, not a wire
  protocol.
- Decide whether run evidence becomes immutable/signed or remains an editable
  folder with explicit imported corrections and local integrity checks.
- Qualify lock and atomic-rename behavior on supported filesystems and decide
  the project archive/import and retention interfaces.

## Validation evidence

Focused shared-fixture tests in `thot-engine` and `thot-alpine-agent` validate
valid package/asset digests and reject a bad digest. Projected Alpine smoke
validation exercises a real KVM run and checks the committed portable folder.
Neither validation is a Windows OVA, nested guest, hostile input, remote
transport, nor provider-trait conformance test. Repository smoke tests cover
input/artifact persistence, revision verification, writer collision and
interrupted-run recovery. A real three-student KVM-Alpine smoke run completed
with grades 10/10, 2/10 and 0/10; the latest package was owned by the invoking
professor account. Google OAuth/large-file download was not exercised here.
