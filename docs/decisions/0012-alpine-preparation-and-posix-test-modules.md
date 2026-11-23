# ADR-0012: Alpine preparation and portable POSIX test modules

- Status: Accepted for the first Alpine P3 implementation
- Date: 2026-11-23

## Context

P1 defined module identity, capabilities and evidence, but no executable module
ABI or preparation-script contract. The Alpine smoke runner still embedded
preparation shell snippets in the project YAML, limited transfer to small
files, and let the guest agent execute only direct argv commands. P3 needs a
small first implementation that can be reused by Linux providers without
making the engine execute instructor/student code on the host.

## Decisions

1. **Preparation scripts are project files, not inline YAML.** A preparation
   entry names a safe project-relative UTF-8 shell script, may pin an expected
   SHA-256, and may set a timeout no longer than the platform's preparation
   timeout. Thot calculates and records the actual digest before provisioning.
   Alpine packages install first with `apk`; scripts then run in order as root
   in project-baseline preparation only, under the configured network policy
   and timeout. Their path, digest, timeout, exit status, bounded stdout/stderr,
   requested packages and installed package inventory are recorded. Any
   failure prevents baseline publication and student evaluation.
2. **The first executable test-module ABI is
   `posix-shell-script-v1`.** A module package is one UTF-8 POSIX shell script
   beginning with `#!/bin/sh`; its bytes must match the SHA-256 in its pinned
   module manifest. Tar/zip bundles, dynamic libraries, WASM and arbitrary
   runtime loading are not part of this ABI. A future ABI can add assets and
   richer runtimes without changing this one.
3. **The project references the manifest and script separately.** A module
   test declares runtime, manifest path, package path, and argv arguments.
   Thot parses and validates the versioned manifest, rejects undeclared
   package assets in this runtime, verifies the script digest, records both
   manifest and package as run artifacts, and stages the package under a
   content-addressed guest path.
4. **Module scripts run as the unprivileged student account.** The guest
   agent verifies the staged file's regular-file status, path, 32 MiB bound
   and SHA-256 again immediately before launch, checks required capability
   ID/version pairs, and runs `/bin/sh <staged-module> <argv...>` as UID/GID
   1001 with a cleared environment, bounded streams and task timeout. The
   module has the same guest-visible student files and network restriction as
   a regular test. It does not receive the engine API, host files, grades,
   OAuth tokens or host hypervisor handles.
5. **Guest capabilities are advertised in the VSOCK v2 handshake.** The
   additive `capabilities` response field is default-empty for protocol-v2
   compatibility. The Alpine agent advertises `process.argv`, `files.stage`,
   `files.sha256`, and `module.posix-shell`; it adds `disk.inspect` when
   `guestfish` is installed and `nested.kvm` when `/dev/kvm` is present. The
   runner rejects an unsupported module capability before transferring that
   module. The agent independently checks capabilities at launch. A capability
   is a declared tool/operation presence, not guest attestation or proof that
   a student OS is honest.
6. **File staging is streaming and acknowledged.** The host reads files in
   64 KiB chunks, the existing framed VSOCK protocol receives an ACK for every
   chunk, and the guest streams to a temporary file, computes SHA-256 without
   loading the whole file into memory, and publishes it read-only only after
   final digest verification. The current per-file receiver ceiling is 64 GiB;
   project aggregate limits remain enforced on the host. Transfer is not
   resumable or idempotently retryable: disconnect/interruption fails the
   student attempt and cleanup must discard the clone.
7. **Tests remain sequential.** The current agent executes one request and
   returns one result before reading another. It enforces task timeout by
   killing the process group and captures bounded stdout/stderr, exit code,
   signal and diagnostics. Infrastructure/unsupported outcomes remain
   pending rather than being converted to zero. There is no live progress
   stream or cancel request while a task is running in this slice.

## Consequences and scope limits

- The Alpine provider now has a real path for script preparation, package
  inventory/evidence, digest-pinned shell modules and large input streaming.
- POSIX shell offers a portable command-level module across Linux guests, but
  capability implementations are still platform-specific. This is not yet a
  full cross-provider module catalogue or the production `SandboxProvider`
  lifecycle.
- The Debian, Docker and host agents do not implement this execution ABI yet.
- Modules cannot declare auxiliary assets in this first runtime. Tests needing
  helper files may use the existing instructor-tests input tree; future module
  bundles need a safe archive format, path/size/file-count rules and explicit
  digest mapping.
- The guest agent has no separate upload/blob ACK protocol for results and no
  out-of-band task cancellation. Captured result frames remain bounded by the
  current protocol limits.
- A module can inspect or corrupt its disposable guest, and may attack the
  outer KVM boundary. The smoke VM is not hostile-OVA qualification; P0/P2
  isolation gates remain mandatory.

## Validation

The bundled activity has a preparation script which verifies installed Bash
and a digest-pinned sum-check module shared through the new runtime. Focused
tests exercise package digest/capability rejection, project configuration
validation, streaming agent staging and the KVM three-student smoke run.
Debian/Docker conformance and Windows OVA execution remain untested.
