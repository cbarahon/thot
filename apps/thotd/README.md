# `thotd` engine service

`thotd` serves the student import API and the local VirtualBox activity API. For native host setup, see [`deploy/README.md`](../../deploy/README.md).

## Project execution

The activity API accepts an absolute project archive path. The archive root must contain `configuration.yaml`; external tests are stored at paths such as `extern/tests/check.py`. Project configuration controls test order, parameters, timeout, prerequisites, and repeats. A relative `delivery_folder` is resolved from the archive's containing directory; an absolute value is used directly. A run can also supply a delivery directory override.

Built-in tests are divided under `crates/thot-engine/src/tests/` by platform. External test processes receive `key=value` arguments and `THOT_ARTIFACT_DIR`. Both built-in and external results save status, stdout, stderr, exit code, diagnostics, observations, and evidence. Per-test artifacts are under each delivery's `test_records/<filename>/<test name>/artifacts` directory.

External tests are trusted teacher programs. They run as child processes with the daemon account's permissions and receive timeout and output limits; this is not an isolation sandbox. Run `thotd` under a dedicated unprivileged account. The POC currently allows one active run and processes deliveries sequentially. The configuration's concurrency value is accepted but is not yet used for parallel execution.

## Runtime configuration

The service defaults to `127.0.0.1:8081` and a SQLite database at `/var/lib/thot/thot.sqlite3`. VM activity paths default below `/var/lib/thot`, except projects (`/etc/thot/projects`) and `VBoxManage` (`/usr/bin/VBoxManage`). Important variables include:

- `THOTD_BIND_HOST`, `THOTD_BIND_PORT`, `THOTD_DATABASE_URL`
- `THOTD_ARTIFACT_ROOT`
- `THOTD_STORAGE_ROOT`, `THOTD_VBOX_USER_HOME`, `THOTD_VBOX_MANAGE_PATH`
- `THOTD_EXPECTED_VBOX_VERSION`, `THOTD_OVA_SIZE_LIMIT`, `THOTD_FREE_SPACE_RESERVE`
- `THOTD_STARTUP_TIMEOUT`, `THOTD_IMPORT_TIMEOUT`, `THOTD_SSH_TIMEOUT`, `THOTD_SHUTDOWN_GRACE`
- `THOTD_OUTPUT_LIMIT`, `THOTD_FIREWALL_CONFIRMATION`

VM tests require a pinned VirtualBox version, a dedicated profile/storage configuration, free loopback ports 8080 and 2222, and administrator confirmation of firewall and guest egress policy. Guest SSH credentials come from the project's test parameters. Protect project archives that contain real credentials.

## API routes

- `GET /health`
- `POST /import/students`, `GET /get/students`
- `POST /api/v1/runs`
- `GET /api/v1/runs/{run_id}`, `POST /api/v1/runs/{run_id}/cancel`
- `GET /api/v1/expedients/{expedient_id}`
- `POST /api/v1/expedients/{expedient_id}/cleanup`

Run metadata is stored in SQLite and expedients and evidence are persisted under the artifact root. VirtualBox integration requires a Linux host with VirtualBox and is not validated by the basic tests when VirtualBox is unavailable.
