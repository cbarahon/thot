# Contracts and interfaces

Status: Mixed contract reference. The P1 Rust sandbox contract and run repository are implemented as initial internal APIs; they are not yet wired to a production sandbox provider or public HTTP endpoint. The YAML examples and most workflow DSL remain target design. The existing `/api/v1` API and `thot-virtualbox-agent` HTTP v1 protocol do not change.

## Common rules

JSON UTF-8 for messages, secure YAML without executable tags for documents. `schema_version: thot.dev/v2` in configuration, `protocol_version: 2` in messages. Reject unknown major version; Optional new fields require advertised capacity. Duplicate/unknown YAML keys, expansive aliases, non-finite numbers, and escape paths are errors. Initial limits: JSON message 1 MiB, YAML document 4 MiB, depth 32, 1000 steps per workflow, blob chunk 256 KiB (before base64 encoding); values ​​reviewable via version/policy. Logs and blobs are never unlimitedly embedded in JSON.

ID UUID or generated opaque identifier, RFC 3339 UTC dates, duration in whole milliseconds, quantities with explicit unit MiB/GiB. SHA-256 over blob bytes; Object traces over RFC 8785 canonical JSON. YAML is not canonicalized directly. Common diagnosis: `code`, `message` understandable, `field`, `retryable`, `technical_detail` optional worded.

## Delivery-agent contract

The current CLI exposes this contract locally; no public HTTP delivery-agent endpoint is introduced yet. Implementations are selected from project configuration and must be read-only:

```text
list() -> DeliveryInfo[]
get(name: exact_name, destination: private_staging_path) -> AcquiredDelivery
```

`DeliveryInfo` contains `name`, `size_bytes`, optional RFC 3339 `created_at` and `modified_at`, optional stable `provider_id`, `revision`, `mime_type`, and provider checksum plus its algorithm. `get` selects an exact name and rejects ambiguous matches; `get_by_id` selects the stable source ID when supported. Run provenance binds the selected metadata to the acquired byte count, acquisition time, and SHA-256. Google Drive uses its file ID, head revision/version when offered, and MD5 metadata when offered; the acquired bytes are independently SHA-256 hashed. `delivery_limits.max_bytes` and `max_files` default to 128 MiB and 10,000 files and can be configured up to 64 GiB and 100,000 files. Drive data is streamed in bounded chunks to private staging; it is not accumulated in memory. Local paths and OAuth credentials never enter the guest or the project snapshot. Missing/ambiguous selectors, unsupported Drive-native documents, permissions, quotas, and download failures are intake/infrastructure errors, never student assessment failures.

```yaml
delivery_limits:
  max_bytes: 8589934592 # 8 GiB; must fit available host staging storage
  max_files: 10000
```

When `deliverable_id` is used in an assignment, it must exactly match the
`provider_id` displayed by `thot deliveries list`; the professor should retain
that project assignment snapshot with the evaluation record.

The Alpine smoke CLI accepts the strict `thot.dev/v2` schema and the explicit
`thot.dev/v3` evaluation-item extension in
`thot_core::sandbox_project::SandboxProject`; v2 keeps its one weighted rubric,
while v3 uses independent `evaluation.items[]`. The typed provider request is a
separate internal versioned Rust contract (`SANDBOX_CONTRACT_VERSION = 1`),
not a YAML replacement. Callers must validate the project before invoking a
provider. V3 does not yet provide a managed provider or general workflow DSL.

## Preparation and portable test modules

Preparation installs only instructor-declared packages from the platform's
configured repositories and runs separately declared, project-relative
instructor scripts. Package managers are platform-specific (`apk` for Alpine,
`apt` for Debian, and image build/package rules for Docker). Script payloads
are copied into the preparation environment, hashed in the project snapshot,
run in order with individual and aggregate timeouts, restricted network policy,
bounded stdout/stderr, and no student delivery content. The completed package
inventory, repository identity, script digests/results, image digest and
prepared-baseline identity are recorded. Failed preparation blocks student
execution. Preparation scripts are trusted professor code and are not
downloaded from a student's Drive delivery.

