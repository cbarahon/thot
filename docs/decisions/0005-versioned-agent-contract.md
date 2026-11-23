#ADR-0005: Versioned and bounded agent contract

Status: **proposed**, 2026-11-15. Scope: target platform v2; It does not imply implementation or definitive approval.

## Context

Separating tests from the engine requires checking for identity, forwarding, connection loss, and untrusted output. The current HTTP v1 VirtualBox uses shared routes and is not sufficient for the new border.

## Proposed decision

v2 protocol with typed tasks, identities, epoch/lease, ACK events, hashed blobs and normalized results; Unix/vsock local. Agents without access to the notes store.

## Alternatives

Alternatives: remote shell commands or reuse v1 without changes. They do not provide the required scope, transfer and recovery guarantees.

## Consequences

Compliance and transition adapter must be developed; message delivery can be repeated. Execution of external effects and L1 attestation are not exactly promised.

## Validation and review

P3 checks loss/duplication/cancellation and limits. Remote network requires another ADR with mTLS and authorization. Review this ADR if those conditions are not met. See [architecture](../architecture/overview.md), [contracts](../architecture/contracts.md) and [open issues](../planning/platform-review.md).