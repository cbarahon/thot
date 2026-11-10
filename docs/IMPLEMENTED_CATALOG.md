# Implemented tests, checks, rubrics, and evaluation

This reference lists the test adapters and evaluation methods currently implemented by the engine. Project files configure tests under `tests`, checks under `checks`, rubric aggregations under `rubrics`, and final evaluation aggregations under `evaluation` in `configuration.yaml`.

| Kind | Reference or methods | Purpose |
|---|---|---|
| Test | `VirtualBox/import_ova` | Imports an assigned OVA and records VM configuration and ownership data. |
| Test | `VirtualBox/start_vm`, `VirtualBox/stop_vm`, `VirtualBox/destroy_vm` | Starts, shuts down, and removes the VM owned by the delivery run. |
| Test | `ssh/get_host_info` | Reads host, OS, memory, and filesystem information over SSH. |
| Test | `network/nmap` | Scans a host's TCP ports and reports open services using Nmap. |
| Test | `libguestfs/disk_info`, `libguestfs/linux_rootfs_info`, `libguestfs/linux_tinycore_info` | Inspects VM disk images without booting them. |
| Test | `stats/regex_extract`, `stats/yaml_extract` | Summarizes values extracted from earlier tests for one delivery. |
| Test | `aggr/select` | Selects exactly one earlier test result matching a condition and forwards its process output. |
| Test | `aggr/join` | Joins process output from earlier tests, optionally filtering the selected results. |
| Test | `import/csv` | Imports stdout, stderr, and exit code from a project CSV by student external ID; can override another test result. |
| Test | `extern/<path>` | Runs a project-provided Python, shell, or executable test program. |
| Check | `pass`, `regex_match`, `regex_extract`, `yaml_match`, `yaml_extract`, `stats/regex_extract`, `stats/yaml_extract`, `aggr/checks`, `import/csv` | Evaluates tests, aggregates check results, or imports per-student check results. |
| Rubric | `avg`, `max`, `min`, `median`, `p1`…`p99`, `weighted_avg`, `sum`, `import/csv` | Aggregates configured checks or imports per-student rubric marks. |
| Evaluation | `avg`, `max`, `min`, `median`, `p1`…`p99`, `weighted_avg`, `sum`, `import/csv` | Aggregates rubric marks or imports per-student final marks for a learning outcome. |

## Configuration and result conventions

Each configured test has a unique `name` and a `test` reference. It can also define an optional student-facing `description`, scalar `parameters`, a test-specific `timeout`, earlier-test `requirements`, and `repeat`. Repetitions produce result IDs such as `boot_01`, `boot_02`. Check evaluation happens after tests; tests that require another test only run when all repetitions of that requirement pass.

Test results record `test_id`, `test_reference`, the optional configured `test_description`, `test_result_id`, `status`, `exit_code`, timing, `stdout`, `stderr`, `diagnostics`, artifact paths, and evidence. Structured test data is emitted through `stdout`; expedient test rows do not duplicate it in an `observation` field. A passed test has exit code 0, a failed test normally has a nonzero code, and timeout or setup errors may have no process exit code. Checks produce 0 or 1; malformed inputs or unavailable properties are reported in the check row's `err` field and have result 0.

Configured parameters are scalar values unless a test describes a list. The engine passes project-provided test parameters as `key=value` arguments to external scripts. Use `timeout` at the test level to override the project's global timeout.

Any test may set `filter_grep` to a regular expression, or `filter_jsonpath` to a JSONPath expression for JSON/YAML stdout. Each accepts one expression or a list; either may also be a mapping from output labels to expressions. Use a string or list when you want unlabeled raw filter output; use a mapping only when you want labels. An unlabeled JSONPath expression emits its selected value directly, and a list emits selected values as a YAML list. An unlabeled grep expression emits the matching lines, with a list of expressions combining matching lines once in original order. In a labeled grep mapping, each regex independently selects matching lines: one match becomes a string and multiple matches become a list. Every JSONPath expression queries the complete original stdout document; list results are combined in expression order. A labeled mapping produces a YAML object with each label as a key; a label selecting one value gets that value, while multiple matches become a list. If a filter yields no output, its result is left empty and the other filters continue; a missing match does not change the test status. When both filters are set, JSONPath runs first and grep scans the complete resulting YAML text. Filter parameters are handled by the engine and are not passed to the test implementation. The expedient contains filtered stdout; each test's `stdout.txt` and `stderr.txt` artifacts retain the original process output. Invalid filter expressions are rejected when the project is loaded.