The engine resolves a configured test module to an immutable package by ID,
version and SHA-256 digest. It checks the platform agent's advertised
capabilities before provisioning, submits the package and its declared assets to the provider. The provider stages them in the clone and verifies their digests before use.
The platform-independent module request/result contract is:

```json
{
  "module": {"id": "thot.ova-layout", "version": "1", "sha256": "<digest>"},
  "task_id": "<opaque-id>",
  "student_unit_id": "<opaque-id>",
  "inputs": [{"role": "student-deliverable", "sha256": "<digest>", "path": "/work/input/submission.ova"}],
  "capabilities": ["artifact.read", "disk.inspect"],
  "limits": {"timeout_seconds": 600, "memory_mib": 1024, "output_bytes": 1048576},
  "parameters": {"required_roles": ["efi", "archlinux", "alpine", "windows", "shared"]}
}
```

The module returns normalized observations and evidence references. The module
cannot choose new tests, access the roster/engine API, write expedients, or
assign grades. Platform adapters provide only typed capabilities advertised
by the active agent. For example, `disk.inspect` may use libguestfs inside the
Alpine clone; `nested-vm.create`, `nested-vm.console` and `guest-probe` are
available only when explicitly configured and admitted. There is no arbitrary
host shell or host hypervisor socket capability. Missing capabilities cause
preflight rejection or an explicit `unsupported` status; they do not trigger a
fallback platform.

The module package format/ABI (for example, a bounded executable protocol or
WASM component), signing policy, runtime compatibility and dependency bundling
are unresolved. They must be decided before implementation. The invariant is
that a single module package carries common assessment logic across compatible
platforms, while each platform implements or declines the same versioned
capability contract. Platform adapters may differ; grading semantics do not.

For `kvm-alpine`, the platform section includes professor-selected outer VM
vCPU, memory, storage and nested-virtualization settings. Host preflight checks
hardware nesting and administrator resource ceilings. The project preparation
phase can install Alpine repository packages and run an instructor script to
install a tool unavailable from those repositories. Each student's OVA and
the pinned module package are transferred only after a fresh linked clone is
created from the prepared snapshot.

## Example declarative configuration

Complete example of the requested flow at the contract level: the digests and the referenced key are values ​​provided by the template/installation. The `image_ref` type must resolve to an installed digest before executing. The commands are argv lists, without shell interpolation. `${...}` is a typed reference and is only allowed in fields that declare it; it does not evaluate expressions or command substitutions.

