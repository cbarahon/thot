# Thot VM activity POC

**Status:** Project-defined test model implemented. The latest workspace changes compile; VirtualBox execution has not been exercised in this environment because VirtualBox is unavailable.

This POC runs teacher-defined checks against OVA deliveries on one Linux host with VirtualBox. It records technical test results and evidence. Project-scoped student rosters and manual or suggested delivery assignments are supported; scoring and learning-outcome evaluation are not yet implemented.

## Project bundle

A project can be a `.thot.zip`, `.thot.tgz`, or `.thot.gz` archive with `configuration.yaml` at its archive root, or an unpacked project directory with that file at its root. External teacher scripts are stored in the project. For example:

```text
linux_competition.thot.tgz
├── configuration.yaml
└── extern/
    └── tests/
        └── vm_start_time.py
```

The configuration defines an ordered test list. A test reference has the form `Platform/test_name`, such as `VirtualBox/import_ova` or `ssh/get_host_info`; a teacher-provided script uses `extern/<archive-relative-path>`. Tests may set scalar `parameters`, a per-test `timeout` in seconds, `requirements` naming earlier tests, and `repeat`. A repeated test produces names such as `start_time_01`, `start_time_02`, and `start_time_03`. Requirements run only when all repetitions of each required test pass.

An optional root `students.csv` may define the project's roster. It is imported automatically when a run is created, and rows are stored under that project. The CSV must include name and email columns; common English, Spanish, and Catalan header aliases are accepted. A missing or blank external-ID value is replaced with the student's email, so every stored project student has an external ID. CLI student import and retrieval commands also require a current project and include its ID, name, and path in their responses.

The example project is [configuration.yaml](../examples/vm-activity-poc/configuration.yaml). Its tests import an OVA, run the external HTTP startup and screenshot check three times, start the VM, collect host information over SSH, stop it, and destroy it. The external script receives parameters as `key=value` arguments and gets `THOT_ARTIFACT_DIR`, `VBOX_USER_HOME`, and `VBOX_MANAGE_PATH` environment variables. Its stdout, stderr, and process exit code are recorded. Internal tests return the same three values through the engine result contract.

The loader snapshots project contents before execution. For directory projects, it excludes the configured delivery directory from the immutable project snapshot; relative project paths resolve from the project directory. For archives, relative paths resolve from the archive's containing directory. Absolute delivery paths are accepted when readable by the daemon. Project `vm_path` remains relative to the project location and must resolve safely beneath it.

Current built-in references are `VirtualBox/import_ova`, `VirtualBox/start_vm`, `VirtualBox/stop_vm`, `VirtualBox/destroy_vm`, and `ssh/get_host_info`. The code is divided by platform under `crates/thot-engine/src/tests/`; adding a platform requires a dispatcher and its own module. Unknown platform/test references fail as test errors.

## Run behavior and limits

The CLI accepts either a project path directly, for example `thot run create /srv/course/linux_competition.thot.tgz`, or discovers one when run with no subcommand. Bare `thot run` selects exactly one `.thot.gz`, `.thot.tgz`, `.thot.zip`, or root `configuration.yaml` from the current directory and reports ambiguity when more than one is present. `thot run list` lists saved runs. `thot run status` resolves the run only when exactly one run is queued, running, or cancelling. Its response includes one expedient per student with the assigned delivery filename and a lifecycle status of `pending`, `running`, or `finished`. `thot run delete <run-id>` removes an inactive run and its related database records. Queued, running, or cancelling runs cannot be deleted. Run IDs use the project name plus its UTC creation time, such as `linux_competition-20261026_154512`; each per-student expedient ID includes its delivery filename, the same timestamp, and the student ID. Successful run and expedient CLI responses are displayed as YAML while the HTTP API remains JSON. Deletion leaves report and VM artifacts on disk until `thot clean` removes data proven to be orphaned.

Run creation selects the current project. Student and assignment commands invoked from a directory containing root `configuration.yaml` select that project's directory; elsewhere they use the most recently selected project. Assignment guess/get/import index the configured delivery folder before a run exists. Guessing compares project students to those deliveries using names, surnames, email usernames and addresses, external IDs, and student IDs, with a second pass that rewards agreement across identity fields. Review suggestions with `thot assignments get`, then replace them with `thot assignments import FILE`. Import resolves each row using only `student_external_id` and `delivery_filename`; other row fields are ignored and populated from current project data. The import response includes the count of valid records and per-record errors with corrections. Valid rows replace the saved mapping; invalid rows are omitted. Several students can share one delivery; each student has at most one delivery assignment. Before creating a run, every project student and every OVA must be assigned. Guess, get, and import responses identify the project and list unassigned deliveries. A root `assignments.yaml` or `assignments.json` is imported automatically during run creation and replaces saved assignments; having both files is an error.

Run creation creates a pending expedient for each assigned student. During execution, expedients for students sharing a delivery become running together; the tests execute once for that delivery and the resulting report and test evidence are copied to each student's expedient. Each copied report identifies its student while retaining the shared test-result IDs and output. The run-status response lists all of its student expedients and their pending, running, or finished states.