For example, a labeled mapping keeps the meaning of selected values:

```yaml
parameters:
  filter_jsonpath:
    filesystem_size_bytes: $.linux_root_filesystem.filesystem_size_bytes
    distribution: $.operating_system.distro
    tvadmin: "$.users[?(@.username == 'tvadmin')]"
```

Alternatively, `filter_jsonpath: [$.hostname, $.kernel]` combines selected values into a list, while a list of `filter_grep` patterns retains lines matching any of them.

Use a labeled grep mapping to keep matched output lines identifiable:

```yaml
parameters:
  filter_grep:
    os_release: '^NAME='
    kernel: '^Linux version '
```

The resulting YAML stdout contains `os_release` and `kernel` keys, each holding its matching line or a list when a pattern matches several lines.

When `parameters` uses the sequence-of-mappings form, labeled filters can also be written as a list of one-label mappings, matching the project's other parameter style:

```yaml
parameters:
  - filter_grep:
      - 'Disk info': 'vmdisk'
      - 'Memory (Mb)': 'memory'
      - 'NAT rule': 'Forwarding'
```

To show raw, unlabeled results instead, use a string or list:

```yaml
parameters:
  filter_jsonpath: $.linux_root_filesystem.filesystem_size_bytes
  filter_grep: '^NAME='
```

A filter list also stays unlabeled. For example, `filter_jsonpath: [$.hostname, $.kernel]` emits the selected values as a YAML list, and a list of grep expressions emits the union of matching lines.

Labeled and unlabeled entries may be mixed in one filter list. Mixed lists preserve entry order and emit a YAML list: labeled entries appear as one-key mappings, and unlabeled entries appear as the raw filter result.

## CSV result imports

All import paths are relative to the root of the project archive or project directory. Each CSV must use exactly one identity column: `external_id` to match individual students, or `delivery_name` to match the assigned delivery filename. A `delivery_name` row applies its results to every student assigned that deliverable, which supports shared group submissions. Students or deliveries absent from a CSV keep their normal test/check/rubric/evaluation behavior. Test imports require `stdout` and `exit_code`, and accept `stderr`. Check imports require one `0` or `1` result column per check; rubric and evaluation imports use numeric mark columns. An optional `descriptions` parameter maps imported column names to descriptions. A CSV column with the same name as an already calculated check, rubric, evaluation item, or override test replaces that result for the students or delivery represented by the row.

```yaml
tests:
  - name: professor_notes
    test: import/csv
    parameters:
      filepath: extern/results/style.csv
      override_test: style_review
checks:
  - name: professor_check_results
    check: import/csv
    parameters:
      filepath: extern/results/checks.csv
      descriptions:
        style: Code style review
        attitude: Class participation
rubrics:
  - name: professor_rubric_marks
    calculation: import/csv
    parameters:
      filepath: extern/results/rubrics.csv
      descriptions:
        style: Code style rubric
evaluation:
  - name: professor_final_marks
    calculation: import/csv
    parameters:
      filepath: extern/results/evaluation.csv
      descriptions:
        RA1: Operating systems installation
```

To apply a row to every student assigned to one deliverable, use `delivery_name` as the CSV key instead. For example, a checks CSV can contain:

```csv
delivery_name,style,teamwork
team_alpha.ova,1,1
team_beta.ova,0,1
```

Each `delivery_name` must match the assigned filename exactly. A CSV cannot contain both identity columns, and keys must be unique within the file.

An undelivered student still gets an expedient. The `not_delivered_mark` project setting can be a number or `null` (default); it is assigned to every evaluation item for that student. Test, check, and rubric sections stay empty. The per-student expedient ID is built from `external_id` and `run_id`.

## Tests