```yaml
schema_version: thot.dev/v2
project_id: 2d4d004f-4c0f-46fb-b69a-77591364d814
name: Laboratorio de redes
roster:
  identity_field: email
submissions_file: submissions.yaml
assignments_file: assignments.yaml
sandbox:
  profile: nested-lab
  image_ref: linux-lab-2026.11
  cpu: 4
  memory_mib: 12288
  disk_gib: 40
  deadline_seconds: 1800
  network_policy: acquire-then-offline
workflow:
  - id: checkout
    action: git.checkout@2
    inputs: {role: source, kind: git, cardinality: 1}
    with: {revision: submitted, destination: repo}
    timeout_seconds: 120
  - id: files
    action: fs.inspect@2
    requires: [checkout]
    with:
      root: '${steps.checkout.outputs.tree}'
      required_paths: [Makefile, scripts/verify.sh]
    timeout_seconds: 10
  - id: code
    action: exec.run@2
    requires: [files]
    with:
      cwd: '${steps.checkout.outputs.tree}'
      argv: [python3, /instructor/extern/check_code.py]
    timeout_seconds: 60
  - id: create_lab
    action: exec.run@2
    requires: [code]
    with:
      cwd: '${steps.checkout.outputs.tree}'
      argv: [make, lab-up]
    timeout_seconds: 600
  - id: discover
    action: kvm.discover@2
    requires: [create_lab]
    with: {scope: laboratory, names: [router, client]}
    timeout_seconds: 30
  - id: properties
    action: kvm.inspect@2
    requires: [discover]
    with: {resource: '${steps.discover.outputs.resources.router}'}
    timeout_seconds: 30
  - id: ready
    action: ssh.wait@2
    requires: [properties]
    with:
      resource: '${steps.discover.outputs.resources.router}'
      credential_ref: lab-router
    timeout_seconds: 90
  - id: verify
    action: ssh.exec@2
    requires: [ready]
    with:
      resource: '${steps.discover.outputs.resources.router}'
      credential_ref: lab-router
      argv: [sh, /opt/activity/verify.sh]
    timeout_seconds: 60
  - id: stop
    action: exec.run@2
    after: [verify]
    always: true
    with:
      cwd: '${steps.checkout.outputs.tree}'
      argv: [make, lab-down]
    timeout_seconds: 60
checks:
  - id: code_valid
    source: code.exit_code
    predicate: {equals: 0}
  - id: router_memory
    source: properties.observations.memory_mib
    predicate: {greater_or_equal: 1024}
  - id: functional
    source: verify.exit_code
    predicate: {equals: 0}
rubrics:
  - id: lab
    checks: [code_valid, router_memory, functional]
    calculation: weighted_avg
    weights: [0.2, 0.2, 0.6]
evaluation:
  - id: final
    rubrics: [lab]
    calculation: avg
    multiplier: 10
```

`submissions.yaml` declares for each delivery `submission_id`, `inputs: [{input_id, role: source, kind: git, url: https://github.com/example/student-work.git, revision: <ref>}]`; the engine sets commit and hashes after acquiring. The actual identifiers and refs are entered by import, not by writing a template per student. The image provides Python, make, KVM tools and agents; the profile connects `make lab-up` to libvirt **from L1**. `kvm.discover` would invent domains from that lab, requires unique names, and produces verified references; it does not accept an arbitrary host address. `ssh.wait` resolves private endpoint and host key using resource registration; If the image does not allow a reliable key to be established, the preflight of the activity fails. The remote script must have been installed by the activity; SSH doesn't mean the host magically copies it.

Each action defines input scheme, output, capabilities, effects, cardinality and operational success result. For `exec.run`/`ssh.exec`, `requires` considers success only `completed` and exit code 0; Additional accepted codes are declared with `accepted_exit_codes`. `fs.inspect` returns found/missing list and its success requires the required ones to be present. A check can inspect a completed action with exit not accepted. `after` waits for any terminal state. `always` omits the success requirement, but requires available entries; if they do not exist, it is skipped with reason. Outer destruction occurs even if `stop` cannot be executed.

The SSH agent converts argv to a remote command using POSIX escaping checked by argument, without concatenating unescaped values; only supports guest Linux with supported shell. Complex scripts are distributed as files.

There are no loops or scripts embedded in the initial DSL; Bounded repetitions, explicit dependencies and teaching scripts in `extern/` solve advanced cases. The engine validates references and types before executing. Effective limits are the minimum administrator policy, run and step. Initial values ​​of the ordinary profile: 60 s/step if not declared, 1800 s/laboratory, 10 MiB per stream, 100 MiB of artifacts/step, 1000 files/step and 256 processes/container. The nested profile requires specific fees for its services; they are still contained in RAM/CPU/disk/L1 time.

## Internal engine ports

The shared `thot_core::sandbox_contracts` module defines provider kinds,
capability versions, administrator resource ceilings, preparation inputs and
results, immutable module references/manifests, typed test operations and
observations, delivery provenance, run status, artifacts, journal events and
revision metadata. `TemplateHandle`, `BaselineHandle`, and
`StudentEnvironmentHandle` are distinct opaque types. They contain provider
identity plus generation and ownership identifiers, not project-supplied VM
names or paths. `HostCapabilities::validate_admission` rejects provider or
resource mismatches and required-but-unavailable nested virtualization before
provisioning; missing module capabilities are explicit errors with no
fallback.