The configured `vm_path` is relative to the archive's containing directory (or the project directory for an unpacked project) and defaults to `./thot_vms`. Thot creates `<vm_path>/<run-id>/<delivery-id>` for each imported VM. The daemon account (usually `thotd`) needs write and execute permission on the existing `vm_path` directory; if it does not exist yet, the account needs write and execute permission on its nearest existing parent to create it. Pre-creating `vm_path` and granting access there avoids granting write access to the project folder. Keep the archive and its scripts protected from modification, and give write access only to the dedicated VM storage location. Do not use a path that overlaps the VirtualBox profile or artifact root.

The daemon runs tests in declared order once per distinct assigned delivery and bounds concurrency by the project setting. All student expedients assigned that same delivery receive a copy of the shared run result. Projects referencing VirtualBox or SSH tests must set concurrency to one while the adapters use shared loopback ports 8080 and 2222. VM checks require those ports to be free. `VirtualBox/import_ova` records VM information and process output but does not grade hardware or network values; an evaluation phase can assess them. After a successful import, the engine separately applies host-isolation settings before later tests run. If that setup fails, the import test remains passed, later tests are skipped, and each assigned student's expedient records the orchestration error.

Projects may define a `checks` list after `tests`. Each check has a unique `name`, optional `description`, internal implementation name in `check`, related configured test name in `test`, and optional scalar `parameters`. The initial `pass` check returns `1` when the named test passed (all repetitions must pass), or `0` otherwise. Each student expedient records check rows under `checks`, with `name`, `description`, numeric `result`, nullable `err`, and an RFC 3339 `date_time`. Checks are evaluated after the tests and copied with the shared delivery results to each assigned student's expedient.

Maintenance commands are available through the CLI. `thot clean` removes only orphan data whose ownership can be established. `thot prune` keeps the latest run per project, requests confirmation, deletes older run records, and then cleans orphaned data. `thot reset` requires a generated six-digit confirmation code and clears Thot runtime/database records while retaining installation configuration, project archives, and original delivery files. Files and VMs whose ownership cannot be proven are preserved and reported.

External tests are executable teacher code. They run as child processes with the daemon's operating-system permissions, bounded by each test timeout and the configured stdout/stderr output limit. The process boundary does not sandbox scripts. Only install project archives from trusted instructors, and run the daemon under a dedicated unprivileged account and VirtualBox profile. ZIP and TGZ extraction rejects traversal paths and links and enforces archive size and entry limits.

## Results and artifacts

Each student expedient is stored under the artifact root and identifies the student and assigned delivery. Students sharing a delivery receive separate report files with the same test results and evidence; the tests and evidence are produced once in the shared delivery directory. Reports contain project and delivery identifiers, test references and statuses, timestamps and durations, stdout, stderr, exit code, diagnostics, observations, evidence references, cleanup status, and overall orchestration status. Each report reserves an `evaluation` field; score calculation is not implemented yet. Raw test output is also written to files. Artifacts use this layout:

```text
<artifact-root>/<project-id>/<run-id>/<delivery-id>/
├── <delivery>-<timestamp>-student-<student-id>.json
├── <delivery>-<timestamp>-student-<other-id>.json
├── cleanup-manifest.json
└── test_records/
    └── <sanitized-delivery-filename>/
        └── <test-name>/
            ├── stdout.txt
            ├── stderr.txt
            └── artifacts/
```

Teacher scripts should save screenshots and other files under `THOT_ARTIFACT_DIR`. Test statuses distinguish `passed`, `failed`, `skipped`, `timed_out`, and `error`. VM cleanup runs after the configured test list as a manifest-scoped safety step, even when a test fails. A configured `VirtualBox/destroy_vm` test performs that cleanup earlier as part of the test sequence.

## Host requirements and safety

Run the daemon natively on the Linux VirtualBox host; this POC does not support containerized VirtualBox execution. Use a dedicated unprivileged service account, a dedicated `VBOX_USER_HOME`, an administrator-qualified VirtualBox version, and the configured firewall confirmation. Keep guest forwarding bound to loopback and block guest access to unrelated host services and unnecessary outbound destinations. Do not share host folders, clipboard, drag-and-drop, or USB devices with imported guests.

The cleanup manifest records the run, delivery, VM name and UUID, storage path, and archive parent. Cleanup checks VM identity and path ownership before unregistering or removing anything. A failed or interrupted cleanup remains visible in the expedient and requires operator review. These controls reduce accidental host exposure; they do not make arbitrary guest images or instructor scripts safe to run on a general-purpose workstation.

## Verification status

The latest code changes were checked with `cargo check --offline --workspace --all-targets`. This compiles workspace targets without exercising VirtualBox. No VirtualBox import, guest boot, NAT forwarding, screenshot, SSH connection, or VM cleanup integration test was run because VirtualBox is not available in this environment.