| Test reference | Parameters | Behavior and output |
|---|---|---|
| `VirtualBox/import_ova` | `vm_name` (optional; otherwise a generated unique name), `vbox_group` (optional; default `/thot`), `vm_path` (optional; default `./thot_vms`, relative to the project location) | Imports the assigned OVA and captures VBoxManage output, including its machine-readable configuration. It reports the imported VM; grading its hardware or network configuration belongs in checks. Later VM operations require the import manifest. |
| `VirtualBox/start_vm` | `vm_name` (required; use the name from `import_ova`) | Starts the manifest-owned VM with the VirtualBox headless frontend. Captures stdout, stderr, and exit code. |
| `VirtualBox/stop_vm` | `vm_name` (required) | Sends an ACPI shutdown and waits for power-off, using forced power-off if needed. Reports the shutdown method and process output. |
| `VirtualBox/destroy_vm` | No parameters | Removes the VM using its run and delivery cleanup manifest. It refuses to remove a VM whose ownership cannot be verified. The engine also performs manifest-scoped cleanup after the configured test sequence. |
| `ssh/get_host_info` | `host` (optional; default `127.0.0.1`), `port` (optional; default `2222`), `user` (required), `pass` (required) | Connects to the guest over SSH and reports hostname, distribution, kernel, memory, and `df` filesystem data. It pins and checks the SSH host key in the delivery's report directory. |
| `network/nmap` | `host` (required); `ports` or `top_ports` (optional, mutually exclusive); `exclude-ports` (optional) | Runs Nmap with a TCP connect scan and service/version detection. By default it scans the top 1000 TCP ports. `ports` accepts a comma-separated list and ranges such as `22,80,8000-8100`; `top_ports` accepts 1–65535. `exclude-ports` is passed to Nmap's `--exclude-ports` and accepts Nmap port expressions. Captures the Nmap report in stdout. The `nmap` executable must be installed on the daemon's `PATH`. |
| `libguestfs/disk_info` | `disk_path` (required; disk image or directory), `disk_name` (optional; select a file when the directory contains multiple supported images) | Reports detected disks, partitions, filesystems, sizes, labels, UUIDs, and filesystem usage when available. Supported image formats include VDI, VMDK, QCOW2, VHD, VHDX, IMG, and raw. |
| `libguestfs/linux_rootfs_info` | `disk_path` (required), `disk_name` (optional) | Uses libguestfs inspection to identify a Linux root filesystem. Reports the detected OS, root filesystem and partition details, usage, users, and groups. If no Linux root filesystem is identified, it returns an error and writes a partition summary to stderr. |
| `libguestfs/linux_tinycore_info` | `disk_path` (required), `disk_name` (optional) | Searches filesystems for a Tiny Core gzip CPIO initramfs, validates Tiny Core markers, and reports detected version/architecture, users, groups, and boot-storage partition information. It inspects the base initramfs; additional boot-time overlays and persistent backup data are not merged. |
| `stats/regex_extract` | `tests` (earlier test name or comma-separated names), `property` (required), `regex` (required; must contain capture group 1), `data_type` (optional; default `number`) | Extracts one scalar per selected earlier test result and writes a YAML summary to stdout. A repeated test selector includes all its repetitions. |
| `aggr/select` | `tests` (required list of earlier test names); optional `property`, `condition`, `value` | Defaults to selecting rows whose `status == passed`. A repeated test name selects all repetitions. Exactly one row must match; its `stdout`, `stderr`, and `exit_code` become this test's process output. Zero or multiple matches fail with an explanation. Scalar comparison types are inferred from the candidate and configured value. |
| `aggr/join` | `tests` (required list); optional `filter_property`, `filter_condition`, `filter_value` | Concatenates selected results' stdout and stderr in configured test order. Repeated tests contribute each result in repetition order. Exit code is zero only when all included results have exit code zero. When filtering is configured, only matching results are joined; scalar comparison types are inferred. |
| `stats/yaml_extract` | `tests`, `property`, `JSONPath` (required), `data_type` (optional; default `number`) | Reads a JSON or YAML document from the selected test-result property, extracts exactly one scalar using JSONPath, and writes a YAML summary to stdout. Use `stdout` for test output. |
| `import/csv` | `filepath` (required project-relative CSV), `override_test` (optional configured test name) | Reads rows by exactly one key: `external_id` for a student or `delivery_name` for an assigned delivery filename. Requires `stdout` and `exit_code`, accepts optional `stderr`. An `exit_code` of 0 marks the imported result passed; other codes mark it failed. A delivery-keyed row is copied to every student assigned that delivery. When `override_test` is set, matching expedients get the captured process fields in that test's row. Students or deliveries missing from the CSV retain the actual override-test result. If the importer is earlier than the override test and covers the relevant student or delivery group, the engine skips that target once for the shared execution. |
| `extern/<path>` | Any project-specific scalar parameters | Resolves the path under the project `extern/` directory. `.py` files run with `python3`, `.sh` files with `sh`, and other files must be executable. Parameters are appended as `key=value` arguments. `THOT_ARTIFACT_DIR`, `VBOX_USER_HOME`, and `VBOX_MANAGE_PATH` are provided in the environment. stdout, stderr, exit code, timeout, and output limits are captured. These scripts run as trusted daemon child processes, not in a sandbox. |

