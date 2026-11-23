# ADR-0013: Independent evaluation items for sandbox schema v3

- Status: Accepted for implementation
- Date: 2026-11-23
- Scope: local sandbox project contract and its portable result reports

## Context

The multiboot activity has two independently meaningful marks: disk/system
layout and runtime boot/shared-data behavior. Runtime tests can be blocked by
provider or guest failures even when static layout evidence is complete. A
single weighted sum incorrectly forces either a fabricated zero or a blank
overall result that hides completed work.

## Decision

Keep `thot.dev/v2` behavior unchanged: it has exactly one weighted rubric.
Introduce `thot.dev/v3` with `evaluation.items[]`. Each item has a stable ID,
human-readable title, scale, weighted-sum calculation, and criteria referencing
named checks. Item IDs are unique within the platform and criteria must total
the item's scale.

Evaluate items independently. If a referenced test has infrastructure-pending
evidence, the affected item has `status: pending` and `grade: null`; complete
items keep their calculated grades. The overall grade is null until every item
is complete. Academic failures and dependent skips remain zero-valued evidence
for the relevant item. A dependent task skipped after infrastructure-pending
evidence also remains pending. Cleanup status is separately recorded and a
cleanup failure prevents the run from being marked complete.

Portable results use `thot.result/v3` for this shape, preserve item-level
criteria and marks, and leave the existing v2 result contract available to
existing projects. Reports are read only after `ProjectRunRepository::verify_run`
confirms the committed folder and its evidence hashes.

## Consequences

- v2 configurations and run outputs remain compatible.
- A v3 project requires one or more evaluation items and no legacy `rubric`
  field; unknown fields continue to fail validation.
- The current Alpine runner implements weighted-sum evaluation for v3 items.
- Regrading edits, professor corrections, cohort statistics, and cross-platform
  orchestration are not introduced by this ADR.
- This contract does not make OVA partition/boot tests executable. Those need
  reviewed modules, capabilities, staging space, and P2 provider enforcement.

## Validation

Core integration fixtures validate v2/v3 schema separation and reject
duplicate item IDs. Engine tests show a completed item retaining its grade
while another item remains pending. CLI report tests cover multiple items,
Unicode, and HTML escaping.
