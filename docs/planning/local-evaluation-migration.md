# Local evaluation migration backlog

Status: in progress. Initial S1, S2 and S3 work is present; S4 is partial. This backlog implements the [local evaluation architecture](../architecture/local-evaluation.md) and [ADR-0017](../decisions/0017-local-evaluation-cli.md). It is not a declaration that P0–P7 or multiboot qualification are complete.

## Product outcome and working method

A professor installs Thot once, selects a project, runs `thot run`, closes the terminal, follows understandable status, and receives portable evidence and grades. Thot manages the needed background workers and platform lifecycle without routine environment-variable setup. A failed operation always leaves a timestamped explanation and a bounded cleanup outcome.

Use short iterations, each ending in a demonstrable vertical slice and documentation update. The slices below are dependency order, not promised dates. Split a slice further if it cannot fit one iteration; do not defer all integration until the last slice. The backlog owner accepts behavior from professor-facing demonstrations, while engineering records evidence and remaining limits. Preserve the current release as the rollback path until migration qualification passes.

## Ordered slices

### S0 — Contract fixtures and migration inventory

Story: as a professor with an existing project, I can understand what changes and retain my existing results.

Tasks:
- Inventory every command/flag from the CLI source against the compatibility table; include remote host/port flags, destructive maintenance, assignment/import and report formats.
- Freeze v4 field names, explicit units, normalized Rust plan types, supported override keys and versioned defaults. Supply full simple-script and OVA example configurations as design fixtures before parser changes.
- Define run lifecycle, cleanup state, event envelopes, module ABI and provider submission/observation/cancellation behavior in shared contract fixtures. Reuse existing provider types rather than create another runner-specific interface.
- Capture current v2/v3 fixtures, grading outputs and archive verification as regression cases. Document which historical daemon fields cannot map to sandbox evidence.

Acceptance: valid/invalid fixtures cover resource mismatch, independent evaluation items, cross-platform dependency rejection and unknown fields. Every existing command has a destination or explicit compatibility route. No historical result is modified. These are contract choices, not a requirement to implement all providers.

Dependencies: none. Exit: reviewed schema and compatibility fixtures, resolving the mechanical choices listed below.

### S1 — Effective configuration and project diagnostics

Story: I can configure the assessment without copying provider boilerplate or exporting shell variables.

Tasks:
- Add versioned built-in/system/user defaults and deterministic resolution; parse `.env` as data. Retain current environment keys as documented advanced aliases where possible.
- Translate v2/v3 into the normalized model; implement v4 and non-destructive explicit migration. Derive template resources and limits from a single declaration.
- Implement `validate`, `config show --effective`, `doctor`, and template-based `init`; redact credentials and snapshot value provenance.
- Report storage amplification, host reserves, nested capability requirements and unavailable module dependencies before downloading large submissions.

Acceptance: a fresh shell with no Thot environment variables validates a small Alpine project; layer precedence and list replacement are tested. Old conflicting resources produce a field-specific error. Effective settings survive changed global defaults after acceptance. Migration leaves originals untouched.

Dependencies: S0. Demo: configure the Bash fixture with one resources block and diagnose a deliberately undersized OVA host.

### S2 — One background execution and evidence path

Story: I start an Alpine evaluation with `thot run`, close the terminal and return to durable progress/results.

Tasks:
- Extract orchestration from the Alpine foreground command into the engine/provider integration. Engine requests execution; provider retains all VM lifecycle operations.
- Add durable job acceptance, singleton startup/project locks and supervised local engine startup. Route `sandbox run` through the same submission path as a deprecated alias.
- Integrate delivery agents with engine intake; keep local default and Drive read-only access. Add a guided login command without copying tokens into jobs or running the whole CLI as root.
- Use the sandbox run repository for new sandbox jobs; expose legacy daemon histories through a reader/adapter. Do not convert them into hash-verified sandbox evidence by assertion.
- Add basic status, cancellation, report locations and conservative startup reconciliation. Preserve grading outputs and independent evaluation items.

Acceptance: exactly one job is accepted under concurrent CLI startup; repeated CLI connections do not duplicate tests. The three-student fixture runs in the background, closing the initiating terminal does not stop it, and saved grading matches the baseline. Delivery failure remains pending, not zero. Read-only status/report works with the engine stopped. This slice is not releasable without S3 supervision.

Dependencies: S1. Demo: start, detach, inspect, cancel and read an interrupted run without re-execution.

### S3 — Observable operations and bounded failures

Story: I can tell what Thot is doing and obtain an explanation when it cannot continue.

Tasks:
- Add concurrent guest heartbeat/control handling; negotiate protocol capabilities and invalidate incompatible agent baselines.
- Instrument intake, preparation, provisioning, boot, transfer, each test/tool operation, assessment and cleanup with the event envelope.
- Bound host subprocesses, partial transport frames, output queues, descendant-held pipes and process-group termination. Add provider supervisor leases and service restart reconciliation.
- Implement status/watch and log/follow from the journal; keep liveness distinct from operation progress and show absolute stage/deadline context.
- Persist unacknowledged provider results for reconciliation, deduplicate event delivery and preserve terminal events despite log pressure.

Acceptance: fault injection covers silent child, child retaining stdout after parent exit, stuck preparation, lost guest connection, stalled partial frame, engine death, provider death, full result disk and failed clone cleanup. Each has a bounded outcome, latest-operation evidence and correct pending assessment. A heartbeat cannot extend a task deadline. A cleanup failure blocks unsafe reuse. Log following consumes bounded memory and cannot stall execution.

Dependencies: S2. Demo: a deliberately hung fixture becomes a diagnosed timeout and its clone is reclaimed; no real Windows OVA required.

### S4 — On-demand installation and complete CLI compatibility

