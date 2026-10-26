# Thot VM activity POC

**Status:** Project-defined test model implemented; basic Rust tests pass. VirtualBox execution has not been exercised in this environment because VirtualBox is unavailable.

This POC runs teacher-defined checks against OVA deliveries on one Linux host with VirtualBox. It records technical test results and evidence; it does not assign students, calculate marks, or grade submissions. The roster API is separate from this workflow.

## Project bundle

A project is a `.thot.zip` or `.thot.tgz` file with `configuration.yaml` at its archive root. External teacher scripts are stored in the same archive. For example:

```text
linux_competition.thot.tgz
├── configuration.yaml
└── extern/
    └── tests/
        └── vm_start_time.py
```

The configuration defines an ordered test list. A test reference has the form `Platform/test_name`, such as `VirtualBox/import_ova` or `ssh/get_host_info`; a teacher-provided script uses `extern/<archive-relative-path>`. Tests may set scalar `parameters`, a per-test `timeout` in seconds, `requirements` naming earlier tests, and `repeat`. A repeated test produces names such as `start_time_01`, `start_time_02`, and `start_time_03`. Requirements run only when all repetitions of each required test pass.

The example project is [configuration.yaml](../examples/vm-activity-poc/configuration.yaml). Its tests import an OVA, run the external HTTP startup and screenshot check three times, start the VM, collect host information over SSH, stop it, and destroy it. The external script receives parameters as `key=value` arguments and gets `THOT_ARTIFACT_DIR`, `VBOX_USER_HOME`, and `VBOX_MANAGE_PATH` environment variables. Its stdout, stderr, and process exit code are recorded. Internal tests return the same three values through the engine result contract.

Current built-in references are `VirtualBox/import_ova`, `VirtualBox/start_vm`, `VirtualBox/stop_vm`, `VirtualBox/destroy_vm`, and `ssh/get_host_info`. The code is divided by platform under `crates/thot-engine/src/tests/`; adding a platform requires a dispatcher and its own module. Unknown platform/test references fail as test errors.

## Run behavior and limits

The CLI accepts a project archive path directly, for example `thot run create /srv/course/linux_competition.thot.tgz`. Project archives may be anywhere readable by `thotd`; there is no project-root setting. A relative `delivery_folder` resolves from the archive's containing directory, and an absolute value may point anywhere the daemon can read. An optional `--deliveries` override can also point to any readable directory. `thot run` and `thot run list` list runs; `thot run delete <run-id>` removes an inactive run and its related database records. Queued, running, or cancelling runs cannot be deleted. Run IDs use the project archive stem plus its UTC creation time, such as `linux_competition-20261026_154512`; each expedient ID uses its delivery filename plus the same timestamp. Deletion leaves report and VM artifacts on disk.

The configured `vm_path` is relative to the archive's containing directory and defaults to `./thot_vms`. The service creates per-run and per-delivery storage beneath it. The service account therefore needs write access to that VM storage directory. Keep the archive and its scripts protected from modification, and give write access only to the dedicated VM storage location. Do not use a path that overlaps the VirtualBox profile or artifact root.

The current daemon permits one active run and processes deliveries and tests sequentially. `concurrency` is parsed as a project setting but is not yet used to run students in parallel. VM checks require loopback ports 8080 and 2222 to be free. Imported guests must use NAT forwarding from `127.0.0.1:8080` to guest port 80 and `127.0.0.1:2222` to guest port 22. The import test rejects unsupported network configuration and applies isolation settings before later tests run.

External tests are executable teacher code. They run as child processes with the daemon's operating-system permissions, bounded by each test timeout and the configured stdout/stderr output limit. The process boundary does not sandbox scripts. Only install project archives from trusted instructors, and run the daemon under a dedicated unprivileged account and VirtualBox profile. ZIP and TGZ extraction rejects traversal paths and links and enforces archive size and entry limits.

## Results and artifacts

Each delivery has a `delivery.json` expedient under the artifact root. It contains project and delivery identifiers, test references and statuses, timestamps and durations, stdout, stderr, exit code, diagnostics, observations, evidence references, cleanup status, and overall orchestration status. Raw test output is also written to files. Artifacts use this layout:

```text
<artifact-root>/<project-id>/<run-id>/<delivery-id>/
├── delivery.json
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

The workspace has been checked with `cargo check --workspace --locked` and `cargo test --workspace --locked`. Unit tests cover project YAML parsing and validation, ZIP/TGZ extraction including traversal rejection, artifact path creation, and external-process output, timeout, and exit-code handling. Python syntax can be checked without a VM. No VirtualBox import, guest boot, NAT forwarding, screenshot, SSH connection, or VM cleanup integration test was run because VirtualBox is not installed in this environment.
