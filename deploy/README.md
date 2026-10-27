# Native Linux VirtualBox deployment

Thot runs natively on an x86_64 Linux host with systemd and VirtualBox installed. It uses a dedicated unprivileged `thotd` account and VirtualBox profile. The container deployment does not provide the host VirtualBox registry or kernel driver required by this POC.

## Build and publish a release

GitHub Releases are the distribution channel. A release tag such as `v0.1.0` triggers GitHub Actions to build `thot` and `thotd` on Ubuntu 22.04, package the binaries and service files, create a SHA-256 checksum, and publish a release. The build supports x86_64 Linux hosts with compatible glibc, OpenSSL 3, zlib, and zstd runtime libraries; the installer checks binary dependencies. Cargo and Rust are not needed on installation servers.

Merge and push changes to `main` first. For a new release, update `[workspace.package].version` in `Cargo.toml`, run `cargo check --offline --workspace` to refresh the lockfile, then commit and push both files on `main`. Finally run:

```sh
make deploy
```

The target refuses a dirty or unpushed worktree, builds both release binaries locally, and pushes an annotated `v<version>` tag for the current `main` commit. It does not commit changes or connect to any server. GitHub Actions publishes the downloadable release after the tag push; check the Actions run before installing it. A version tag can be published only once. `DEPLOY_BRANCH` defaults to `main` and can be overridden for a different release branch.

## Install or update on a server

Install VirtualBox and configure the host firewall and guest egress policy first. Then download and review the installer, and run it as root:

```sh
curl --fail --location --output thot-install.sh \
  https://github.com/cbarahon/thot/releases/latest/download/thot-install.sh
curl --fail --location --output SHA256SUMS \
  https://github.com/cbarahon/thot/releases/latest/download/SHA256SUMS
set -o pipefail
awk '$2 == "thot-install.sh" { print; count++ } END { if (count != 1) exit 1 }' SHA256SUMS \
  | sha256sum --check
sed -n '1,240p' thot-install.sh
sudo bash thot-install.sh
```

The installer supports repeat runs and preserves `/etc/thot/thotd.env` and data. It downloads the latest stable release over HTTPS, verifies the binary archive against the release SHA-256 checksum, installs both binaries and the systemd unit, creates the service account and data directories, then enables and restarts `thotd`. To install a specific release, pass `THOT_VERSION` through `sudo`:

```sh
sudo env THOT_VERSION=v0.1.0 bash thot-install.sh
```

The installer checks that `VBoxManage`, `/dev/vboxdrv`, and systemd are available. If `/dev/vboxdrv` is group-owned by `vboxusers`, it adds `thotd` to that group so it can use the driver. This group may also permit host USB access; Thot disables USB for imported guests. The installer does not install VirtualBox or change firewall rules.

The installer creates `/etc/thot/thotd.env` and starts with `THOTD_FIREWALL_CONFIRMATION=false`. Review the file and set this value to `true` only after the firewall and guest egress policy are configured. The expected VirtualBox version is detected during initial installation and pinned in the environment file. When updating VirtualBox itself, update that value to the new `VBoxManage --version` without its trailing `r<build>` suffix.

Rust, Cargo, Docker, the web UI, and lighttpd are not needed on the server.

## Prepare the host

Install VirtualBox from the host's approved package source. The service account must be allowed to use its kernel driver. Set up separate service-owned locations for artifacts, VM storage, and the VirtualBox profile. Project archives and delivery directories can be stored anywhere the service account can access. Do not run `thotd` as root or reuse a teacher's normal VirtualBox profile.

Example paths:

```text
service account: thotd
artifacts:      /var/lib/thot/artifacts
VM storage:     /var/lib/thot/vms
VBox profile:   /var/lib/thot/vbox-profile
```

The project archive can be placed anywhere readable by the service account. Keep it and its scripts read-only to the daemon. A relative `delivery_folder` is resolved from the archive's containing directory; an absolute folder can be anywhere the service can read. The project `vm_path` remains relative to the archive's containing directory, so provision a separate writable subdirectory such as `vms/` next to the archive.

```sh
sudo install -d -o root -g thotd -m 0750 /etc/thot/projects
sudo install -d -o thotd -g thotd -m 0750 \
  /etc/thot/projects/vms /etc/thot/projects/delivery \
  /srv/course/linux-competition-deliveries
sudo install -o root -g thotd -m 0640 linux_competition.thot.tgz \
  /etc/thot/projects/linux_competition.thot.tgz
```

Project extraction happens into a run-scoped snapshot under the artifact root. Treat external scripts as trusted instructor code: they execute as child processes with the daemon account's access to the host and VirtualBox profile. The runner applies per-test timeouts and output limits, but it is not a script sandbox.