`thot_runner::sandbox_provider::SandboxProvider` is the engine-facing async
provider trait: `kind`, `preflight`, `prepare`, `run_deliverable`, `status`,
`cancel`, and `reconcile`. The provider owns all template, baseline,
student-environment, nested-resource, and cleanup lifecycles. The current
KVM-Alpine smoke application has not yet been refactored to this trait; it is a
contract implementation, not evidence that the trait is backed by a production
provider.

P1's module manifest fixes metadata identity (`module_id`, immutable `version`,
manifest version, package SHA-256, typed required capabilities, and assets with
path/size/SHA-256). The fixture package is metadata-only. It proves shared
manifest validation and digest handling in engine and guest-agent crates; it is
not an executable module. The first Alpine executable format is selected in
ADR-0012 below; signing policy, dependency bundling, archive assets and other
platform loaders remain open.

The first Alpine P3 module runtime is now specified by
[ADR-0012](../decisions/0012-alpine-preparation-and-posix-test-modules.md):
one SHA-256-pinned POSIX shell script per package, argv passed as script
arguments, required capabilities advertised during the VSOCK v2 handshake,
and agent-side digest verification immediately before execution as the
unprivileged student account. Project preparation names project-relative
script files; Thot hashes them before baseline reuse, runs them only in the
preparation VM with per-script timeout, and archives bounded output and the
installed `apk` inventory. The smoke path streams staged files in 64 KiB
chunks with per-chunk acknowledgements and final SHA-256 verification. This
ABI is implemented only by the Alpine guest agent. Bundled module assets,
Debian/Docker adapters, output ACKs, resume and in-flight cancellation remain
open.

## Independent evaluation items (v3)

Schema v3 keeps named checks local to their platform and declares one or more
independent items under `evaluation.items`. Each item defines a title, scale,
weighted-sum criteria and check references. A test result with an infrastructure
`pending` assessment keeps the affected item ungraded; completed items remain
visible and graded. Academic failures and dependent skips count as completed
negative evidence, while a skipped test whose dependency was infrastructure
pending remains pending. The overall grade is null while any item is pending. V2
projects still use the required single `evaluation.rubric`; see
[ADR-0013](../decisions/0013-v3-independent-evaluation-items.md).

The local CLI can export a complete verified run folder to a stored ZIP and
import it into a new private directory. Import rejects path traversal,
platform-specific separators, symlinks, duplicate entries and expansions over
128 GiB, then verifies the committed run before it is accepted. The archive
contains evidence and project snapshots, not guest disks or credentials; it
does not make the run replayable. See
[ADR-0014](../decisions/0014-portable-sandbox-run-archives.md).

## Sandbox platform contract

The project-level `sandbox_platforms` list is the only place that chooses an execution boundary. A platform object has a stable `id`, `kind` (`kvm-debian`, `kvm-alpine`, `docker`, or `host`), kind-specific parameters, `preparation`, ordered `tests`, resource/network policy, and cleanup policy. `kvm-debian` and `kvm-alpine` share KVM provider operations but have separate image and guest-agent profiles; Alpine's proposed lifecycle is documented in [KVM_ALPINE_POC.md](../implementation/kvm-alpine.md). A test definition includes `id`, `action`, `timeout_seconds`, input selectors, same-platform `requires`/`after` edges, and output declarations. Unknown platform kinds, duplicate IDs, test membership in more than one platform, missing references, dependency cycles, and cross-platform edges are validation errors.

### Engine-facing provider interface

