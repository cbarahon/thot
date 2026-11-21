# ADR-0007: Multiple independent sandbox platforms

Status: proposed, 2026-11-20. Applies to the target architecture; it does not describe an implemented capability.

## Context

Assessment tests have different risk and resource profiles. Some only inspect files; some install dependencies or execute student scripts; others need a full Linux system. One execution boundary cannot provide both the strongest practical isolation and the lowest cost. Tests must also be reproducible and their platform failures must not be mistaken for student failures.

## Decision

Define three explicit platform kinds in project configuration:

- `kvm`: Debian 13 guest VM, common instructor-controlled preparation, stopped clean baseline snapshot, and a fresh linked clone per student or assigned group. Copy only the submission and declared instructor test assets into each clone. An in-guest agent executes ordered tests, enforces limits, captures output/exit status/diagnostics, and returns progress/results. The engine alone validates evidence, evaluates checks, and creates expedients.
- `docker`: pinned base image and a fresh container with bounded writable state per student/group. Treat host Docker as lower assurance than KVM because the container shares the host kernel. Never expose host sockets, privileged mode, or unrestricted mounts.
- `host`: allowlisted, reviewed, low-risk adapters (for example, read-only PDF metadata or archive inventory). This is not a sandbox. Arbitrary student scripts and project-provided executables are disallowed unless an administrator verifies that the entire Thot host is itself an outer sandbox.

Each test belongs to exactly one platform. Test dependencies and runtime resource references are platform-local; cross-platform test dependencies and live interaction are rejected. The engine may combine finalized results from all platforms for checks, rubrics, and reporting. This aggregation does not let a test on one platform read another platform's files or live state.

Preparation runs once per platform invocation and is trusted instructor content. It has separate limits and captured provenance; it is not student evidence. KVM preparation must complete before the clean snapshot is taken. A failed preparation or snapshot blocks that platform before student tests begin.

## Alternatives considered

- Run all tests in KVM: strongest and conceptually simple, but unnecessarily expensive for safe file inspection.
- Use Docker as the universal sandbox: cheaper and convenient, but an escape can reach the shared host kernel; unsuitable as the only boundary for hostile code.
- Run all tests on the host: simplest and fastest, but exposes professor files and system state to student-controlled code.
- Allow tests to depend across platform boundaries: flexible, but creates hidden transfer channels, ordering and consistency problems, and additional security policy. Defer until an explicit artifact bridge is justified.

## Consequences and difficult issues

- KVM linked clones depend on an unchanged parent image/snapshot. They are efficient overlays, not standalone backups. Portable exports must include the parent chain or be marked non-replayable.
- Snapshot creation needs a defined consistency point: stop and flush the guest; do not snapshot while package operations are still writing. Memory snapshots are excluded initially.
- Debian package installation introduces network, supply-chain, version-drift, and reproducibility risks. Prefer digest-pinned images and controlled mirrors; if `apt` is used, record repository identity and installed versions.
- KVM, nested virtualization, and linked clone support vary by host/kernel/storage. Preflight must verify actual capability, not infer it from a device node or configuration setting.
- Docker host versus Docker inside KVM remains an open deployment choice. These have different isolation and resource semantics and must not be represented as equivalent configurations.
- Host execution requires an auditable action allowlist and safe file parsers. “Read-only” alone does not guarantee safety: parsers can have vulnerabilities, and decompression can exhaust resources.
- The guest agent is inside an environment that may be controlled by the student. Results need bounded transport and attribution, but cannot be treated as tamper-proof after the student gains guest root. High-assurance observation may require a separate observer boundary.
- Network policy, credentials, artifact size, output truncation, cancellation, cleanup failure, and whether one failed test blocks the rest need explicit defaults.
- Group assignments need a declared shared `student_unit_id`; otherwise each student's environment must be separate even when input files are identical.

## Validation required

Before enabling any platform for untrusted submissions, demonstrate per-student cleanup and isolation on real Linux hosts; test KVM snapshot/clone recovery and parent-chain export; test package provenance and offline operation; test Docker limits and forbidden host access; fuzz/limit host file parsers; and verify that infrastructure failures leave grades pending rather than zero. Revisit the ADR after the host-Docker decision and the first hardware/storage prototype.
