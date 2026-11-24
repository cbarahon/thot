# ADR-0015: Portable module platform-failure exit code

- Status: Accepted for `posix-shell-script-v1` on the Alpine agent
- Date: 2026-11-24

## Context

Portable modules are professor-supplied actions executed inside the disposable
sandbox. Their process exit code is normally an action result: a nonzero exit
is evaluated against the configured test expectation. Some failures, however,
are caused by the sandbox platform (for example, unavailable libvirt or
insufficient scratch storage). Treating those as ordinary completed failures
would incorrectly deduct student points.

## Decision

For a pinned portable module only, process exit code `75` means
`platform_error`. The Alpine guest agent sets both the typed status and the
`platform_error` result flag. Thot records the test assessment as pending and
the grading policy keeps the grade pending. A non-module command returning 75
retains its ordinary completed-process semantics. Timeouts take precedence
over the reserved exit code.

Modules should use this code only when the action could not be performed due to
platform/service/configuration limits. Invalid or unsupported student
deliverable content remains an ordinary nonzero test result. The code is a
minimal compatibility convention for the current shell module ABI; a future
structured module result ABI should carry this distinction as a typed field.

## Consequences

- Modules can distinguish infrastructure failure from student failure without
  changing the existing execute request/result frame.
- Test authors must reserve exit 75 and document module-specific failure
  conditions.
- The current implementation is in the Alpine agent; other sandbox agents
  must adopt the same mapping before claiming this module ABI capability.

## Validation

The Alpine agent has focused status-mapping coverage for module/non-module,
ordinary failure, infrastructure failure and timeout precedence.
