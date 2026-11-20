# ADR-0002: Common laboratory per evaluation unit

Status: **proposed**, 2026-11-15. Scope: target platform v2; It does not imply implementation or definitive approval.

## Context

Tests and agents need to interact with services and VMs created by previous steps without mixing student work.

## Proposed decision

Share workspace, network and resource registration between agents of an explicit unit; separate units using different L1s. Engine retains the DAG and notes.

## Alternatives

Alternatives: per-step sandbox loses state; one VM for the entire class allows contamination. Joint comparisons are declared as their own unit.

## Consequences

Allows Git/make/KVM/SSH scenarios. Root in L1 can falsify observations from the same laboratory; the nested profile declares that limitation.

## Validation and review

P4 demonstrates interaction between agents and lack of access between units. Review this ADR if those conditions are not met. See [architecture](../architecture.md), [contracts](../api-spec.md) and [open issues](../PROJECT_PLATFORM_PROPOSAL_REVIEW.md).