Story: installation leaves Thot idle, and all required services start and stop with my evaluation.

Tasks:
- Install dormant units and local endpoint permissions; validate terminal/logout independence in the chosen service-manager mode. Configure the bounded privileged provider helper during installation.
- Start only requested providers and the VirtualBox service only for legacy activities that need it. Reference-count workers; stop after evidence flush, cleanup and idle grace.
- Retain explicitly configured remote/always-on mode during transition. Upgrade existing installations without leaving competing engines or automatically terminating active runs.
- Complete command aliases, maintenance previews, saved-evidence regrade and report/export/import compatibility. Add readable startup failure diagnostics.

Acceptance: fresh install has no continuously running Thot engine/agent, local Alpine run needs no environment variables or routine sudo, workers stop after completion/cancellation, and unrelated libvirt services remain untouched. Concurrent projects do not terminate each other's workers. An engine crash leaves recoverable status. Legacy command/format coverage passes.

Dependencies: S3. Demo: install → run → status → report → idle on a clean Linux host.

### S5 — Reusable native modules

Story: I choose a test by name, and Thot stages the right executable and checks its dependencies.

Tasks:
- Implement manifest/catalogue resolution, `native-executable-v1` request/events/result validation, target selection, digest checks, assets and bounded subprocess execution.
- Port libvirt import/start/stop/destroy to shared Rust module logic with tool adapters. Preserve OVA manifest, OVF interpretation, disk conversion and lifecycle evidence.
- Build libguestfs and VirtualBox migration increments separately, maintaining old script/service adapters until parity is proven. Keep custom shell modules.
- Add target-specific CI artifacts and conformance fixtures; preparation installs the declared external dependencies.

Acceptance: unsupported target/tool fails before student execution; malformed module output is infrastructure failure; synthetic OVA lifecycle matches prior results. The same request fixtures execute on Alpine and a second supported environment before claiming cross-platform portability. No claim of full multiboot validation follows from import/start/stop/destroy alone.

Dependencies: S0 contracts and S3 supervision; may proceed alongside S4. Start with libvirt; additional module ports can ship incrementally rather than block a useful Alpine release.

### S6 — Professor workflow qualification and release

Story: I can follow the guide from a fresh laptop to a repeatable grade report.

Tasks:
- Update runnable examples only when the new parser is available; migrate the OVA guide, installer documentation and CLI reference together.
- Run local Bash fixture, synthetic OVA lifecycle, Drive ordinary-file download and reports/archive round trip through the unified CLI.
- On a sufficiently sized nested-KVM host, run the supplied real OVA and record capacity, timings, firmware/boot outcomes and cleanup. Qualify EFI, OS boot and shared-data checks separately as they are implemented.
- Exercise old project/history compatibility and document downgrade/rollback steps, pending assessments and unsupported environments.

Acceptance: professor guide works without hidden shell setup; raw/effective configuration and final evidence are portable; archive import and regrade do not execute tests; no unexplained active jobs or leftover student VMs. Release notes distinguish passing synthetic lifecycle, real OVA lifecycle and full multiboot qualification. A blocked Windows test remains an explicit open qualification, never an inferred pass.

Dependencies: S4 and first libvirt slice of S5. Additional provider qualification remains its own backlog.

## Definition of done for every implementation slice

- Commands and failure messages demonstrate the user story; current-status docs distinguish shipped behavior from target behavior.
- Focused tests cover changed contracts and material failures; Rust changes pass formatting, Clippy and appropriate tests. VM lifecycle changes also have host smoke evidence with version/capacity recorded.
- No secrets or actual student submissions enter commits. Evidence samples use synthetic data.
- Historical data remains readable; migrations write a new destination or explicit backup. No automatic re-execution on upgrade, import or recovery.
- Rollback and known limits are recorded. Passing unit tests alone is not hardware qualification.

## Remaining implementation choices and risks

| Choice/risk | Default direction and closure point |
|---|---|
| User engine lifetime across logout | Prefer user service manager with verified persistent lifetime; provide system transient-service mode where unavailable. Prove on supported fresh distribution in S2/S4 before claiming background reliability. |
| Privileged KVM operations | Reuse existing provider logic behind bounded installed helper; do not expose arbitrary project-root commands. Final helper packaging is S4 work. |
| Exact v4 grammar and Rust names | Follow the one-resources-block design; freeze full fixtures in S0. The architecture fragment is deliberately not a parser contract. |
| Wire compatibility | Test additive heartbeat/cancel negotiation; bump protocol if old sequencing cannot safely interoperate. Decide in S0, implement S3. |
| Dynamic native dependencies | Select musl/static only when supported by dependencies; record target/tool ABI and prove packaging in S5. |
| Legacy reports | Map fields explicitly; retain readers and unavailable indicators. No fabricated equivalent evidence. Close format matrix in S4. |
| Drive OAuth distribution | Target a developer-registered Desktop client distributed with Thot, with per-professor consent and an override for testing or self-hosting. Client bundling and environment overrides are implemented; a guided login remains to be implemented. See the [developer guide](../guides/google-drive-developer.md). |
| Full multiboot assessment | Still requires actual layout/GRUB/OS/shared-volume test logic and suitable hardware. Track in the existing multiboot plan; do not bury it inside CLI success. |

## Rollout and rollback

Keep v2/v3 and current archives readable throughout. Introduce the new engine path behind an explicit migration opt-in until S3/S4 pass; do not silently switch a foreground command before documentation and the compatibility warning ship. Upgrade only with no active run or defer activation. Record schema/module/default versions in every new run. Keep previous binaries/configuration available; an older binary must reject unsupported v4 instead of misreading it. Downgrading must not rewrite or delete newer results. Remove deprecated aliases only in a later announced breaking release after equivalent workflows and migration examples are established.
