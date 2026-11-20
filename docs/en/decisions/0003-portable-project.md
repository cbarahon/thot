#ADR-0003: Portable folder as source of truth

Status: **proposed**, 2026-11-15. Scope: target platform v2; It does not imply implementation or definitive approval.

## Context

The teacher needs to move, archive, inspect and correct an assessment without relying on a database installation.

## Proposed decision

YAML and native evidence in folder with snapshots, manifests and relative references; SQLite only rebuildable index after migration. Maintain CSV/PDF/HTML for exchange.

## Alternatives

Alternatives: central database as authority or only final exports. Both make full portability or requalification difficult.

## Consequences

Requires exclusive writer, recoverable commit, inventory and quotas; Active editing from systems without reliable locking is not supported. Packages contain personal data.

## Validation and review

P1/P7 must demonstrate transfer, recovery, reconstruction and importation without loss. Review this ADR if those conditions are not met. See [architecture](../architecture.md), [contracts](../api-spec.md) and [open issues](../PROJECT_PLATFORM_PROPOSAL_REVIEW.md).