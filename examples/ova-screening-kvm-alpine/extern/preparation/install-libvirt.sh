#!/bin/sh
set -eu

# This runs in the disposable Alpine baseline VM, not on the professor host.
# libvirt is deliberately confined to this outer guest; the test module never
# receives the host's libvirt socket.
grep -q '^libvirt:' /etc/group || addgroup -S libvirt

# The guest agent runs test code as UID/GID 1001. Give that identity a stable
# NSS name so PolicyKit can evaluate the narrowly scoped libvirt group rule.
if ! grep -q '^thot:' /etc/passwd; then
  if awk -F: '$3 == 1001 { found = 1 } END { exit !found }' /etc/passwd; then
    echo 'UID 1001 is already assigned to a different Alpine account' >&2
    exit 1
  fi
  grep -q '^thot:' /etc/group || addgroup -g 1001 thot
  adduser -D -H -u 1001 -G thot -s /sbin/nologin thot
fi
addgroup thot libvirt

# The import module runs as thot and must remove the staged archive after
# conversion. The agent remains able to stage files as root.
mkdir -p /work
chown thot:thot /work
chmod 0755 /work

# Load the nested hypervisor on every boot of the prepared student clone.
if grep -qw vmx /proc/cpuinfo; then
  kvm_module=kvm_intel
elif grep -qw svm /proc/cpuinfo; then
  kvm_module=kvm_amd
else
  echo 'Nested virtualization CPU support is unavailable in Alpine' >&2
  exit 1
fi
modprobe "$kvm_module"
test -c /dev/kvm || { echo 'Nested KVM device is unavailable' >&2; exit 1; }
grep -q "^$kvm_module$" /etc/modules || echo "$kvm_module" >> /etc/modules
grep -q '^kvm:' /etc/group || addgroup -S kvm
addgroup qemu kvm

for config in /etc/libvirt/libvirtd.conf /etc/libvirt/virtproxyd.conf; do
  touch "$config"
  sed -i -E \
    '/^[[:space:]]*unix_sock_group[[:space:]]*=/d; /^[[:space:]]*unix_sock_rw_perms[[:space:]]*=/d' \
    "$config"
  cat >>"$config" <<'EOF'
unix_sock_group = "libvirt"
unix_sock_rw_perms = "0770"
EOF
done

# A system libvirt connection normally asks PolicyKit for an interactive
# authorization. Test modules are non-interactive and run as the unprivileged
# thot account with only the libvirt supplementary group. Grant that group the
# domain-management action inside this disposable Alpine guest only.
mkdir -p /etc/polkit-1/rules.d
cat >/etc/polkit-1/rules.d/50-thot-libvirt.rules <<'EOF'
polkit.addRule(function(action, subject) {
    if (action.id == "org.libvirt.unix.manage" && subject.isInGroup("libvirt")) {
        return polkit.Result.YES;
    }
});
EOF

rc-update add dbus default
rc-service dbus start
test -S /run/dbus/system_bus_socket
rc-update add polkit default
rc-service polkit start
rc-update add virtlogd default
rc-update add virtlockd default
rc-update add virtqemud default
rc-update add virtproxyd default
# Alpine's virtproxyd OpenRC script requires a network service, but the
# student clone intentionally has no network. This provider only needs the
# local libvirt Unix socket, so make network an optional dependency.
sed -i 's/^[[:space:]]*need net$/    use net/' /etc/init.d/virtproxyd
grep -Eq '^[[:space:]]*use net$' /etc/init.d/virtproxyd
rc-service virtlogd start
rc-service virtlockd start
rc-service virtqemud start
rc-service virtproxyd start

socket=/run/libvirt/libvirt-sock
test -S "$socket"
test "$(stat -c %G "$socket")" = libvirt
mode=$(stat -c %a "$socket")
group_mode=${mode%?}
group_mode=${group_mode#?}
case "$group_mode" in
  6|7) ;;
  *) echo "libvirt socket group has no read/write access (mode=$mode)" >&2; exit 1 ;;
esac
virsh -c qemu:///system uri
virsh -c qemu:///system capabilities >/dev/null
command -v qemu-img >/dev/null
command -v qemu-system-x86_64 >/dev/null
test -r /usr/share/OVMF/OVMF_CODE.fd
test -r /usr/share/OVMF/OVMF_VARS.fd
qemu-system-x86_64 -accel help 2>&1 | grep -q kvm
printf 'libvirt-ready socket=%s group=%s mode=%s accel=kvm\n' \
  "$socket" "$(stat -c %G "$socket")" "$mode"
