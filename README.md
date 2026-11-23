# Thot

Thot is a project-based assessment tool for professors. It collects student
deliverables from a configured source, runs instructor-defined checks in
disposable Linux/KVM guests, captures evidence, and evaluates results with
checks and a rubric. A read-only delivery-agent interface currently supports
local folders and Google Drive. The included Alpine KVM workflow is a
lightweight, usable CLI path; it is still a limited platform implementation,
not the complete managed sandbox service described in the architecture docs.

Thot also includes `thotd`, an HTTP service for the existing project, roster,
assignment, report, and VirtualBox activity workflows. These workflows are
separate from `thot sandbox run`; read the relevant guides before selecting a
deployment model.

## Install

### Install a published Linux release

On a supported x86_64 Linux system with systemd, download the installer from
the [latest GitHub release](https://github.com/cbarahon/thot/releases/latest):

```sh
curl -fLO https://github.com/cbarahon/thot/releases/latest/download/thot-install.sh
sudo bash thot-install.sh
```

The installer sets up `thot`, `thotd`, the VirtualBox agent, systemd services,
and service configuration. It does not install KVM/libvirt or VirtualBox. For
the local KVM evaluation workflow, install its host dependencies separately.
On Debian 13:

```sh
sudo apt update
sudo apt install qemu-system-x86 libvirt-daemon-system libvirt-clients virtinst \
  libguestfs-tools cloud-image-utils curl
sudo systemctl enable --now libvirtd
sudo virsh net-autostart default
```

Confirm `/dev/kvm` exists and the libvirt `default` network is active. The
network is used for the Alpine preparation phase; student guests have no
network device. The installer places the static `thot-alpine-agent` next to the
CLI. See [deployment instructions](deploy/README.md) for details and the
VirtualBox configuration path.

### Build from source

Install a Rust stable toolchain and the platform dependencies above, then run:

```sh
cargo build --release -p thot-cli -p thot-alpine-agent
```

The Alpine guest agent needs the musl target and `musl-tools`:

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl -p thot-alpine-agent
```

The workspace development commands are `make build`, `make test`, `make fmt`,
and `make lint`.

## Getting started: evaluate a project

Start with the runnable synthetic project in
[`examples/kvm-alpine-poc`](examples/kvm-alpine-poc/). It contains a project
configuration, sample student roster and assignments, instructor tests, and
three sample submissions. Copy it to a writable location before editing or
running it so generated results stay with your copy.

```sh
cp -a examples/kvm-alpine-poc "$HOME/thot-course-project"
thot deliveries list "$HOME/thot-course-project"
sudo thot sandbox run "$HOME/thot-course-project"
```

The first run downloads and verifies a pinned Alpine cloud image, prepares a
reusable project baseline, creates a disposable guest for each student, runs
the configured tests, and writes result JSON under
`<project>/results/<evaluation-id>/`. The sample students should score 10/10,
2/10, and 0/10. Subsequent runs reuse the local image and prepared baseline
when possible. The CLI runs KVM locally and does not require `thotd`.

Use `thot deliveries list PROJECT_DIR` to inspect names and sizes from the
configured delivery source. Retrieve one item with:

```sh
thot deliveries get student1 "$HOME/thot-course-project" --output /tmp/student1
```

The project YAML selects the delivery source. If `delivery_agent` is omitted,
Thot uses the configured local `delivery_folder`; if that is omitted, it uses
the project’s existing local delivery layout. The Google Drive agent reads a
configured Drive folder with read-only OAuth. It requires a Google Cloud OAuth
Desktop client, Drive API enabled, and professor consent. See
[`docs/DELIVERY_AGENTS.md`](docs/DELIVERY_AGENTS.md) before configuring it.

To adapt the sample for a course, edit `configuration.thot.yaml`, the roster,
assignments, instructor tests, and delivery folder. Assignments refer to exact
names shown by `thot deliveries list`. Test commands and professor preparation
scripts execute inside the guest; treat those instructor-authored files and
configuration as trusted inputs. Review the [KVM POC limitations and status](docs/KVM_ALPINE_POC.md)
before using it for consequential grading.

## Other workflows

- **Daemon and existing project runs:** install the service, configure a
  project, then use `thot run create PROJECT_PATH` and the `thot run status`,
  `thot run expedient`, and `thot report` commands. See the
  [CLI guide](apps/thot-cli/README.md) and [deployment guide](deploy/README.md).
- **VirtualBox activity evaluation:** the daemon and VirtualBox agent support
  the existing VM activity workflow. It has separate host and agent setup; it
  is not the KVM Alpine sandbox. See
  [VirtualBox deployment](apps/thot-virtualbox-agent/README.md).

## Project layout

- `apps/`: CLI, daemon, Alpine guest agent, and VirtualBox agent.
- `crates/`: reusable core types, delivery agents, API, engine, and runners.
- `docs/`: architecture, requirements, contracts, deployment, and ADRs.
- `examples/`: sample projects, including the Alpine KVM fixture.
- `deploy/`: release installer and service assets.

Start with the [documentation index](docs/README.md) for architecture and
implementation status. Thot is licensed under AGPL-3.0-or-later.
