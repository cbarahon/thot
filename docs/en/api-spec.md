# Contracts and interfaces

Status: Design Specification v2. The following endpoints and YAML are proposed, not currently available commands. The existing `/api/v1` API and `thot-virtualbox-agent` HTTP v1 protocol do not change as of this writing. They will be kept separate during migration.

## Common rules

JSON UTF-8 for messages, secure YAML without executable tags for documents. `schema_version: thot.dev/v2` in configuration, `protocol_version: 2` in messages. Reject unknown major version; Optional new fields require advertised capacity. Duplicate/unknown YAML keys, expansive aliases, non-finite numbers, and escape paths are errors. Initial limits: JSON message 1 MiB, YAML document 4 MiB, depth 32, 1000 steps per workflow, blob chunk 256 KiB (before base64 encoding); values ​​reviewable via version/policy. Logs and blobs are never unlimitedly embedded in JSON.

ID UUID or generated opaque identifier, RFC 3339 UTC dates, duration in whole milliseconds, quantities with explicit unit MiB/GiB. SHA-256 over blob bytes; Object traces over RFC 8785 canonical JSON. YAML is not canonicalized directly. Common diagnosis: `code`, `message` understandable, `field`, `retryable`, `technical_detail` optional worded.

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

| Interface | Operations and responsibility |
|---|---|
| ProjectRepository | `validate`, `snapshot`, `lock`, `import`, `export`; without running content |
| ResultStore | `append_attempt`, `commit_revision`, `read`, `verify_integrity`; sole final results writer |
| SandboxProvider | `capabilities`, `preflight`, `create`, `status`, `renew`, `stop`, `destroy`; never evaluates |
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