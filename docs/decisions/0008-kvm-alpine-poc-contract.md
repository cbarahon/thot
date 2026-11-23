# ADR-0008: KVM Alpine POC project and provider contracts

Status: **Accepted for the KVM Alpine POC**, 2026-11-22. Scope: `kvm-alpine` local Linux provider and its new v2 project configuration. This decision does not change the existing VM activity YAML, `/api/v1`, VirtualBox agent HTTP protocol, or persisted legacy result semantics.

## Context

The POC implementation contains competing provider traits, incompatible configuration shapes, an incomplete guest protocol, and no stable distinction between command outcomes and infrastructure failures. Implementing VM operations before those interfaces are fixed risks producing successful-looking but unauditable results. Existing `VMActivityProject` is used for legacy VM activities and must remain loadable while the new sandbox project model is introduced.

## Decisions

### Canonical configuration

The canonical configuration is `configuration.thot.yaml`, with `schema_version: thot.dev/v2`. `thot-core::sandbox_project::SandboxProject` is the shared Rust model, and `examples/kvm-alpine-poc/configuration.thot.yaml` is its canonical fixture. `students.yaml` and `assignments.yaml` are separate project files referenced by the root configuration. Checks are descriptive mappings from check IDs to required test IDs; each test declares its expected exit code and optional exact stdout. Rubric criteria reference check IDs and points. No parallel `configuration.yaml` form is introduced for sandbox v2. Existing legacy VM activity configuration continues to use `VMActivityProject` and its compatibility rules; there is no implicit conversion between the schemas.

The Rust configuration model belongs in `thot-core` and is serde serializable/deserializable:

- `SandboxProject`: schema version, project identity/name, referenced roster and assignments files, and sandbox platforms.
- `SandboxPlatform`: stable `id`, `kind`, isolation scope, lifecycle, resource limits, network policy, input declarations, ordered tests, and evaluation rules.
- `SandboxLifecycle`: image and template identity, VM resources, guest agent version, project-baseline configuration, preparation packages/scripts, and per-student clone policy.
- `SandboxTest`: stable ID, action, dependency IDs, `argv`, timeout, expected exit code, and optional exact stdout. No shell parsing is implicit; the professor may explicitly put a shell executable in argv.
- `SandboxEvaluation`: named checks referencing test IDs plus rubric criteria referencing check IDs and point values.

The parser validates references, unique IDs, path containment, bounded positive limits, dependency ordering, test IDs, and rubric references before host effects. Unknown schema versions and unsupported platform kinds are rejected. The project YAML can request limits below host policy but cannot raise host ceilings or assert that a security control is enabled. The first local CLI implementation supports one `kvm-alpine` platform; additional kinds need explicit providers.

### One provider API and typed handles

There is one async, object-safe provider interface, owned by `thot-runner` and consumed by `thot-engine`. The engine owns task planning, evidence validation, grading, and result persistence. The provider owns host preflight, libvirt resources, guest transport, execution delivery, and cleanup. Provider methods return typed errors; no method returns fabricated success or silently falls back to host execution.

The interface has these operations, in lifecycle order:

1. `preflight(platform) -> HostCapabilities`
2. `ensure_template(platform) -> TemplateHandle`
3. `prepare_baseline(platform, template, run_snapshot) -> (BaselineHandle, PreparationResult)`
4. `create_student_environment(platform, baseline, run_id, student_unit_id, inputs) -> StudentEnvironmentHandle`
5. `execute_test(environment, task) -> AgentResult` (called sequentially for each declared test)
6. `destroy_student_environment(environment) -> CleanupResult`
7. `reconcile_owned_resources() -> ReconciliationReport` (startup and operator maintenance)

Concrete opaque handles are `TemplateHandle`, `BaselineHandle`, and `StudentEnvironmentHandle`. Each carries a provider kind, opaque resource UUID, generation/manifest digest, and ownership identity; baseline handles reference the template generation, and student handles reference the run, student unit, and baseline. Handles must not expose host file paths or be forgeable from project-supplied names. Persistent registry records, not the opaque in-memory Rust value, are authoritative after restart. A domain name is a display attribute only; lookup/deletion requires matching Thot ownership UUID and expected generation. Student handles are single-run and single-unit scoped.

