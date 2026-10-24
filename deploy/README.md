# Native VirtualBox POC deployment

The VM activity runs `thotd` natively on the VirtualBox host. The existing
container deployment template is not the execution boundary for this POC:
`VBoxManage` inside a container would use a different VM registry unless the
host profile and devices were mounted into it, which this deployment does not
do. Do not run the POC through Docker or Podman.

## Build production artifacts

Build on a Linux machine compatible with the target host. Rust and Cargo are
needed only on this build machine. The target host receives the two release
binaries and configuration, not the source tree or Cargo development files.

```sh
cargo build --release --locked -p thotd -p thot-cli
install -D -m 0755 target/release/thotd ./dist/thotd
install -D -m 0755 target/release/thot ./dist/thot
install -D -m 0644 examples/vm-activity-poc/project.yaml \
  ./dist/projects/example-vm-activity.yaml
```

Copy `dist/` to the target; it contains the two production binaries and the
sample project configuration, without the source tree or development files.
Keep the binaries from the same build together. Runtime prerequisites on the
target are a supported Linux host, VirtualBox with `VBoxManage`, SQLite-compatible filesystem storage, and
GNU `df` from coreutils (the daemon uses `df -B1 --output=avail`), plus the
shared libraries reported by `ldd dist/thotd` and `ldd dist/thot` (install
those from the host distribution). Use the same Linux architecture and ABI for
the build and target hosts. Neither Rust nor Cargo is needed on the target.
The web UI, lighttpd, Docker, and a container runtime are not required.

## Prepare the host

Use a dedicated, unprivileged OS account for Thot. It must own its VirtualBox
profile and storage paths and have permission to use the installed VirtualBox
kernel driver. Do not run `thotd` as root and do not reuse a teacher's normal
VirtualBox account or profile.

Example paths (adjust to local policy):

```text
service account: thotd
projects:       /etc/thot/projects
deliveries:     /var/lib/thot/deliveries
artifacts:      /var/lib/thot/artifacts
VM storage:     /var/lib/thot/vms
VBox profile:   /var/lib/thot/vbox-profile
SSH secret:     /etc/thot/secrets/guest-ssh-password
```

On a fresh Debian/Ubuntu style host, create the service account and writable
data tree. Install VirtualBox from the host's approved package source first;
the service account must be allowed to use that installation's kernel driver.
Some distributions require membership in `vboxusers`; follow the installed
VirtualBox package's access policy and restart the service after changing
group membership.

```sh
sudo useradd --system --create-home --home-dir /var/lib/thot --shell /usr/sbin/nologin thotd
sudo install -d -o thotd -g thotd -m 0750 \
  /var/lib/thot/deliveries /var/lib/thot/artifacts /var/lib/thot/vms \
  /var/lib/thot/vbox-profile
sudo install -d -o root -g thotd -m 0750 /etc/thot/projects /etc/thot/secrets
```

The configured paths must remain separate and the VM storage root must be
empty when starting a run. Make the project directory read-only to the service
after project installation. Restrict the SSH secret to the service account
(`root:thotd`, mode `0640`, or stricter). It contains only the guest password.
Never place the password in project YAML, process arguments, database rows,
expedients, or logs.

Run all VirtualBox commands as `thotd` with `VBOX_USER_HOME` set to the
dedicated profile. Import, inspection, power operations, screenshot, and
cleanup must use the same account, executable, profile, and storage root. Use
the pinned VirtualBox version recorded during qualification; repeat adapter
compatibility checks after changing it.

The host firewall must prevent the guest from reaching unrelated host services
and unnecessary outbound destinations. NAT forwarding is loopback-only:
guest TCP 80 maps to `127.0.0.1:8080`, and guest TCP 22 maps to
`127.0.0.1:2222`. Do not use bridged networking. Disable shared folders,
shared clipboard, drag-and-drop, and USB passthrough. Do not attach host
directories or devices to imported guests. The `/thot` VM group and VM name
prefix are labels; the dedicated profile, storage root, and persisted cleanup
manifest establish run ownership.

Before starting `thotd`, verify that host ports 8080 and 2222 are free. The
service API defaults to `127.0.0.1:8081`; keep it loopback-only. Configure
`thot` on the same host, or use an SSH tunnel for operator access. The run
preflight rejects any OVA exceeding the size limit. Before import, the daemon
estimates the VM storage needed by summing the VMDK member sizes in the OVA tar
and requires that estimate plus the configured reserve. This is a conservative
archive-based estimate; the reserve must account for VirtualBox conversion and
filesystem overhead. The retained source OVA is separately checked against
available space in artifact storage. Keep report, HTTP body, and command output
limits finite. Source OVAs, project snapshots, expedients, logs, and evidence
remain until an operator explicitly archives or removes them.

