#ADR-0004: Editing and expansion with slight provenance

Status: **proposed**, 2026-11-15. Scope: target platform v2; It does not imply implementation or definitive approval.

## Context

You want to adjust values ​​or incorporate deliveries/tests without repeating everything, preserving reliable explanations of the results.

## Proposed decision

Run modifiable with small revisions; editing of entries/corrections validated. Reuse by input footprint, step, dependencies, images and policy. Rebuild live state when L1 was lost.

## Alternatives

Alternatives: overwrite without history or complete immutability with new run for each change. The first loses attribution and the second makes everyday use difficult.

## Consequences

Introduces selection of results by review and reporting of corrections. It does not guarantee saving execution if the new test requires older machines.

## Validation and review

P5 must demonstrate rubric, code changes, late submission, individual correction, and live dependency. Review this ADR if those conditions are not met. See [architecture](../architecture/overview.md), [contracts](../architecture/contracts.md) and [open issues](../planning/platform-review.md).