The two `stats/...` tests compute `count`, `max`, `min`, and `freq` (mode) for supported scalar types. Numeric values also include `avg`, population `var`, population `std`, and percentiles `p1`, `p5`, `p10`, `p20`, `p30`, `p40`, `p50`, `p60`, `p70`, `p80`, `p90`, `p95`, and `p99`. Percentiles use linear interpolation between sorted samples. Values that cannot be extracted are counted in `ignored_count`; the test errors if no sample can be extracted. Supported `data_type` values are `number`, `text`, `date` (`YYYY-MM-DD`), and `datetime` (RFC 3339).

## Checks

Every check has a unique `name`, a configured test name in `test`, an implementation in `check`, and optionally `description`, `property`, and `parameters`. A check over a repeated test evaluates each repetition and passes only when every repetition passes. Results are stored in each student expedient's `checks` list with `name`, `description`, numeric `result`, nullable `err`, and `date_time`.

| Check reference | Required fields and parameters | Behavior |
|---|---|---|
| `pass` | `test`; no extra parameters | Returns 1 when the test passed, or when every repetition passed; otherwise returns 0. |
| `regex_match` | `property`, parameter `regex` | Applies the regex to the selected scalar result property. Returns 1 if it matches each result for the configured test, otherwise 0. It does not need a capture group, `data_type`, `condition`, or comparison `value`. |
| `regex_extract` | `property`; parameters `regex`, `data_type`, `condition`, `value` | Captures group 1 from the property and compares it with the configured constant for each result. A non-match returns 0. |
| `yaml_match` | `property`; parameter `JSONPath` | Parses the property as JSON or YAML (or uses a structured JSON value directly) and returns 1 when JSONPath selects one or more nodes for each result; returns 0 when any result has no matches. Selected values may be objects or scalars. |
| `yaml_extract` | `property`; parameters `JSONPath`, `data_type`, `condition`, `value` | Parses the property as JSON or YAML (or uses a structured JSON value directly), selects exactly one scalar, and compares it with the configured constant. |
| `stats/regex_extract` | `property`; parameters `regex`, `data_type`, `stat`, `condition` | Extracts group 1 from each unique delivery's named test result, calculates a run-wide statistic, then compares this delivery's extracted value with it. |
| `stats/yaml_extract` | `property`; parameters `JSONPath`, `data_type`, `stat`, `condition` | Extracts one scalar by JSONPath from each unique delivery's named test result, calculates a run-wide statistic, then compares this delivery's value with it. |
| `aggr/checks` | Parameters `checks`, `condition`, and `value` for numeric conditions | Combines named check results. Supports `all`, `any`, `one`, `none`, `majority`, and comparisons of the passing-check count with an integer value. Referenced checks must be configured; aggregate dependencies may be nested but cannot contain cycles. |
| `import/csv` | `filepath`; `descriptions` (optional map of CSV column to description) | Requires exactly one of `external_id` or `delivery_name`, plus one 0/1 column for each imported check. Each result column creates a check result; a column matching an existing configured check replaces that result. A delivery-keyed row is applied to every student assigned that filename. |

