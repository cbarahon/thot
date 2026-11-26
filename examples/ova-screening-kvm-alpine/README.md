# OVA screening with libvirt on KVM-Alpine

From the project folder, run `thot run`, follow the evaluation with `thot status` and `thot log`, then read the grades with `thot report`. The `alpine-ova` profile supplies the VM settings, preparation and module defaults.

This project fixture replaces the earlier unsupported VirtualBox dependency
with a pinned `libvirt-vm` portable module. It configures the Alpine outer VM
to install `libvirt`, QEMU, `qemu-img`, Python, and OVMF during preparation,
then runs `import_ova`, `start_vm`, `stop_vm`, and `destroy_vm` as separate
Thot tests. Import interprets the OVF and defines an offline libvirt domain.

Put the student OVA in `deliveries/ArchLinux_Dualboot.ova` and run the project
from this directory with the KVM-Alpine CLI. The supplied example config uses
a 100 GiB outer disk for the OVA's 60 GiB virtual disk. The importer reserves
space for the OVA, extracted source disk, and converted qcow2 before
conversion. Ensure the host has enough additional free space for the source,
run input, allocated guest overlay, and runtime overhead. The 59 GiB test host
is too small for a real run.

The module performs OVA import and VM lifecycle actions only. Partition,
operating-system, boot-menu, and shared-data checks still need separate tests.