| Operation | Responsibility |
|---|---|
| `configure(platform, project_snapshot)` | The provider validates capabilities and ensures its prepared environment is ready. |
| `run_deliverable(attempt_id, input, modules, ordered_tests)` | The engine sends the bounded deliverable bytes and digest, module references or blobs, and the test plan. The provider returns an attempt receipt and owns execution through cleanup. Repeated submission with identical ID and payload returns the same attempt. |
| `events(attempt_id, after_sequence)` | The provider streams progress, bounded results and artifact references. The engine acknowledges evidence after durable storage. |
| `status(attempt_id)`, `cancel(attempt_id)` | Query or cancel an attempt. The provider performs cleanup after success, error or cancellation and reports both the execution outcome and verified cleanup state. |

The engine requests the next assigned item from the delivery agent after the
current attempt ends with confirmed cleanup. If contact is lost, it queries
the existing attempt. The provider reconciles its own resources. Pending or
failed cleanup pauses dispatch and remains visible in the expedient.

The `host` provider is deliberately not a sandbox. It accepts only registered action types, such as bounded PDF metadata extraction or archive inventory, whose adapters do not execute student-controlled code. Arbitrary `exec`, shell, package installation, student-supplied plugins, or writable access to the instructor's project directory is forbidden on this provider. An installation that declares itself to run inside an outer VM must pass an administrator-owned capability check; project YAML cannot assert that fact.

KVM and Docker providers create one environment per student or explicitly assigned group, not one mutable environment shared by the class. `student_unit_id` is stable for the run. KVM preparation is shared only through a read-only baseline; student writes go to a linked overlay that is destroyed after evidence collection. The base image, preparation manifest, snapshot identity, parent chain, and guest agent version are recorded. A linked clone is not portable alone: export must include the base image and snapshot or mark the run non-replayable.

Preparation output and test output are separate result classes. Preparation can declare installed package inventory, scripts run, hashes, stdout/stderr, exit code, elapsed time, and errors, but it cannot add a student test mark. Execution outcome distinguishes `completed`, `timed_out`, `resource_exceeded`, `platform_error`, `cancelled`, `interrupted`, and `unsupported`; `exit_code` is nullable. A completed non-zero exit code is an observation for engine checks, not automatically an infrastructure failure. Assessment is separate: `passed`, `failed`, `skipped`, or `pending`; cleanup is separately `clean`, `pending`, or `failed`. A skipped test records a reason and does not fabricate process output. Stdout/stderr and artifacts are bounded references containing relative path, byte size, media type, SHA-256, and completeness/truncation state. Diagnostics have a stable code/message plus retryability and optional technical detail.

Control is engine-initiated and scoped to a platform run, student unit, and task ID. For the local KVM Alpine POC, the guest agent initiates a local VSOCK connection to a host listener, verifies the assigned peer CID and a per-clone, one-use credential delivered through a protected bootstrap device, and negotiates the v2 protocol. Do not embed a long-lived shared token in the base image. This scopes a session but does not attest guest integrity. A future remote provider requires a separate network-transport decision. The engine can cancel and query progress but the agent cannot access engine APIs for roster, grading, or expedients. Use bounded messages and artifact transfer from the protocol section above. Host platform actions run locally through the same task/result envelope, with their weaker assurance recorded. See [ADR-0008](../decisions/0008-kvm-alpine-poc-contract.md) for the frozen POC details.

Platform independence is enforced both in configuration validation and at runtime: result references carry `platform_id`; an edge is legal only when producer and consumer have the same `platform_id`. Engine checks/rubrics are permitted to consume finalized results across platforms after execution completes. They are not test actions and cannot create a live bridge. If a future use case requires platform-to-platform exchange, define an explicit export/import artifact edge with a copied, hashed, read-only payload and a new threat review; no such bridge is part of v2.

### Platform-specific configuration questions