The shared task/result Rust models are `AgentTaskV2` and `AgentResultV2` in `thot-core::agent_protocol`. `AgentTaskV2` contains protocol version, task and attempt IDs, run/platform invocation/student-unit/test IDs, an argv vector, guest-relative working directory, validated environment-variable map, timeout seconds, stdout/stderr byte caps, and input blob references with guest-relative destinations and SHA-256. It contains no host path, student roster data, credential, grade, or arbitrary provider command. `AgentResultV2` contains those correlation IDs, `ExecutionStatus`, start/end UTC timestamps, monotonic duration milliseconds, optional process exit code and signal, stdout/stderr `CapturedStream` descriptors, structured observations, `Diagnostic` entries, and declared `ArtifactRef`s. A stream descriptor has byte count, truncation and completion flags, SHA-256, and a content-addressed blob reference when retained. Every diagnostic has a stable code, message, retryable boolean, and optional bounded technical detail. Assessment and cleanup results are engine/provider records respectively; the agent cannot set them.

All methods are cancellation-aware and idempotent where safe. Repeating an operation with the same idempotency key and payload returns its known state; a different payload for an existing identity is a conflict. Destruction is idempotent, ownership checked, and returns verified cleanup state. Cleanup failure is retained and blocks reuse of uncertain resources.

### VSOCK task transport

For the local KVM Alpine POC, the guest agent initiates an `AF_VSOCK` connection to a host listener. The host listener is bound only for the expected student environment and a dynamically allocated port; there is no guest IP listener or general inbound host port. The guest connects to host CID 2. The host checks the peer CID against the CID assigned to that libvirt domain and binds the accepted session to its typed student-environment handle.

Each student environment receives a freshly generated 256-bit random, one-use session secret over a per-clone, read-only bootstrap device. The secret is not stored in the reusable template, project baseline, source tree, logs, or result archive. It is invalidated after handshake, clone destruction, lease expiry, or run cancellation. The guest proves possession in the VSOCK handshake; comparison is constant-time. The host validates protocol version, expected run/platform/student/task scope, peer CID, nonce, and secret before accepting task messages. This prevents accidental cross-run/stale-agent connections; it is **not** attestation and cannot make results tamper-proof against a student who controls the guest as root. The POC runs tests as an unprivileged guest account and records this assurance limit.

Wire encoding is UTF-8 JSON preceded by a four-byte unsigned big-endian payload length. Maximum JSON frame is 1 MiB; frames are read/written with exact-length loops and oversized, truncated, invalid UTF-8/JSON, unknown-version, duplicate, and out-of-scope messages are rejected. Every envelope carries `protocol_version: 2`, `message_id`, `message_type`, `run_id`, `platform_invocation_id`, `student_unit_id`, a monotonically increasing per-session `sequence`, and a typed payload. Task messages additionally carry stable `task_id` and payload SHA-256. The agent executes tasks sequentially and returns one terminal result per task. Repeated same-ID/same-hash requests return the recorded state; same-ID/different-hash is a protocol conflict. A transport disconnect does not imply the task did not execute; the host queries/reconciles the task state and never silently resubmits with a new ID.

Large files and streams are not embedded without bound. Blob transfer is chunked into payloads of at most 256 KiB with blob ID, offset, total byte count, SHA-256, and final commit acknowledgement. Aggregate bytes and concurrent transfers are bounded by host policy. Results are persisted by the engine before ACK; the agent retains unacknowledged bounded results only for the active lease. A missing ACK, invalid hash, or quota failure is an infrastructure error.

### Status ownership and meaning

Execution and assessment are separate typed fields; v2 does not overload the legacy `TestStatus` enum:

- `ExecutionStatus` (provider/agent fact): `completed`, `timed_out`, `resource_exceeded`, `platform_error`, `cancelled`, or `interrupted`.
- `AssessmentStatus` (trusted engine decision): `passed`, `failed`, `skipped`, or `pending`.
- `CleanupStatus`: `clean`, `pending`, or `failed`, independent of execution and assessment.

