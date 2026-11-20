#ADR-0006: Separate rating observation

Status: **proposed**, 2026-11-15. Scope: target platform v2; It does not imply implementation or definitive approval.

## Context

A platform failure, a program with a non-zero exit code, and an unmet academic criterion have different meanings.

## Proposed decision

Agents return facts and diagnoses; engine calculates checks, rubrics and notes from persisted evidence. Infrastructure error leaves pending unless explicit intervention.

## Alternatives

Alternatives: agent returns final grade or all errors are equal to zero. They make it difficult to explain and reclassify fairly.

## Consequences

Requires separate operational/academic statuses, explicit timeout rules, and corrections with cause; preserves raw evidence.

## Validation and review

P5/P6 compare technical and academic failures and regrade without executing code. Review this ADR if those conditions are not met. See [architecture](../architecture.md), [contracts](../api-spec.md) and [open issues](../PROJECT_PLATFORM_PROPOSAL_REVIEW.md).