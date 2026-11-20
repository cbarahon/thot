# Data model

Status: proposed v2. The following names are target contracts; They do not yet replace Rust types or the POC SQLite schema.

## Entities and identity

```mermaid
erDiagram
    PROJECT ||--o{ STUDENT : contiene
    PROJECT ||--o{ SUBMISSION : recibe
    STUDENT ||--o{ ASSIGNMENT : participa
    SUBMISSION ||--o{ ASSIGNMENT : asigna
    SUBMISSION ||--|{ INPUT : agrupa
    PROJECT ||--o{ RUN : ejecuta
    RUN ||--|{ REVISION : registra
    RUN ||--o{ EXECUTION_UNIT : organiza
    EXECUTION_UNIT }o--|{ SUBMISSION : selecciona
    EXECUTION_UNIT ||--o{ ATTEMPT : produce
    ATTEMPT ||--o{ ARTIFACT : conserva
    REVISION ||--o{ EXPEDIENT : genera
    STUDENT ||--o{ EXPEDIENT : recibe
    EXPEDIENT }o--o{ ATTEMPT : referencia
```

| Entity | Required fields and rules |
|---|---|
| Project | `schema_version`, `project_id` Stable UUID, Name, Configuration and Requested Policy |
| Student | `internal_id` UUID, non-empty unique external `id`, name; optional mail; `identity_field` choose import column |
| Submission | `submission_id` UUID, label, list of typed inputs; filename-independent identity |
| Input | `input_id`, type, selector/role, source, SHA-256, and size; Git adds URL without credential, ref requested and commit resolved |
| Assignment | `student_internal_id`, `submission_id`; unique pairs, N×N; an individual correction does not alter shared results |
| ExecutionUnit | `unit_id`, ordered set of deliveries/inputs, effective workflow and footprint; a unit shares a laboratory |
| Run | `run_id`, UTC date, status, current revision, units and snapshot references |
| Review | `revision_id`, optional parent, create/extend/requalify/fix operation, actor, reason, input/configuration hashes and result selection |
| Attempt | `attempt_id`, unit, step, repetition, attempt number, task/lab, footprint, status and result |
| Artifact | `artifact_id`, relative path, media type, bytes, SHA-256, provenance and visibility |
| File | student, review, deliveries, references to attempts, checks, rubrics, calculated grade, correction and effective grade |
| Overriding | base revision, student, destination check/rubric/evaluation, value, author, reason, date; always validated against scale |

Internal IDs do not contain routes. `run_id` combines readable label and UTC seconds; Exclusive creation rejects collision and proposes suffix only when necessary. A teacher is not trusted to be the only client. Folder names derived from tags replace unsupported characters with `_` and add `_1`, `_2` suffixes in collisions. The original mapping → route is stored; identity is never deduced from the route. External references of the students are preserved exactly, without destructively normalizing accents.

## Portable folder

```text
actividad/
  configuration.thot.yaml
  students.yaml                 # o students.csv
  submissions.yaml              # local or Git resources per submission
  assignments.yaml
  extern/                       # instructor scripts/configuration
  delivery/                     # originales importados
  overrides.yaml                # correcciones editables
  runs/<run-id>/
    run.yaml                    # status and pointer to committed revision
    snapshot/<revision-id>/     # configuration, roster and effective assignments
    inputs/<sha256>/            # materialized copies, including Git tree
    revisions/<revision-id>.yaml
    journal/events.jsonl        # secuencia confirmada, sin secretos
    tests/<unit-id>/<step-id>/<attempt-id>/
      result.yaml
      stdout.bin
      stderr.bin
      artifacts/
    expedients/<revision-id>/<safe-student-id>.yaml
    reports/<revision-id>/      # HTML, PDF, CSV y manifiesto de generación
    integrity.yaml             # export inventory and hashes
```

`configuration.yaml` is supported as legacy with notice; both names together are error. Roster YAML takes precedence over CSV with notice; the snapshot preserves which one was used and the effective conversion. `assignments.yaml` is canonical; JSON is converted explicitly and the coexistence is error. `submissions.yaml` groups various inputs and preserves roles such as `source`, `disk`, `report`. Selection by role/type and exact cardinality or rank, never by incidental directory order. The scheme validates extensions as hints, not as proof of the actual format.

The complete package includes all the tickets needed to repeat; a light export may exclude them, but it is marked `replayable: false` and lists absences. L1/L2 live disks are not portable results by default; They are preserved only as a requested and limited artifact. A materialized view can be regenerated without SQLite; All necessary data must be confirmed before the engine reports success.

## Results and statuses

`result.yaml` uses the [interfaces](api-spec.md) envelope. Distinguishes operational status from academic meaning. `completed` means the action is over and your observations are available; a process with exit code 2 can be `completed`. `passed/failed` belongs to the engine check. Additional terminal states: `timed_out`, `resource_exceeded`, `platform_error`, `cancelled`, `interrupted`, `skipped`. In these cases there may be no exit code; a zero is never invented. A skip records a dependency or condition that caused it.

Each output preserves engine UTC times and agent monotonic duration, raw stdout/stderr with size/hash and truncation flag, exit code or signal, structured diagnostics, and JSON observations. Filter Output only creates one view; It does not replace the original. Persistent structured observations allow checks without parsing human messages. Log limits include drain/discard to not block processes.

```mermaid
stateDiagram-v2
    [*] --> draft
    draft --> ready: validation and snapshot
    ready --> running: admission
    running --> cancelling: solicitud
    running --> finalizing: unidades terminales
    cancelling --> finalizing: confirmed stop
    running --> recovering: restart or loss
    recovering --> finalizing: reconciliar intentos
    finalizing --> finished: persistencia confirmada
    finished --> [*]
```

The overall result of the run and `cleanup_state: clean|pending|failed` are independent. `finished` does not mean all tests correct; the summary includes units with error and `assessment_state: complete|needs_review`. No final grade is published if evidence is missing due to technical failure, except for explicit teaching correction. In actual absence of delivery, `not_delivered_mark` or null applies. For statistical checks, define cohort closure barrier and deduplicate shared units; recalculate when incorporating new deliveries and register the set of samples.

## Editing, reuse and consistency

The teacher can modify settings, associations and `overrides.yaml`; validate shows differences and confirm applies to revision. The run is a modifiable folder: revisions are small records of provenance, not complete copies of all the evidence. Copying a read-only run allows you to consult it; expanding it requires a writable copy. Direct editing of results causes integrity discrepancy: the modification is allowed to be imported as an attributed correction, without presenting it as the agent's original output. Local hashes do not prove that the owner did not also rewrite the manifest.

Reusable fingerprint = hash of materialized inputs + normalized step specification + selected outputs/dependencies + image and agent digest + protocol version + relevant execution policy + seed/replay. It is not enough to find the same test name. Changing only evaluation does not invalidate the observations. Live state dependencies require reconstruction even if there are cached results. Default results are never reused between different runs or students.

A single writer blocks the project; Temporary files are written to the same volume, synchronized and renamed, publishing the revision pointer at the end. When recovering, unconfirmed temps are ignored, the journal is reconciled, and the inventory is verified. Readers see the latest confirmed review. Active work on network folders/cloud sync is not supported without reliable locking; a snapshot is exported for transport. Retention removes artifacts only when no preserved revisions reference them and updates the inventory transactionally.