`completed` says only that execution ended and observations exist; a nonzero process exit is still `completed`. The engine applies configured checks to a completed observation and chooses `passed` or `failed`. `skipped` is engine-assigned only when a declared dependency or condition prevents execution and includes a reason. A configured student command timeout is recorded as `timed_out`; it affects the assessment only through an explicit rubric/check rule. Boot, image, agent, transport, protocol, host quota, or cleanup failure is `platform_error`/`resource_exceeded` and leaves assessment `pending`; it is never converted to a student zero. User cancellation is `cancelled`; an unclassified daemon/host loss is `interrupted` until reconciled. No process start means `exit_code` and `signal` are null.

### Durable evidence

Each run persists an immutable configuration snapshot and its SHA-256, schema/provider/protocol versions, project/run/platform-invocation/student-unit/test/task/attempt IDs, input file hashes and declared guest destinations, template and baseline handles plus provenance digests, and state/UTC timestamps. Each preparation/test attempt persists execution and assessment status, started/finished UTC timestamps, monotonic duration in milliseconds, nullable exit code and signal, timeout/resource flags, stdout/stderr byte counts and truncation flags, SHA-256 and content-addressed artifact references for complete captured streams, structured observations, diagnostics with stable code/retryable flag, and cleanup status. The run record includes the effective rubric/check outcomes and calculation inputs so a professor can explain or recompute an assessment.

Raw stdout, stderr, and declared test artifacts are stored as bounded binary blobs under the portable project/run directory; their metadata and hashes are in the YAML result record. Persist input and instructor-test hashes, not extra copies of secret-bearing bootstrap material. Never persist the session secret, host socket, credentials, or arbitrary guest host paths. VM disks and overlays are not assessment evidence and are deleted after acknowledged results; retaining them requires a separate explicit diagnostic policy with size/retention limits and must mark the run non-portable. Incomplete streams are marked incomplete and cannot be represented as a complete result.

## Alternatives considered

- Reuse `VMActivityProject` for sandbox runs: rejected because it models existing test/check/rubric activities and does not express platform lifecycle, input staging, network/resource policy, or typed guest tasks. It remains supported for legacy behavior.
- Keep both engine and runner provider traits: rejected because their lifecycle and error semantics already diverge.
- Use host SSH/TCP: rejected for this local KVM POC because it requires guest IP/port exposure and host-key/network configuration. A future remote provider needs a separate authenticated network transport decision.
- Use a permanent token in the template: rejected because clones would share credentials and stale copies could impersonate active runs.
- Let the agent assign pass/fail or calculate grades: rejected; the guest is an observation source, and the engine applies academic policy.
- Store full student VM disks by default: rejected because they are large, contain transient state and credentials, and are not needed to interpret captured test evidence.

## Consequences

The `thot sandbox run` local CLI currently reads the shared v2 core types and delegates exact test/rubric checks to `thot-engine`. It remains a local execution path until it is refactored onto the lifecycle `SandboxProvider` interface and integrated with durable engine/API run records. Existing `TestStatus` remains unchanged for legacy workflows; adapters must map only after an explicit compatibility rule. The agent, host provider, engine persistence, portable project layout, and API run model must share these v2 types or generated protocol fixtures.

The handshake secret's bootstrap device and guest process identity must be implemented before enabling untrusted scripts. VSOCK peer CID and the secret authenticate session scope, not guest software integrity. High-assurance grading against a hostile guest requires an external observer or stronger architecture and is outside this POC.

## Validation required

Before POC closure, validate configuration round-trip/unknown versions/invalid paths, Rust-to-agent shared protocol fixtures, exact frame bounds and partial reads/writes, token expiry/replay/cross-clone rejection, task deduplication, disconnect recovery, status mapping, durable result/blob hashes, pending grading on infrastructure failures, and unprivileged guest execution. The remote KVM integration must show one clean, one incorrect-result, and one syntax-error student result with separate clones and verified cleanup.

Review this ADR if a remote server provider is added, the v2 configuration replaces legacy activity YAML, authentication is changed, agents gain assessment authority, or the evidence/retention policy changes.