For `regex_extract`, `yaml_extract`, and the statistical checks, `data_type` is `number`, `text`, `date`, or `datetime`. The ordinary extract checks require a constant `value` for comparison operators; statistical checks instead require a `stat`. Valid comparison conditions are `<`, `<=`, `>`, `>=`, `==`, and `!=`; `contains` is available only for text. Predicate conditions are also supported: `r'<regex>'` passes when the candidate value matches the regular expression, and `json'<JSONPath>'` passes when the candidate JSON/YAML document has at least one JSONPath match. Predicate conditions do not need a comparison `value` or `data_type`. Quote these conditions in YAML, for example `condition: "r'^open$'"` or `condition: "json'$.users[?(@.username == \"tvadmin\")]'"`. The same condition forms are accepted by `aggr/select` and `aggr/join` filtering. Number comparisons require finite numbers. Dates use `YYYY-MM-DD`; datetimes use RFC 3339. `yaml_match` is a presence predicate and does not take comparison parameters.

Example check for at least one user named `bin` in a YAML document emitted on stdout:

```yaml
- name: bin_user_exists
  description: The guest includes a user named bin.
  check: yaml_match
  test: linux_info
  property: stdout
  parameters:
    - JSONPath: "$.users[?(@.username == 'bin')]"
```

Statistical checks support `max`, `min`, `freq`, `avg`, `var`, `std`, and `p1`, `p5`, `p10`, `p20`, `p30`, `p40`, `p50`, `p60`, `p70`, `p80`, `p90`, `p95`, `p99`. `freq` returns the most frequent value; ties select the smallest value. `avg`, `var`, `std`, and percentiles require numeric data. Samples are deduplicated by delivery so shared group submissions count once. A statistical check targeting a repeated test must name one expanded result, such as `start_time_01`. Statistical check rows additionally include `statistic`, `sample_count`, and `student_value`.

Example regex match check:

```yaml
- name: http_forward_defined
  description: A NAT rule maps host port 8080 to guest port 80.
  check: regex_match
  test: import_ova
  property: stdout
  parameters:
    - regex: 'Forwarding\(\d+\)="[^"]*,tcp,,8080,,80"'
```

An aggregate check does not use `test` or `property`. Its `checks` parameter is a list of check names, and `condition` selects how their 0/1 results are combined. `all`, `any`, `one`, `none`, and `majority` do not take `value`; `>=`, `<=`, `<`, `>`, `==`, and `!=` compare the number of passing checks with a non-negative integer `value`. For example:

```yaml
- name: linux_installed
  description: At least one Linux root filesystem detector succeeded.
  check: aggr/checks
  parameters:
    - checks: [system_info, tinycore_info]
    - condition: any
- name: mostly_correct
  check: aggr/checks
  parameters:
    - checks: [ssh, http, dns, ntp]
    - condition: majority
- name: minimum_services
  check: aggr/checks
  parameters:
    - checks: [ssh, http, dns, ntp]
    - condition: '>='
    - value: 3
```

`majority` means strictly more than half of the listed checks passed. A nested aggregate can refer to another aggregate check. Unknown check names, repeated names, and dependency cycles are rejected when loading the project configuration.

## Rubrics

Rubrics are configured in the root `rubrics` list. Each rubric has a unique `name`, optional `description`, an ordered list of check names (`checks` or the alias `checks_list`), a `calculation`, and optional `parameters`. A rubric aggregates the referenced check rows and stores its numeric result and scaled inputs in the expedient's top-level `rubrics` list. `rubrics_multiplier` (default `1`) scales each 0/1 check value before rubric calculation, so rubrics can be marked over 10, 100, or another positive finite scale. The check rows themselves remain 0/1.

| Calculation | Parameters | Definition |
|---|---|---|
| `avg` | None | Arithmetic mean of the check result values. |
| `max` / `min` | None | Largest or smallest check result value. |
| `median` | None | 50th percentile (`p50`). |
| `p1`, `p5`, `p10`, `p20`, `p30`, `p40`, `p50`, `p60`, `p70`, `p80`, `p90`, `p95`, `p99` | None | Percentile with linear interpolation between adjacent sorted check values. |
| `sum` | None | Sum of the check result values. |
| `weighted_avg` | `weights` | Weighted mean. `weights` must contain one finite, non-negative number per check, in the same order, and their sum must be positive. The weighted sum is divided by the sum of weights. |

