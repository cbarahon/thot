#ADR-0001: KVM VM as outer border

Status: **proposed**, 2026-11-15. Scope: target platform v2; It does not imply implementation or definitive approval.

## Context

Running deliveries can launch processes, Docker, and new hypervisors. A host container shares its kernel and does not satisfy the chosen containment objective.

## Proposed decision

Create a disposable L1 KVM per drive and use Docker/KVM nested exclusively inside. Prohibit access to the teacher's filesystem, devices and sockets.

## Alternatives

Alternatives: native processes, Docker on host and dedicated host per activity. The first two reduce isolation; the third increases operating cost.

## Consequences

Increased consumption, bootstrapping, and nesting dependency; Real Linux certification is required. It does not eliminate hypervisor escapes.

## Validation and review

P0/P2 must validate nesting, permissions, network, quotas and forced destruction. Review this ADR if those conditions are not met. See [architecture](../architecture/overview.md), [contracts](../architecture/contracts.md) and [open issues](../planning/platform-review.md).