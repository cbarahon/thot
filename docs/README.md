# Thot documentation

Start with [implementation status](status.md) to see which workflows run today, what has been tested, and which features are planned.

## Install and run Thot

| I want to… | Read |
|---|---|
| Install Thot on a fresh host | [Deployment and host setup](../deploy/README.md) |
| Run my first sandbox assessment | [KVM Alpine starter project](../examples/kvm-alpine-poc/README.md) |
| Configure local or Google Drive submissions | [Delivery agents and OAuth](guides/delivery-agents.md) |
| Screen a submitted OVA using the existing daemon | [VirtualBox screening tutorial](guides/ova-screening.md) |
| Find a command or export grades | [CLI reference](../apps/thot-cli/README.md) |

## Architecture and decisions

Read the [architecture overview](architecture/overview.md) for the engine, providers, execution agents and isolation boundaries. Use the [requirements](architecture/requirements.md), [data model](architecture/data-model.md) and [interface contracts](architecture/contracts.md) for the target design. The [decision register](decisions/README.md) records the rationale and status of individual choices.

## Implementation and planning

| Question | Reference |
|---|---|
| How does the Alpine implementation work and what passed on hardware? | [KVM Alpine implementation record](implementation/kvm-alpine.md) |
| Which tests, checks, rubrics and reports does the daemon implement? | [VM activity reference](implementation/vm-activity-reference.md) |
| Where does the daemon store runs, and how does its VirtualBox agent work? | [Daemon](../apps/thotd/README.md) · [VirtualBox agent](../apps/thot-virtualbox-agent/README.md) |
| What has been decided for the nested OVA multiboot assessment? | [Multiboot decisions, tasks and qualification gates](planning/ova-multiboot.md) |
| What work remains across the platform? | [Platform roadmap](planning/platform-roadmap.md) · [Engineering review and open risks](planning/platform-review.md) |