Check results, including zero results that have an `err`, are the numeric inputs to rubrics. Rubrics are recalculated whenever checks are evaluated, including with `thot run checks [run-id]`. A rubric declaration with `calculation: import/csv` instead reads exactly one of `external_id` or `delivery_name` and one numeric column per rubric; optional `descriptions` maps column names to descriptions. Matching CSV columns replace configured rubric results for the matching students or all students assigned to the named delivery.

Evaluation items are configured under the root `evaluation` list. Each item has a unique `name`, optional `description`, an ordered `rubrics` list (or alias `rubrics_list`), a `calculation`, and optional `parameters`. They support the same aggregations and `weighted_avg` weights as rubrics, but use rubric results as inputs. The resulting final marks are saved under `evaluation.results`. `evaluation_multiplier` (default `1`) scales each calculated evaluation item. If both multipliers differ from `1`, run creation returns a warning and the expedient records it: rubric marks are scaled first, then evaluation marks are scaled again. Multipliers must be finite positive numbers.

```yaml
rubrics_multiplier: 10
evaluation_multiplier: 1
rubrics:
  - name: virtualization
    checks: [ova_imported, vm_started]
    calculation: avg
evaluation:
  - name: RA1
    description: Virtual machine deployment and operation
    rubrics: [virtualization, system_information]
    calculation: weighted_avg
    parameters:
      weights: [0.7, 0.3]
```

Here each check contributes 0 or 10 to its rubric, while the final `RA1` mark remains on the same scale because `evaluation_multiplier` is 1. An evaluation declaration with `calculation: import/csv` reads exactly one of `external_id` or `delivery_name` and one numeric column per evaluation item, with optional column descriptions; these imported marks replace same-named configured evaluation rows for the matching students or delivery group.

Every project student receives an expedient, even if no delivery is assigned. Such expedients keep tests, checks, and rubrics empty. Set `not_delivered_mark` in `configuration.yaml` to a number or `null` (default) to provide that value or leave evaluation results blank for each evaluation item. Each expedient ID combines the student's `external_id` and the run ID.

## Reports

`thot report test_stats [RUN_ID]`, `checks_stats`, and `rubric_stats` report per-item statistics across student expedients; without a run ID, they use the newest run. Test and check statistics include passed, failed, error, skipped, pending, and running counts where applicable, success rate, and average, median, p95, and maximum execution time. Rubric statistics include calculated/error counts, average and median marks, p10/p90, min/max, standard deviation, and calculation times. The reports flag tests with no successful expedients, all-pass results, high skip rates, or execution near/over 30 seconds or the configured timeout; they flag checks with no passes, all-pass outcomes, errors, or slow execution; and rubrics with calculation errors or identical marks.

`thot report expedients [RUN_ID]` prints four matrices: `expedient_x_test`, `expedient_x_check`, `expedient_x_rubric`, and `expedient_x_evaluation`. Test/check cells show `Ok`, `X`, `R`, or blank; rubric marks are rounded to three decimal places. Report endpoints return JSON, while the CLI supports terminal tables and `--csv`, `--yaml`, or `--json` output.

`thot report grades --students --pdf [RUN_ID]` creates an individual progress-report PDF for each student, including final evaluation marks, rubric and check details, and a test-results appendix. `thot report grades --professor --pdf [RUN_ID]` creates a PDF with evaluation-item, rubric, check, and test matrices across the run's students. Full names and descriptions are used in section labels; abbreviated EI/R/C codes appear only in matrix column headers when full names do not fit. Check outcomes are shown as `v` or `X`, and marks are rounded to three significant digits. `thot report grades --evaluation-items --csv [RUN_ID]` writes student name, external ID, email, and one column per final evaluation item to stdout. All grades reports default to the newest run; PDF output defaults to `grades/<run-id>/` and accepts `--output-dir DIR`.