| Question | Initial recommendation | Still to decide |
|---|---|---|
| KVM VM identity/resources | `image` digest, name prefix, vCPU, RAM MiB, disk GiB, boot timeout, snapshot name | Supported libvirt connection and permissions; linked-clone format; whether snapshots are disk-only or include memory (recommend disk-only) |
| Debian preparation | Version-pinned packages and instructor scripts, with per-step timeout and captured package inventory | Offline mirror versus controlled online access; package pinning policy and reproducibility guarantee |
| Guest agent | Signed/versioned package in image, engine-authenticated session, no grade authority | Bootstrap mechanism, update/rollback policy, and how to verify guest identity |
| Docker | Immutable image digest, per-container CPU/RAM/PID/disk/output limits, no privileged mode or host sockets | Whether initial Docker provider runs on host or only inside KVM; rootless/runtime choice and storage quota enforcement |
| Host actions | Allowlist of read-only adapters, input/output paths copied or opened read-only | Exact action catalogue and administrator override mechanism; no arbitrary project-defined executable |
| Cleanup | Always destroy student environment after artifact ACK; retain only bounded diagnostics | Quarantine duration and operator workflow for unkillable VM/container |

| Interface | Operations and responsibility |
|---|---|
| ProjectRepository | `validate`, `snapshot`, `lock`, `import`, `export`; without running content |
| ResultStore | `append_attempt`, `commit_revision`, `read`, `verify_integrity`; sole final results writer |
| SandboxProvider | `configure`, `run_deliverable`, `events`, `status`, `cancel`; owns environment lifecycle |
| AgentDispatcher | `submit`, `status`, `cancel`, `events`, `upload_blob`; takes tasks to an authorized laboratory |
| Evaluator | confirmed results + rules + corrections → checks/rubrics/notes; no student I/O or network |

## Engine, supervisor and laboratory

Engine → Unix socket supervisor with permissions and peer UID verification; supervisor → coordinator by virtio-vsock, associated with the CID, laboratory and startup nonce. Handshake negotiates protocol/capabilities and ephemeral session token, transmitted over dedicated boot channel without saving it on portable disk. The host only accepts the connection from the assigned CID. This authentication attributes a session; it does not attest to the integrity of a committed L1. The supervisor is not exposed to the student or the network.

JSON messages with prefix length of 32 big endian bits, maximum 1 MiB, `request_id`, `type`, `protocol_version`, `laboratory_id`, `lease_epoch` and `payload`. Capabilities announces actions/versions/schemes, digests, effective nesting and quotas. Preflight resolves resources and rejects shortages before starting deliveries. Messages: `hello`, `capabilities`, `task.submit`, `task.status`, `task.cancel`, `event`, `lease.renew`, `blob.begin/chunk/commit`, `laboratory.stop/destroy`. The supervisor link and the coordinator link use different allowlists; the coordinator is not given authority to create L0 resources.

Task payload (in production all opaque IDs are generated):

```json
{
  "task_id": "task-opaque", "attempt_id": "attempt-opaque",
  "run_id": "run-opaque", "revision_id": "revision-opaque", "unit_id": "unit-opaque",
  "action": "exec.run@2",
  "inputs": [{"blob_id": "blob-opaque", "sha256": "<64 hex>", "target": "repo"}],
  "parameters": {"cwd": "repo", "argv": ["make", "lab-up"]},
  "limits": {"timeout_seconds": 600, "stdout_bytes": 10485760},
  "resource_refs": [], "retry_policy": "never"
}
```

No host routes or credentials are sent in this payload. Paths are relative to the workspace or verified resource references. Task identity persisted before submission. Same `task_id` and hash returns the existing state; different payload is conflict. Transport timeout does not imply that the process did not start: query, do not resend with a new ID. Each new reboot/lease invalidates previous epoch; Late result remains as a diagnosis and never replaces the confirmed one. In new L1 another attempt is generated and the state is rebuilt, not exactly an execution is asserted.

Heartbeat every 10 s, lease 30 s, events with increasing sequence and ACK after persist; reconnect requests next sequence and deduplicates. Pending results/blobs are kept until ACK or storage limit. Expiration stops L1 via external watchdog. Cancel is idempotent, grants 10s and forces stop before 30s; if it can't kill the entire task, it destroys L1. A `make` or SSH with unknown effects is not automatically retried. Only actions declared idempotent can be retried in a limited manner (two initial attempts maximum).

