# KVM Alpine POC fixture

This folder is a synthetic project for a future `kvm-alpine` provider. It is documentation/configuration and test data only: the current Thot implementation does not recognize `kind: kvm-alpine`, auto-install Alpine, create snapshots or linked clones, or execute this project.

The proposed workflow is:

1. Validate host KVM, libvirt/storage, quotas, and required network policy.
2. Reuse or build the pinned reusable VM named `thot-alpine-platform`, verifying ISO checksum/signature and template provenance.
3. Reuse or create the uniquely named project clone `thot-alpine-poc-sum-project`; install Bash in the one-time preparation phase, then shut down and snapshot it.
4. For each student, create a fresh linked clone from that prepared snapshot, copy that student's script plus instructor test scripts, run syntax then functional tests, collect bounded output, and remove the clone.
5. Let the Thot engine evaluate the collected observations and build expedients. The guest agent only executes tests and reports evidence.

See [configuration.thot.yaml](configuration.thot.yaml), [synthetic roster](students.yaml), [assignments](assignments.yaml), [test expectations](extern/tests/README.md), and the [architecture/host feasibility analysis](../../docs/KVM_ALPINE_POC.md).

Expected cases: Student One's script is correct; Student Two's valid Bash multiplies instead of adding; Student Three has a syntax error. The expected functional input is `17 25`, producing exactly `42` plus a newline.

The example pins Alpine 3.24.4 x86_64 `virt` ISO for reproducibility. Verify the official checksum/signature and review the current Alpine security/support status before a future implementation uses it. The repository development host lacks `/dev/kvm`. A separate Debian 13 KVM test host is available, but no Alpine guest lifecycle has completed there; the current code does not build. See the [implementation audit and closure plan](../../docs/KVM_ALPINE_POC.md#implementation-audit-missing-and-incorrect-behavior).
