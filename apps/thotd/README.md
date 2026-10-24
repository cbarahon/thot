# `thotd` engine service

`thotd` serves the student import API and the local VirtualBox VM activity API.
For a production host account and native installation, follow
[`deploy/README.md`](../../deploy/README.md).

## Runtime configuration

The service defaults to `127.0.0.1:8081` and a SQLite database at
`/var/lib/thot/thot.sqlite3`.
VM activity paths default below `/var/lib/thot`, except projects
(`/etc/thot/projects`) and `VBoxManage` (`/usr/bin/VBoxManage`). Set host paths
explicitly before running a POC. Important environment variables include:

- `THOTD_BIND_HOST`, `THOTD_BIND_PORT`, `THOTD_DATABASE_URL`
- `THOTD_PROJECT_ROOT`, `THOTD_DELIVERY_ROOT`, `THOTD_ARTIFACT_ROOT`
- `THOTD_STORAGE_ROOT`, `THOTD_VBOX_USER_HOME`, `THOTD_VBOX_MANAGE_PATH`
- `THOTD_EXPECTED_VBOX_VERSION`, `THOTD_OVA_SIZE_LIMIT`, `THOTD_FREE_SPACE_RESERVE`
- `THOTD_STARTUP_TIMEOUT`, `THOTD_IMPORT_TIMEOUT`, `THOTD_SSH_TIMEOUT`, `THOTD_SHUTDOWN_GRACE`
- `THOTD_SSH_SECRET_FILE`, `THOTD_FIREWALL_CONFIRMATION`

The API bind address must be loopback. The activity run preflight checks that
the configured VirtualBox version matches, ports 8080 and 2222 are free, the
dedicated profile has no unaccounted VMs, and the protected SSH secret file is
available. `THOTD_FIREWALL_CONFIRMATION=true` is an administrator assertion
after manually checking firewall and guest egress rules.

## API routes

- `GET /health`
- `POST /import/students`, `GET /get/students`
- `POST /api/v1/runs`
- `GET /api/v1/runs/{run_id}`, `POST /api/v1/runs/{run_id}/cancel`
- `GET /api/v1/expedients/{expedient_id}`
- `POST /api/v1/expedients/{expedient_id}/cleanup`

Run creation accepts a project ID and a delivery directory below the configured
delivery root. It performs project and host preflight before scheduling one
sequential delivery worker. Results and evidence are persisted under the
artifact root; run metadata is stored in SQLite.
