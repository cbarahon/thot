# Deploy Thot

Thot is a Rust workspace with three runtime pieces: `thotd` coordinates project runs, `thot-virtualbox-agent` executes VirtualBox tests as an independent HTTP service, and `thot` is the CLI. The agent connects to the VirtualBox host over SSH and invokes `VBoxManage` there. Built-in VirtualBox tests do not run `VBoxManage` inside the engine; the existing orphan-VM maintenance commands still use the daemon's local VirtualBox settings.

## Build and install

Create a release with `make deploy` after committing and pushing the release version. GitHub Actions publishes the CLI, daemon, agent, systemd units, and sample configuration. Install a release on an x86_64 Linux systemd machine with:

```sh
sudo bash thot-install.sh
```

The installer creates the `thotd` service account, installs both services, and places configuration at `/etc/thot/thotd.env` and `/etc/thot/thot-virtualbox-agent.config.yaml`. It does not install VirtualBox on the remote execution host.

## Configure the VirtualBox agent

Edit `/etc/thot/thot-virtualbox-agent.config.yaml`. Set `ssh.host`, `ssh.user`, `ssh.port`, `ssh.known_hosts`, the remote `ssh.vbox_user_home`, and the exact qualified `ssh.expected_vbox_version`. Configure `allowed_output_roots`, `allowed_delivery_roots`, and `allowed_vm_storage_roots` to contain only approved paths. The sample maps engine paths `/srv/thot-projects`, `/var/lib/thot/artifacts`, and `/var/lib/thot/vms` to `/srv/thot-projects`, `/srv/thot-artifacts`, and `/srv/thot-vms` on the VirtualBox host. Mount or copy the project and retained OVA into those remote paths, and create `/srv/thot-vms` writable by the SSH account. Adjust both roots and mappings when your host uses different mounts. The engine and agent must both access each test output folder so agent evidence is available to the engine.

The agent creates an Ed25519 key pair on first startup and logs its public key. Add that key to the configured host account's `~/.ssh/authorized_keys`; that account must be able to run `VBoxManage` and write only to the dedicated VirtualBox profile and VM storage. Pin the host key in the configured `known_hosts` file. Do not enable password login, forwarding, or an interactive shell for agent use. Restart with `sudo systemctl restart thot-virtualbox-agent` after configuration changes. Check `journalctl -u thot-virtualbox-agent` for SSH, version, and profile readiness errors.

Set `THOT_VBOX_AGENT_URL` in `/etc/thot/thotd.env` to the agent URL. If the service listens beyond loopback, configure a protected `auth_token` in the agent file and set the same `THOT_VBOX_AGENT_TOKEN` for `thotd`. Keep the agent on a trusted network and firewall its HTTP port.

## Project files and run layout

A project is an unpacked directory or supported `.thot.gz`, `.thot.tgz`, or `.thot.zip` archive. Its canonical configuration is `configuration.thot.yaml` (legacy `configuration.yaml` is accepted). Rosters use `students.csv` or `students.yaml`; assignments use `assignments.yaml` or `assignments.json`. Deliverables are selected by `delivery_folder` or a CLI/API override.

Runs are stored under the project directory at `runs/<run-id>/`; archive projects use `/var/lib/thot/artifacts/projects/<project-id>/runs/<run-id>/`. Each run contains `run.yaml`, an immutable `snapshot/project/`, `expedients/<expedient-id>.yaml`, and per-test outputs in `tests/<submission-id>/<test-id>/`. Agents may write only inside their assigned test folder; expedient reports are written by the engine.

```sh
thot run create /srv/thot-projects/course/project
thot run status <run-id>
thot run expedient <expedient-id>
thot run cleanup <expedient-id>
thot run delete <run-id>
```

Deleting a finished run removes its run folder and database records. Project files and original deliveries remain. Test VM integration requires an SSH-accessible VirtualBox host with the pinned version, a dedicated empty profile, approved storage paths, and shared output/delivery access. A local workspace `cargo check --workspace` does not validate that host integration.
