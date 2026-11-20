# `thotd` engine service

`thotd` validates projects, schedules submissions, stores normalized test results, and calculates grades. VirtualBox tests are submitted to the separate `thot-virtualbox-agent` HTTP service; that service connects to the configured host over SSH and runs `VBoxManage`. See [`deploy/README.md`](../../deploy/README.md) and [`apps/thot-virtualbox-agent/README.md`](../thot-virtualbox-agent/README.md) for setup. The CLI uses the daemon API and never invokes `VBoxManage` itself.

## Project and run files

The canonical project configuration is `configuration.thot.yaml`; legacy `configuration.yaml` is accepted when the canonical file is absent, and a project containing both is rejected. Roster input is `students.csv` or `students.yaml`; assignment input is `assignments.yaml` or `assignments.json`. Projects may be directories or supported `.thot` archives. Paths to packaged scripts and CSV inputs resolve inside the project snapshot.

A directory project stores each run under `runs/<run-id>/`; archive projects use `/var/lib/thot/artifacts/projects/<project-id>/runs/<run-id>/`. A run contains `run.yaml`, `snapshot/project/`, `expedients/<expedient-id>.yaml`, `reports/`, and `tests/<submission-id>/<test-id>/` with `result.yaml`, stdout/stderr, and artifacts. The engine writes expedients; an agent may write only to its assigned test folder. Agent tasks/results use the versioned protocol in `thot-core::agent_protocol`; HTTP routes are `/v1/capabilities` and `/v1/tasks`.

SQLite remains the service's index for run, roster, assignment, and delivery lookups. Project files and run result files provide the portable inputs and evidence. The API currently supports one active run at a time.

## Runtime configuration and trust

The daemon defaults to `127.0.0.1:8081` and `/var/lib/thot/thot.sqlite3`. `THOT_VBOX_AGENT_URL` selects the agent and `THOT_VBOX_AGENT_TOKEN` supplies its optional bearer token. `THOTD_ARTIFACT_ROOT`, `THOTD_STORAGE_ROOT`, timeouts, output limits, and guest firewall confirmation are configured in `/etc/thot/thotd.env`.

External tests are trusted instructor programs and run with daemon-account permissions. VirtualBox guest access still requires host firewall and guest egress controls. Built-in VirtualBox tests use the remote agent, while existing orphan-VM maintenance commands still use the daemon's local `VBoxManage` settings.

## API routes

- `GET /health`
- `POST /import/students`, `GET /get/students`
- `POST /api/v1/runs`, `GET /api/v1/runs/{run_id}`, `POST /api/v1/runs/{run_id}/cancel`
- `GET /api/v1/expedients/{expedient_id}`, `POST /api/v1/expedients/{expedient_id}/cleanup`
- `GET /api/v1/reports/runs`
- `GET /api/v1/reports/deliveries?run_id=...&view=summary|tests|checks`
