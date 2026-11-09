# `thot` command-line client

The CLI talks to `thotd`; it never invokes `VBoxManage` or sends guest
credentials. Its default API endpoint is `http://127.0.0.1:8081`.

```text
thot [-h HOST] [-p PORT] import students <file.csv>
thot [-h HOST] [-p PORT] get students [csv]
thot [-h HOST] [-p PORT] assignments guess [--json]
thot [-h HOST] [-p PORT] assignments get [--json]
thot [-h HOST] [-p PORT] assignments import <FILE.yaml|json> [--json]
thot [-h HOST] [-p PORT] run [list]
thot [-h HOST] [-p PORT] run create <PROJECT_PATH> [--deliveries DIR]
thot [-h HOST] [-p PORT] run create --project <PROJECT_PATH> [--deliveries DIR]
thot [-h HOST] [-p PORT] run status [RUN_ID]
thot [-h HOST] [-p PORT] run checks [RUN_ID]
thot [-h HOST] [-p PORT] run delete RUN_ID
thot [-h HOST] [-p PORT] run expedient EXPEDIENT_ID
thot [-h HOST] [-p PORT] run cancel RUN_ID
thot [-h HOST] [-p PORT] run cleanup EXPEDIENT_ID
thot [-h HOST] [-p PORT] report runs [--csv|--yaml|--json]
thot [-h HOST] [-p PORT] report deliveries [RUN_ID] [--csv|--yaml|--json]
thot [-h HOST] [-p PORT] report deliveries tests [RUN_ID] [--csv|--yaml|--json]
thot [-h HOST] [-p PORT] report deliveries checks [RUN_ID] [--csv|--yaml|--json]
thot [-h HOST] [-p PORT] report test_stats [RUN_ID] [--csv|--yaml|--json]
thot [-h HOST] [-p PORT] report checks_stats [RUN_ID] [--csv|--yaml|--json]
thot [-h HOST] [-p PORT] report rubric_stats [RUN_ID] [--csv|--yaml|--json]
thot [-h HOST] [-p PORT] report expedients [RUN_ID] [--csv|--yaml|--json]
thot [-h HOST] [-p PORT] clean
thot [-h HOST] [-p PORT] prune
thot [-h HOST] [-p PORT] reset
```

Bare `thot run` searches the current directory for exactly one `.thot.gz`,
`.thot.tgz`, `.thot.zip`, or root `configuration.yaml` project source and starts
it. A configuration file selects its containing directory. It reports an error
if no source or multiple sources are found. Use `thot run list` to list saved
runs. The `create` forms accept an archive path or project directory;
`--project` is an explicit spelling of the project path. Without
`--deliveries`, the engine uses `delivery_folder` from `configuration.yaml`;
relative paths resolve from the archive's containing directory or the project
directory, and absolute paths are accepted. A delivery override can point
anywhere the daemon can read. Assignment commands index the project's direct
`.ova` children before a run begins. Deliveries without assigned students are
skipped. Every imported project student gets an expedient, including students
without a delivery; those expedients have empty test, check, and rubric sections.
Set `not_delivered_mark` to a number to use it for every evaluation item on such
an expedient, or leave it `null` (the default) for a blank mark. If no student
has a delivery, the run still creates and finishes their expedients without
running tests.

`thot run checks [RUN_ID]` reevaluates configured checks against saved test
results, recalculates configured rubrics and evaluation items, then updates each
student's `checks`, `rubrics`, and `evaluation.results` sections. It also applies
configured CSV imports. Expedients without a delivery retain blank checks and
rubrics and receive `not_delivered_mark` for each evaluation item. Without an ID, it selects the most recently
created finished run. When run from the same unpacked project directory used
by that run, it reads that directory's current `configuration.yaml`; otherwise
it uses the run's saved configuration.
It does not start or repeat tests. Check results replace the previous `checks`
section, rubric results and final evaluation marks replace their previous sections.

`rubrics_multiplier` and `evaluation_multiplier` in `configuration.yaml` default
to 1 and must be positive finite numbers. The first scales check values before
rubric calculations; the second scales each final evaluation item. If both are
set to values other than 1, run creation returns a warning because the scales
compound.

Successful run and expedient responses are printed as human-readable YAML,
with stdout/stderr shown as literal blocks and test, check, rubric, and evaluation fields
ordered for scanning. Expedients show general run/delivery details, student
data, tests, checks, rubric results, then final evaluation marks. Visible `\n` and `\t` sequences in these output fields
are rendered as line breaks and tabs. The HTTP API continues to use JSON.
Student commands also print structured output as YAML. The explicit
`get students csv` form remains CSV.

`thot run status` without an ID requires exactly one queued, running, or
cancelling run; otherwise specify `RUN_ID`. Status output includes one
expedient per student, its assigned delivery, and lifecycle state (`pending`,
`running`, or `finished`). Cleanup uses the persisted manifest and checks
recorded VM identity and storage ownership.