## Configure and install

Install the project YAML as `/etc/thot/projects/<project-id>.yaml`. The project
contains the fixed activity policy and a secret reference only; host paths,
credentials, and executable commands do not belong in the project. Place OVA
files directly in a subdirectory of `/var/lib/thot/deliveries`.

Install the repository's sample as a starting point and create a delivery
folder:

```sh
sudo install -o root -g thotd -m 0640 dist/projects/example-vm-activity.yaml \
  /etc/thot/projects/example-vm-activity.yaml
sudo install -d -o thotd -g thotd -m 0750 \
  /var/lib/thot/deliveries/batch-01
```

Provision the guest password into the configured secret file using the host's
secret-management process. For example, create the file out of band and then
set its owner to `root:thotd` and mode to `0640`; do not put its value in a
shell command or this environment file.

Create `/etc/thot/thotd.env` (mode `0640`, root-owned and readable by the
service) and set the host-specific values:

```ini
THOTD_BIND_HOST=127.0.0.1
THOTD_BIND_PORT=8081
THOTD_DATABASE_URL=sqlite:/var/lib/thot/thot.sqlite3
THOTD_PROJECT_ROOT=/etc/thot/projects
THOTD_DELIVERY_ROOT=/var/lib/thot/deliveries
THOTD_ARTIFACT_ROOT=/var/lib/thot/artifacts
THOTD_STORAGE_ROOT=/var/lib/thot/vms
THOTD_VBOX_USER_HOME=/var/lib/thot/vbox-profile
THOTD_VBOX_MANAGE_PATH=/usr/bin/VBoxManage
THOTD_EXPECTED_VBOX_VERSION=<qualified-version-without-r-build-suffix>
THOTD_OVA_SIZE_LIMIT=10737418240
THOTD_FREE_SPACE_RESERVE=10737418240
THOTD_OUTPUT_LIMIT=1048576
THOTD_IMPORT_TIMEOUT=1800
THOTD_STARTUP_TIMEOUT=180
THOTD_SSH_TIMEOUT=15
THOTD_SHUTDOWN_GRACE=30
THOTD_SSH_SECRET_FILE=/etc/thot/secrets/guest-ssh-password
THOTD_FIREWALL_CONFIRMATION=true
```

Set `THOTD_EXPECTED_VBOX_VERSION` to the qualified `VBoxManage --version`
number (for example, `7.0.20`, without the trailing `r...`). Runs fail
preflight when the installed version differs. Set
`THOTD_FIREWALL_CONFIRMATION=true` only after the administrator has checked
firewall and guest egress rules manually; the service cannot inspect firewall
policy. The SSH user is fixed as `tvadmin` by the activity project schema.

`THOTD_DATA_ROOT` is an optional shorthand for the default delivery, artifact,
VM, and VirtualBox profile roots. The API binding, project root, database,
VirtualBox executable, and secret path should be reviewed for the target host.

Example systemd unit (`/etc/systemd/system/thotd.service`):

```ini
[Unit]
Description=Thot VM activity service
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=thotd
Group=thotd
EnvironmentFile=/etc/thot/thotd.env
Environment=RUST_LOG=info
ExecStart=/usr/local/sbin/thotd
Restart=on-failure
RestartSec=3
UMask=0077
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/thot

[Install]
WantedBy=multi-user.target
```

Install and start it:

```sh
sudo install -o root -g root -m 0755 dist/thotd /usr/local/sbin/thotd
sudo install -o root -g root -m 0755 dist/thot /usr/local/bin/thot
sudo systemctl daemon-reload
sudo systemctl enable --now thotd
sudo systemctl status thotd
```

Review the systemd sandbox against the installed VirtualBox driver and profile
before use. If the host's VirtualBox setup needs additional access, grant only
that access to the `thotd` service account; do not disable the service
hardening wholesale.

## Start and inspect a run

The CLI default is `127.0.0.1:8081`. Use a project ID matching the YAML file
name without the `.yaml` extension and a delivery directory below the
configured delivery root:

```sh
thot run create --project example-vm-activity --deliveries /var/lib/thot/deliveries/batch-01
thot run status <run-id>
thot run expedient <expedient-id>
thot run cleanup <expedient-id>
```

The host administrator must check that loopback forwarding works, the two VM
ports are free, the API port is available, the VBox profile is dedicated, and
the service can capture screenshots. Confirm the firewall and guest egress
policy before importing any untrusted OVA. Do not run a delivery when those
checks cannot be made.

Cleanup after an interrupted host or process must use the persisted
run-specific manifest and verify the VM UUID and storage paths before deletion.
Never delete by VM name, group, or prefix alone. Verify the VM is unregistered
and only run-owned storage has been removed. A remaining resource means
cleanup is pending and must be reported as such; it is not a clean run.
