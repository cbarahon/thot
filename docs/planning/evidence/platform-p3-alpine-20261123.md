# P3 Alpine preparation, module, and agent validation

Date: 2026-11-23

## Host filesystem expansion

The upgraded disposable Debian 13 KVM host exposed a 60 GiB disk while its DOS
partition table still placed the active swap partition inside an extended
partition immediately after the 30.3 GiB root partition. The host had no
unallocated contiguous space after root until the extended swap partition was
removed. The operation was performed online:

1. Saved the pre-change partition table and `/etc/fstab` as
   `/root/sda-partitions.pre-resize.sfdisk` and `/root/fstab.pre-resize`.
2. Disabled the old 1.7 GiB swap partition, removed logical partition `sda5`
   and extended partition `sda2`, then changed only `sda1`'s end sector. Its
   start sector remained 2048.
3. Updated the running kernel partition size, expanded ext4 online with
   `resize2fs`, and verified the 60 GiB root partition.
4. Replaced partition swap with a 2 GiB `/swapfile`, mode `0600`, enabled it,
   and added `/swapfile none swap sw 0 0` to `/etc/fstab`.

Final observation: `/dev/sda1` is 60 GiB, ext4 reports 59 GiB, the host had
39 GiB free after the run, and the 2 GiB swapfile was active. `findmnt --verify`
reported zero parse/errors; its warnings were the existing CD-ROM type
mismatch and the standard swapfile source warning. No reboot was required.

## Focused local validation

- `cargo fmt --all -- --check` — passed.
- `cargo check -p thot-core -p thot-runner -p thot-engine -p thot-delivery -p thot-kvm-alpine -p thot-alpine-agent -p thot-cli` — passed.
- `cargo test -p thot-kvm-alpine --lib` — 2 passed, including strict loading
  of the sample preparation script and module manifest/package digest.
- `cargo test -p thot-alpine-agent --bin thot-alpine-agent` — 2 passed,
  including package digest and capability rejection.
- `cargo test -p thot-engine --test sandbox_repository --test shared_module_contract`
  — 3 passed.
- `cargo test -p thot-core --test sandbox_contracts` — 3 passed.
- Host CLI release build and static x86_64 musl agent build — passed.

No workspace-wide test suite was run.

## Real KVM-Alpine smoke

The bundled project ran on the upgraded host. Run ID:
`5e84f6ee-f275-43e5-8f1d-bb61e07ee51d`. The CLI committed and verified the
`thot.run/v1` run folder. Results were:

| Student | Grade | Module result |
|---|---:|---|
| student1 | 10/10 | completed, passed, exit 0, stdout `42\n` |
| student2 | 2/10 | completed, failed, exit 1 (academically incorrect script) |
| student3 | 0/10 | skipped because syntax test failed |

The journal contains `platform.preparation_completed` with requested `bash`,
the installed `apk` package inventory, and the `verify-bash.sh` digest,
timeout, exit code and captured stdout/stderr. The module run is tied to its
manifest ID/version and package SHA-256. Guest staging uses acknowledged
64 KiB chunks and the agent verifies the final SHA-256 before publication and
again before executing the script.

All student clones were undefined and removed after each attempt. The current
run folder is owned by `sysadmin`, uses `0700` directories and `0600` files,
and is writable by the professor account.

## Issue found and corrected

The first module fixture invocation omitted the two integer arguments
required by the sample student program. The smoke failed that module test while
the direct test passed. The module was corrected to invoke the student Bash
script with `17 25`, its manifest digest was updated, and the complete
three-student smoke then produced the results above. This confirms the module
path executes the supplied package, not just its metadata.

## Not qualified

This P3 smoke does not test live Google OAuth/large OVA acquisition, Windows
OVA boot, nested Linux/Windows guests, malicious archive parsing, guest escape
or network attack probes, resumable transfers, task cancellation/progress,
module asset bundles, Debian/Docker agents, provider-trait conformance, or
daemon/report import integration. Those remain P0/P2/P3/P5 gates.
