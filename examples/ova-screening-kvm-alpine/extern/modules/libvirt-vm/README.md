# `libvirt-vm` instructor module

This portable Thot module implements four sequential actions for an OVA:

1. `import_ova <name> <path>` validates/extracts the OVA, verifies its optional
   `.mf` checksums, interprets its OVF, converts supported VDI/VMDK disks to
   qcow2, and defines a libvirt domain.
2. `start_vm <name>` starts that owned domain.
3. `stop_vm <name>` requests ACPI shutdown and uses a bounded forced stop if
   the guest does not shut down in time.
4. `destroy_vm <name>` verifies the ownership manifest and disk paths, then
   undefines the domain and removes only that module's private workspace.

The module runs as Thot's unprivileged UID/GID 1001. Preparation creates the
named `thot` account, installs D-Bus and PolicyKit, and grants the guest-local
`libvirt` group the `org.libvirt.unix.manage` action through a PolicyKit rule.
For a pinned module invocation declaring `libvirt.system@1`, the Alpine agent
starts the fixed guest-local service set and grants that process the
supplementary `libvirt` group after checking the Unix socket's group and mode.
Other module launches receive no supplementary groups. The Alpine VM never
mounts or forwards the professor host's libvirt socket. Student code is never
run by this module.

The import maps OVF CPU count, memory, disk capacity, and EFI/BIOS choice. EFI
uses fresh OVMF variables; the VirtualBox NVRAM file is retained only as
provenance and is not imported. All network interfaces are intentionally
omitted to keep the student VM offline. The domain uses QEMU TCG emulation
because this test host does not expose nested `/dev/kvm` to Alpine.

The module reserves exit code 75 for platform/infrastructure failures such as
insufficient sandbox storage or unavailable libvirt. The Alpine agent maps
that code from a pinned module invocation to `platform_error`, so Thot records
the attempt as pending rather than deducting student points. Other nonzero
module exits remain completed test actions and are evaluated by their test's
expected exit-code contract.

This first implementation is intentionally conservative: one OVF descriptor,
up to 64 regular tar members, VDI or VMDK disks, up to 128 GiB combined virtual
disk capacity, and bounded OVA/extracted sizes. It rejects unsafe archive paths,
links, duplicate members, DTD/entity declarations, unsupported disk formats,
and insufficient free guest storage before extraction/conversion. The current
sample's 60 GiB virtual disk needs more storage than the 59 GiB test host can
provide. The example configures a 100 GiB outer guest, but host storage must
also cover the source OVA, run evidence, staged OVA, extracted disk, converted
disk, and guest overlay. The actual OVA trial filled the host filesystem
during conversion, so use a host with substantially more free space.

The four actions validate import/lifecycle mechanics only. They do not yet
inspect EFI partitions, OS filesystems, GRUB entries, or shared-data mounts.
