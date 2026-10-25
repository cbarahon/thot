# Native VirtualBox POC deployment

The POC runs `thotd` natively on the Linux host where VirtualBox is installed. It does not support invoking the host VirtualBox registry from the existing container deployment. Run the service under a dedicated, unprivileged OS account with a dedicated VirtualBox profile.

## Build

Build on a Linux machine compatible with the target host. Rust and Cargo are needed only on the build machine.

```sh
cargo build --release --locked -p thotd -p thot-cli
install -D -m 0755 target/release/thotd ./dist/thotd
install -D -m 0755 target/release/thot ./dist/thot
```

Copy the binaries and a teacher-created `.thot.zip` or `.thot.tgz` project archive to the target. The archive must contain `configuration.yaml` at its root; external checks such as `extern/tests/vm_start_time.py` are packaged beside it. The repository example is in `examples/vm-activity-poc/`. Neither the UI, lighttpd, Docker, nor Cargo is needed on the target.

## Prepare the host

Install VirtualBox from the host's approved package source. The service account must be allowed to use its kernel driver. Set up separate service-owned directories for project archives, deliveries, artifacts, VM storage, and the VirtualBox profile. Do not run `thotd` as root or reuse a teacher's normal VirtualBox profile.

Example paths:

```text
service account: thotd
projects:       /etc/thot/projects
deliveries:     /var/lib/thot/deliveries
artifacts:      /var/lib/thot/artifacts
VM storage:     /var/lib/thot/vms
VBox profile:   /var/lib/thot/vbox-profile
```

Create the base directories and install the project archive. The archive itself should be read-only to the daemon. Create the configured delivery and VM storage subdirectories beside it; the daemon needs read access to deliveries and write access to the VM storage subdirectory because the project `vm_path` is relative to the archive's containing directory. For example, with `vm_path: ./vms`, provision a writable `vms/` directory next to the archive. Keep this directory separate from the archive and its scripts.

```sh
sudo useradd --system --create-home --home-dir /var/lib/thot --shell /usr/sbin/nologin thotd
sudo install -d -o thotd -g thotd -m 0750 \
  /var/lib/thot/deliveries /var/lib/thot/artifacts /var/lib/thot/vms \
  /var/lib/thot/vbox-profile
sudo install -d -o thotd -g thotd -m 0750 /etc/thot/projects
sudo install -o root -g thotd -m 0640 linux_competition.thot.tgz \
  /etc/thot/projects/linux_competition.thot.tgz
sudo install -d -o thotd -g thotd -m 0750 \
  /etc/thot/projects/delivery /etc/thot/projects/vms
```

If administrators keep archive directories root-owned, pre-create each configured writable `delivery_folder` and `vm_path` directory with service-account ownership. Project extraction happens into a run-scoped snapshot under the artifact root. Treat external scripts as trusted instructor code: they execute as child processes with the daemon account's access to the host and VirtualBox profile. The runner applies per-test timeouts and output limits, but it is not a script sandbox.

The service requires free loopback TCP ports 8080 and 2222 before VM runs. Guest networking must forward guest TCP 80 to `127.0.0.1:8080` and guest TCP 22 to `127.0.0.1:2222` through NAT. Configure host firewall and guest egress rules to block access to unrelated host services and unnecessary destinations. Do not use bridged networking or attach host folders, clipboard, drag-and-drop, or USB devices to imported guests. `THOTD_FIREWALL_CONFIRMATION=true` records the administrator's manual confirmation; the daemon does not inspect firewall policy.

## Configure `thotd`

Create `/etc/thot/thotd.env` owned by root and readable by the service account (mode `0640`):

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
THOTD_FIREWALL_CONFIRMATION=true
```

Set `THOTD_EXPECTED_VBOX_VERSION` to the qualified `VBoxManage --version` value without the trailing build suffix. Runs fail preflight if the installed version differs. Project parameters supply guest SSH credentials, so no `THOTD_SSH_SECRET_FILE` is required. The example password is public sample data; replace it with the guest credentials for the assignment and protect the project archive with host permissions. Never put real credentials in a public or student-accessible project archive.

Optional `THOTD_DATA_ROOT` changes the default data base used for delivery, artifact, VM, profile, and database paths. Explicit values above are easier to review. Keep `THOTD_STORAGE_ROOT`, the project `vm_path`, the VirtualBox profile, and artifact storage separate. The service checks that its configured profile, storage, and artifact roots do not overlap and that its dedicated storage root is empty before a VM run.

Example systemd unit:

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
ReadWritePaths=/var/lib/thot /etc/thot/projects/delivery /etc/thot/projects/vms

[Install]
WantedBy=multi-user.target
```

Adjust `ReadWritePaths` to the actual configured `delivery_folder` and `vm_path` directories. Review the systemd sandbox against the installed VirtualBox driver and host paths before use; grant only the access required by the service account.

Install the binaries and start the service:

```sh
sudo install -o root -g root -m 0755 dist/thotd /usr/local/sbin/thotd
sudo install -o root -g root -m 0755 dist/thot /usr/local/bin/thot
sudo systemctl daemon-reload
sudo systemctl enable --now thotd
sudo systemctl status thotd
```

## Create and inspect a run

The project ID is the archive filename without `.thot.tgz` or `.thot.zip`. When no delivery override is supplied, the engine uses the project's `delivery_folder`. Alternatively, pass a directory inside `THOTD_DELIVERY_ROOT`:

```sh
thot run create --project linux_competition
# or
thot run create --project linux_competition --deliveries /var/lib/thot/deliveries/batch-01
thot run status <run-id>
thot run expedient <expedient-id>
thot run cleanup <expedient-id>
```

The current POC accepts one active run and processes deliveries sequentially. Check each expedient and cleanup result. A host without VirtualBox can build and run the basic Rust tests, but cannot validate OVA import, guest boot, forwarding, screenshots, SSH, or VM cleanup.