## Normalized result

```yaml
protocol_version: 2
task_id: task-opaque
attempt_id: attempt-opaque
status: completed
process:
  exit_code: 2
  signal: null
started_at: '2026-11-15T10:00:00Z'
finished_at: '2026-11-15T10:00:03Z'
duration_ms: 3000
stdout: {blob_id: stdout-opaque, bytes: 120, truncated: false, sha256: '<64 hex>'}
stderr: {blob_id: stderr-opaque, bytes: 80, truncated: false, sha256: '<64 hex>'}
observations: {}
resources: []
artifacts: []
diagnostics: []
```

`platform_error` adds diagnostics as `IMAGE_BOOT_FAILED`, `AGENT_LOST`, `DISK_QUOTA`, `SSH_AUTH_FAILED`, `PROTOCOL_INVALID` or `CLEANUP_FAILED`; the engine classifies if it requires revision. Timeout of an academic command can cause a failed check if the rule expressly declares it; a platform startup timeout leaves a pending note. Invalid results are recorded as a protocol error, never as a success. Streams can be empty; process fields are null for non-process actions.

The advertised resources carry `resource_id`, type, lab/task owner, schema attributes, and private endpoint, with no host paths. The supervisor/registration validates membership. The artifacts are uploaded by chunks with offset and hash, quota reservation and closure with total hash. The engine copies staging without following links, validates content/size and commits atomically; it does not accept write paths chosen by an agent. The chunks travel as base64 JSON messages and respect the 1 MiB frame limit; a large blob requires several frames. Only final ACK allows linking them from `result.yaml`. Partial streams remain accessible with incomplete marking.

## Proposed local user API

`/api/v2` HTTP JSON API over Unix socket by default; A loopback adapter requires local token, file permissions and origin/CSRF protection if it includes a browser. No paths support shell or arbitrary paths outside of registered projects. Registering a folder requires local authenticated user access; future multi-user authorization is out of scope.

| Method and route | Main petition | Answer |
|---|---|---|
| POST `/projects` | authorized directory or validated import | 201, ID and warnings |
| POST `/projects/{id}/validate` | current configuration | 200, errors/capabilities/estimation; without student effects |
| POST `/projects/{id}/runs` | unit selection, policy | 202, run and operation |
| GET `/runs/{id}` | — | 200, progress, pending notes and cleaning |
| POST `/runs/{id}/cancel` | reason | 202, cancellation status |
| POST `/runs/{id}/extend` | selected changes, base revision | 202, new review and reuse plan |
| POST `/runs/{id}/regrade` | rules, base review | 202, review without taking actions |
| POST `/runs/{id}/overrides` | corrections, base revision | 201, revision validated |
| GET `/runs/{id}/events?after=N` | cursor | SSE with sequential ID and replay; equivalent polling |
| GET `/runs/{id}/expedients/{student_id}` | optional review | 200, file and provenance |
| GET `/artifacts/{id}` | Authorized ID | bytes, type-safe and hash; default download |
| POST `/projects/{id}/exports` | full/shareable/light and review | 202, package and inventory |
| POST `/runs/{id}/cleanup` | pending resource | 202, cleaning of own resources |

Mutations require `Idempotency-Key`, persisted with hash and response for the life of the run; `extend`, `regrade` and fixes require base revision/`If-Match`. Version/lock conflicts respond 409/412. Other responses: 400 Invalid JSON, 401/403 authentication/scope, 404 ID not visible, 413 limit, 422 validation/capability, 503 admission not available. Error: `request_id`, `code`, `message`, `errors[]` with field and solution. Asynchronous operations include `operation_id` and queryable URI; Progress does not equate to confirmation of result. SSE never transports secrets or binary streams.

Import/export is a file contract: relative paths, SHA-256 inventory, sizes and version. Reject links, traversals, devices, excessive files and decompression bombs. Import does not run or install images automatically. Export excludes credentials and temporary data; Full reports contain personal data and require protection.
