# Implementation status

| Workflow | Available today | Evidence and limits |
|---|---|---|
| Local KVM Alpine CLI | Run ordered instructor tests in a disposable Alpine guest and save per-student JSON under the project. | The three-student Bash fixture scored 10/10, 2/10 and 0/10 on a KVM host. See the [implementation record](implementation/kvm-alpine.md#implementation-test-record) and [saved results](../examples/kvm-alpine-poc/results/). |
| Submission intake | Local folders are the sandbox CLI default. The Google Drive agent supports read-only OAuth and ordinary uploaded files. | The KVM smoke test used local intake. The 128 MiB acquisition limit and small guest staging path prevent general OVA use. See the [delivery guide](guides/delivery-agents.md). |
| Daemon and VirtualBox activities | Use local OVA submissions, project rosters, expedients and grade reports. The HTTP agent controls VirtualBox on its configured host. | The [implemented catalogue](implementation/vm-activity-reference.md) documents supported tests and scoring. The [screening guide](guides/ova-screening.md) covers partial OVA evidence. |
| Drive-to-KVM multiboot | Planning is closed for the first laptop implementation. | Real large-file transfer, nested EFI boots, cross-OS shared-volume evidence and verified teardown are qualification gates. See the [implementation plan](planning/ova-multiboot.md). |

The architecture specifies the provider-managed workflow. The current Alpine CLI implements the narrower smoke path. Consult the implementation record before relying on a proposed provider, protocol or report feature.
