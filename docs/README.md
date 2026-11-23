# Thoth documentation

## Current status

The [implemented catalog](IMPLEMENTED_CATALOG.md), [POC guide](THOT_VM_ACTIVITY_POC.md), and [installation](../deploy/README.md) describe the POC VirtualBox, its testing, qualification, and reporting. The workspace also contains a transition to [VirtualBox HTTP v1 agent](../apps/thot-virtualbox-agent/README.md) and [execution folders](../apps/thotd/README.md); some POC texts still reflect the previous deployment. This separation should not be confused with isolation of external code.

## Proposed platform v2

All of the following documents are design, not published functionality:

| Document | Content |
|---|---|
| [Requirements](requirements.md) | Functional/non-functional requirements and acceptance criteria |
| [Architecture](architecture.md) | Generic model, KVM + Docker, security, operation and Linux requirements |
| [Data model](data-model.md) | Portable project, identities, results, files and editing |
| [Contracts and interfaces](api-spec.md) | YAML for workflow, tasks/results, protocol, API and artifacts |
| [Implementation plan](implementation-plan.md) | Phases, dependencies, testing, migration and subsequent evolution |
| [Architectural decisions](decisions/README.md) | ADR with alternatives and consequences |
| [Proposal Review](PROJECT_PLATFORM_PROPOSAL_REVIEW.md) | Difficulties, risks and pending decisions |

`ROADMAP.md` and `TODO.md` have been replaced by the deployment plan, which retains the Unicode reporting work and evolution options. The POC guidelines are maintained because they are still necessary to operate/migrate the existing implementation.

The [KVM Alpine POC](KVM_ALPINE_POC.md) describes the proposed low-resource `kvm-alpine` platform and a synthetic project fixture.

The [OVA multiboot tutorial](OVA_MULTIBOOT_TUTORIAL.md) walks through the
Arch/Alpine/Windows plus shared-data assessment scenario. It identifies which
parts the current VirtualBox OVA workflow can check and which internal tests
are still needed before the full grading workflow is supported.

The [KVM Alpine OVA gap plan](OVA_MULTIBOOT_KVM_ALPINE_GAP_PLAN.md) maps this
same use case onto the new `kvm-alpine` architecture. It records the current
gaps, proposed decisions, phase gates, and acceptance tests for Drive intake,
safe OVA inspection, nested VM execution, GRUB verification, shared-data
checks, durable grading, and professor qualification.
