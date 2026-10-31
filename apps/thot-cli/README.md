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
`.ova` children before a run begins. Students without deliveries and deliveries
without assigned students are skipped. If there are no assigned pairs, the run
completes without creating student expedients or running tests.

`thot run checks [RUN_ID]` evaluates only configured checks against saved test
results and writes each student's updated `checks` section to their expedient.
Without an ID, it selects the most recently created finished run. When run from
the same unpacked project directory used by that run, it reads that directory's
current `configuration.yaml`; otherwise it uses the run's saved configuration.
It does not start or repeat tests. Check results replace the previous `checks`
section.

Successful run and expedient responses are printed as human-readable YAML,
including multiline `stdout` and `stderr` fields. The HTTP API continues to use
JSON. Student commands also print structured output as YAML. The explicit
`get students csv` form remains CSV.

`thot run status` without an ID requires exactly one queued, running, or
cancelling run; otherwise specify `RUN_ID`. Status output includes one
expedient per student, its assigned delivery, and lifecycle state (`pending`,
`running`, or `finished`). Cleanup uses the persisted manifest and checks
recorded VM identity and storage ownership.

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
