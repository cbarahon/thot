# ADR-0017: Unified local evaluations with on-demand supervision

- Status: Accepted; initial CLI/configuration/evidence slices implemented, supervision migration in progress
- Date: 2026-11-25
- Scope: local professor CLI, configuration, supervision and reusable modules

## Context

The daemon-backed `thot run` and foreground `thot sandbox run` expose different execution, persistence and lifetime models. Long Alpine jobs can appear stalled; configuration repeats technical details and installation starts unused services. The professor needs one understandable workflow while retaining portable evidence and provider-managed isolation.

## Decision

1. `thot run` durably submits a background job to an on-demand engine. `status`, `log`, `cancel` and `report` share its run identity and repository. Old commands remain compatibility routes until equivalent behavior is available.
2. The engine plans, acquires submissions, stores evidence and grades. Providers own all templates, baselines, clones, deadlines and cleanup; agents only execute and report observations. Do not put clone management into the engine.
3. Introduce an explicit v4 project schema translated with v2/v3 to a common internal plan. Declare workload resources once; derive provider limits. Resolve versioned global defaults with project and advanced overrides, then enforce separate administrator ceilings. Save original and effective configuration with redacted provenance.
4. Start only needed Thot workers and stop them after all dependent runs and cleanup finish. Dormant service definitions and installation-time setup are acceptable; routine invocation requires neither sudo nor exported environment variables. Do not stop independently managed host services.
5. Require live heartbeats, stage progress, bounded transport/process operations, independent provider cleanup supervision and durable interruption state. Liveness never resets operation deadlines. Infrastructure failures remain pending assessment.
6. Add a versioned native executable module ABI, with Rust implementations and target-specific bundles alongside existing POSIX modules. External virtualization/inspection tools remain preparation dependencies. Preserve guest-local libvirt authority and isolate each student's resources.
7. The normal classroom profile protects against accidental damage and resource exhaustion. Retain disposal, bounds, credential separation, safe extraction and no host mounts; defer professional adversarial attestation/signing/network-broker infrastructure. Do not claim adversarial security from this profile.

Detailed contracts, lifecycle diagrams, command mapping and defaults are in [local evaluation architecture](../architecture/local-evaluation.md). Ordered implementation and acceptance criteria are in the [migration backlog](../planning/local-evaluation-migration.md).

## Relationship to earlier decisions

Retains ADR-0003/0004 portable evidence and re-evaluation, ADR-0006 observation/assessment separation, ADR-0007 independent platforms, ADR-0009 delivery intake, ADR-0013 evaluation items, ADR-0014 archives and ADR-0016 guest-local libvirt authority. Extends ADR-0008/0011 with a future configuration adapter and supervised run contract; does not change existing serialized files. Extends ADR-0012 with native modules and concurrent progress/control, while preserving its shell ABI. Narrows adversarial release expectations in older overview/P0/P2 target text for the ordinary classroom profile; capacity, bounded execution, isolation and cleanup still require demonstrated behavior. Historical threat findings remain valid limitations.

## Alternatives and consequences

Keeping two runners duplicates status, defaults and failure behavior. A permanent daemon is unnecessary for sporadic local use. A detached foreground process without supervision cannot meet recovery and logout requirements. Rewriting all modules before unifying runs delays the usable slice; migrate libvirt first and retain compatibility adapters.

The selected design adds explicit service lifecycle and configuration migration work but gives one user workflow. Native binaries reduce language-runtime requirements without eliminating platform dependencies. No documentation decision establishes Windows/nested-KVM viability: hardware and full assessment qualification remain separate.
