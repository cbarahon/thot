# KVM Alpine POC fixture

This is a runnable synthetic project for `thot run` on Linux/KVM. The command uses the shared `thot-core` v2 configuration and `thot-engine` check/rubric evaluator. It is not yet managed by `thotd` or the canonical lifecycle provider interface.

The CLI uses a pinned Alpine 3.24.1 x86_64 cloud-init qcow2 image and its configured SHA-512 digest. It creates a prepared linked qcow2 overlay, then one student overlay per roster entry. Student domains have no virtual network device. A per-clone cloud-init seed injects the static guest agent and one-use VSOCK token; submissions, instructor tests, and pinned test modules stream in acknowledged chunks with SHA-256 verification. Assignments point to deliverable directories, a suitable layout for checked-out repositories. Results and test evidence are saved in a portable, verified run folder under `results/<run-id>/`.

`lifecycle.preparation.scripts` names project-relative shell files, optionally pins their expected SHA-256, and supports per-script timeouts. Thot hashes each file before baseline reuse and records output, exit status and installed package inventory. The sample includes a Bash preparation check and a `posix-shell-script-v1` sum-check module under `extern/modules/`; the module manifest pins its package digest and required guest capabilities. This first module ABI supports one script file per module, with argv passed as script arguments. See [ADR-0012](../../docs/decisions/0012-alpine-preparation-and-posix-test-modules.md) for scope and limitations.

The expected outcomes are:

- `student1`: valid Bash adds 17 and 25; 10/10.
- `student2`: valid Bash multiplies instead; syntax criterion passes; 2/10.
- `student3`: invalid Bash syntax; functional test is skipped; 0/10.

Assignments refer to exact names from the delivery-agent inventory. With no `delivery_agent` configured, Thot uses `delivery_folder` (`students/`) and fetches each matching file or directory before VM staging. To use Google Drive, configure a `gdrive` agent in `configuration.thot.yaml`; see [delivery-agent setup](../../docs/guides/delivery-agents.md). Thot does not clone Git repositories itself: place repository checkouts in the local folder or upload them to Drive.

The smoke run from 2026-11-22 is archived under [results/2026-11-22-smoke](results/2026-11-22-smoke/). It ran on `sysadmin@192.168.1.35`, a Debian 13 KVM host. It does not validate the daemon/API provider integration, named libvirt snapshots, timeout recovery, resource enforcement, or export/import workflow.

Build the host CLI and static Alpine guest agent from the workspace, install both on a Linux KVM host with libvirt, `virt-install`, `qemu-img`, `cloud-localds`, and `guestfish`, then run the command as the professor:

```sh
RUSTFLAGS='-C target-feature=+crt-static' cargo build -p thot-alpine-agent --target x86_64-unknown-linux-musl --release
cargo build -p thot-cli --release
sudo install -m 0755 target/release/thot /usr/local/bin/thot
sudo install -m 0755 target/x86_64-unknown-linux-musl/release/thot-alpine-agent /usr/local/bin/thot-alpine-agent
cd examples/kvm-alpine-poc
thot run
thot status
thot log
```

See [configuration.thot.yaml](configuration.thot.yaml), [synthetic roster](students.yaml), [assignments](assignments.yaml), [test expectations](extern/tests/README.md), and the [architecture and closure gaps](../../docs/implementation/kvm-alpine.md).
