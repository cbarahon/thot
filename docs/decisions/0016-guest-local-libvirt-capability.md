# ADR-0016: Guest-local libvirt authority for pinned modules

- Status: Accepted for the KVM-Alpine OVA lifecycle module
- Date: 2026-11-24

## Context

The `libvirt-vm` portable module must define and operate QEMU domains inside
the disposable Alpine student clone. The default module identity is an
unprivileged UID/GID 1001. Alpine's system libvirt interface also depends on
D-Bus and PolicyKit, and its service daemons may not be ready when the guest
agent receives a task. The professor host's libvirt socket must never be
available inside the guest.

## Decision

- The preparation phase installs D-Bus, PolicyKit, libvirt, QEMU, and OVMF;
  creates the named `thot` UID/GID 1001 account; and grants the guest-local
  `libvirt` group only the `org.libvirt.unix.manage` PolicyKit action.
- The Alpine agent recognizes `libvirt.system@1` only when the guest-local
  tools and group exist. For a pinned module declaring that capability, it
  starts a fixed list of guest-local services and adds the `libvirt`
  supplementary group to that module process. It verifies the local Unix
  socket's group ownership and read/write mode before launch.
- Other tasks have supplementary groups cleared. The host socket is neither
  mounted nor forwarded. Imported student VMs have no network interface.
- The outer Alpine VM remains the isolation boundary; this capability grants
  libvirt control inside that disposable VM, not on the professor host.

## Consequences

- Modules can use `qemu:///system` without embedding root privileges or
  starting arbitrary project-specified services.
- The agent and preparation configuration must evolve together. Other guest
  agents must not advertise this capability until they implement equivalent
  local-only authority and permission checks.
- Professor-provided modules requesting `libvirt.system@1` are trusted
  assessment code and can consume the resources assigned to the outer guest.
  Resource quotas and robust recovery remain separate platform requirements.
- The UID/GID 1001 choice is currently coupled to the Alpine agent and must be
  reconsidered if the agent's execution identity becomes configurable.

## Validation

On the Debian 13 KVM test host, a synthetic OVA lifecycle smoke run passed
import, start, stop, and destroy with a clean cleanup result. The real 8.49 GB
student OVA has not yet been rerun with this final capability setup because
the test host lacks sufficient free storage.