The service requires free loopback TCP ports 8080 and 2222 before VM runs. Guest networking must forward guest TCP 80 to `127.0.0.1:8080` and guest TCP 22 to `127.0.0.1:2222` through NAT. Configure host firewall and guest egress rules to block access to unrelated host services and unnecessary destinations. Do not use bridged networking or attach host folders, clipboard, drag-and-drop, or USB devices to imported guests. The installer leaves `THOTD_FIREWALL_CONFIRMATION=false`; change it only after the administrator has configured and checked the firewall policy.

## Configure `thotd`

The installer creates `/etc/thot/thotd.env` owned by root and readable by the service account (mode `0640`). It pins the installed VirtualBox version and uses these defaults:

```ini
THOTD_BIND_HOST=127.0.0.1
THOTD_BIND_PORT=8081
THOTD_DATABASE_URL=sqlite:/var/lib/thot/thot.sqlite3
THOTD_ARTIFACT_ROOT=/var/lib/thot/artifacts
THOTD_STORAGE_ROOT=/var/lib/thot/vms
THOTD_VBOX_USER_HOME=/var/lib/thot/vbox-profile
THOTD_VBOX_MANAGE_PATH=/usr/bin/VBoxManage
THOTD_EXPECTED_VBOX_VERSION=<detected-version-without-r-build-suffix>
THOTD_OVA_SIZE_LIMIT=10737418240
THOTD_FREE_SPACE_RESERVE=10737418240
THOTD_OUTPUT_LIMIT=1048576
THOTD_IMPORT_TIMEOUT=1800
THOTD_STARTUP_TIMEOUT=180
THOTD_SSH_TIMEOUT=120
THOTD_SHUTDOWN_GRACE=30
THOTD_FIREWALL_CONFIRMATION=false
```

Runs fail preflight if the installed VirtualBox version differs from the pinned value. Project parameters supply guest SSH credentials, so no `THOTD_SSH_SECRET_FILE` is required. The example password is public sample data; replace it with the guest credentials for the assignment and protect the project archive with host permissions. Never put real credentials in a public or student-accessible project archive.

Optional `THOTD_DATA_ROOT` changes the default data base used for artifact, VM, profile, and database paths. Explicit values above are easier to review. Keep `THOTD_STORAGE_ROOT`, the project `vm_path`, the VirtualBox profile, and artifact storage separate. The service checks that its configured profile, storage, and artifact roots do not overlap and that its dedicated storage root is empty before a VM run.

The installed systemd unit runs as `thotd`, keeps a private `/tmp`, and protects system binaries and configuration from writes. It deliberately sets `NoNewPrivileges=false`: VirtualBox headless startup failed on the qualified host when privilege gains were blocked. Project-provided scripts must therefore be trusted instructor code; they run as `thotd` and can use setuid or file-capability programs that the service account can execute.

Project archives and delivery directories can be anywhere accessible to `thotd`. The default writable project VM paths are `/srv` and `/etc/thot/projects/vms`; if a project's `vm_path` is elsewhere in a protected system directory, add it to the service's `ReadWritePaths` with `sudo systemctl edit thotd`. The installer includes the service definition at `deploy/thotd.service`.

The installer is the recommended path because it creates the environment file and service-owned directories consistently. After installing, place a teacher-created `.thot.zip` or `.thot.tgz` project archive and its delivery directory on the host. The archive must contain `configuration.yaml` at its root; external checks such as `extern/tests/vm_start_time.py` are packaged beside it. The repository example is in `examples/vm-activity-poc/`.

## Manual build

Build on a compatible Linux x86_64 machine when a GitHub release is not suitable:

```sh
cargo build --release --locked -p thotd -p thot-cli
```

Install `thot` and `thotd` from the same build, install `deploy/thotd.service`, create `/etc/thot/thotd.env` from `deploy/thotd.env.example` with the actual VirtualBox version, and create the service-owned paths shown above. Restart `thotd` after replacing either binary.

## Create and inspect a run

Pass the path to the project archive directly. With no delivery override, the engine uses `delivery_folder`; relative paths resolve from the archive's containing directory, and absolute paths are accepted. An optional delivery override may also point anywhere the daemon can read:

```sh
thot run
# equivalent:
thot run list
thot run create /etc/thot/projects/linux_competition.thot.tgz
# or
thot run create /etc/thot/projects/linux_competition.thot.tgz --deliveries /srv/course/linux-competition-deliveries
thot run status <run-id>
thot run expedient <expedient-id>
thot run cleanup <expedient-id>
thot run delete <run-id>
```

Run IDs use the project archive name and its UTC creation time, for example `linux_competition-20261026_154512`. Expedient IDs use the delivery filename and the same run timestamp. Listing returns run status, creation time, project and delivery paths, and expedient IDs. Deleting a run removes its run, expedient, and unreferenced delivery records from the database; it leaves report files, VM files, and other artifacts untouched. A queued, running, or cancelling run cannot be deleted.

The current POC accepts one active run and processes deliveries sequentially. Check each expedient and cleanup result. A host without VirtualBox can build and run the basic Rust tests, but cannot validate OVA import, guest boot, forwarding, screenshots, SSH, or VM cleanup.