`thot report runs` shows run status, start/finish time, duration, and test
progress/error totals. For each active run, it also shows a delivery table with
delivery status and timing plus pending, running, failed, completed, and total
test counts. `thot report deliveries [RUN_ID]` summarizes test and
check outcomes once per distinct delivery, including the number of students
sharing it. Without `RUN_ID`, delivery reports use the newest saved run.
`thot report deliveries tests` and `thot report deliveries checks` show a
pass/error column for each configured test or check. Reports use a terminal
table sized from the console width, with data-based column widths, concise
two-line headers, and long values truncated as needed. Select `--csv`,
`--yaml`, or `--json` to export the same report data. The JSON API routes are
`GET /api/v1/reports/runs` and
`GET /api/v1/reports/deliveries?run_id=...&view=summary|tests|checks`.

`thot report test_stats`, `checks_stats`, and `rubric_stats` show per-item
success/failure or score distributions, pending/running counts, average and
percentile execution times, and diagnostic signals for all expedients in the
selected run. These commands default to the newest run.
`thot report expedients [RUN_ID]` shows test, check, rubric, and final evaluation matrices; test/check cells
use `Ok`, `X`, `R`, or blank, and rubric marks are rounded to at most three
decimal places. Matrix and statistics reports accept the same export flags.

`thot clean` removes orphaned Thot artifacts and VMs only when ownership can be
verified. `thot prune` keeps the newest run per project and requires an
interactive `y` confirmation before deleting older run data. `thot reset`
requires typing a generated six-digit code; it clears Thot database and runtime
state while preserving installation configuration, project archives, and
original delivery files. Unverified external files are reported and preserved.

API settings resolve from command-line options, then the first available file
among `./thot-config.yaml`, `./.thot-config.yaml`, and
`~/.config/thot/thot-config.yaml`. Files accept scalar `host`/`server` and
`port` keys. Command-line options override the file.

CSV import recognizes common Catalan, Spanish, and English headers. Unknown
columns are ignored, malformed rows are reported on stderr, and validation of
student data remains in the server.

Student operations are scoped to one project. When the current directory has a
root `configuration.yaml`, its project is selected automatically. Otherwise the
CLI uses the most recently selected project, normally the one passed to
`thot run create`; commands fail if no project context exists. Student import
and retrieval responses identify their project. A project's root
`students.csv` is imported automatically on run creation. If it has no
recognized external-ID column or a row has a blank external ID, each student's
email is used as `external_id`. The `get students csv` command writes project
details to stderr and CSV rows to stdout.

Project CSV imports use project-relative `filepath` values. A test declared as
`test: import/csv` requires `stdout` and `exit_code` columns and may include
`stderr`; it can set `override_test` to replace that test's saved process result.
Each CSV uses exactly one key column: `external_id` applies a row to one
student, while `delivery_name` applies it to every student assigned that exact
deliverable filename. Students or deliveries without a CSV row keep their
actual test result. Checks use `check: import/csv`; rubrics and evaluation
items use `calculation: import/csv`. Their CSV files use the same key columns
plus one numeric result column per check, rubric, or evaluation item. Optional
`descriptions` maps column names to descriptions. A matching existing result
is replaced for the student or delivery group matched by that row.

`thot assignments guess` indexes the current project's delivery folder and
compares student names, surnames, email usernames,
external IDs, and student IDs with delivery filenames. It replaces current
assignments with its best non-ambiguous matches; one delivery may be matched to
several students. Inspect or edit the suggested mapping with
`thot assignments get`. Its response contains a row for each student, the
assigned delivery ID and filename, the number of students sharing that delivery,
and a list of unassigned deliveries.

`thot assignments import FILE` replaces all current assignments. The file may
be JSON or YAML and can be edited from `assignments get` output. Each entry is
resolved using `student_external_id`
and `delivery_filename`; other row fields from `assignments get` are ignored and
the student and delivery details are read from the database. Set
`delivery_filename: null` to leave that student unassigned while reviewing
mappings. The response includes `imported_records` and an `errors` list. Each
error identifies its input record when available, describes the problem, and
suggests a correction. Valid records are imported; invalid records are omitted
from the replacement mapping, so correct the reported rows and import again.
Students omitted from the file become unassigned and are skipped by the next
run. Deliveries omitted from all rows are also skipped. If no pairs remain, the
run completes without tests. For example:

```yaml
assignments:
  - student_external_id: student-001
    delivery_filename: student-001.ova
  - student_external_id: student-002
    delivery_filename: student-001.ova
  - student_external_id: student-003
    delivery_filename: null
```

When `assignments.yaml` or `assignments.json` is present at the project root,
run creation imports it automatically after indexing the project's students
and deliveries. If both files exist, run creation fails with an error explaining
that only one may be present. The project file's assignments replace the
current database assignments.

Guess, get, and import print YAML by default. Add `--json` to print their
responses as formatted JSON. The API endpoints are `POST /api/v1/assignments/guess`,
`GET /api/v1/assignments/get`, and `POST /api/v1/assignments/import`.
Assignment operations are scoped to the current project, and deliverable
candidates come from that project's configured delivery folder. A run executes
tests once for each distinct assigned delivery; when multiple students share a
delivery, the engine copies the same test results into each student's
expedient.
