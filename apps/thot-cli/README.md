# `thot` command-line client

The CLI talks to `thotd`; it never invokes `VBoxManage` or sends guest
credentials. Its default API endpoint is `http://127.0.0.1:8081`.

```text
thot [-h HOST] [-p PORT] import students <file.csv>
thot [-h HOST] [-p PORT] get students [csv]
thot [-h HOST] [-p PORT] run [list]
thot [-h HOST] [-p PORT] run create <PROJECT.thot.tgz|PROJECT.thot.zip> [--deliveries DIR]
thot [-h HOST] [-p PORT] run status RUN_ID
thot [-h HOST] [-p PORT] run delete RUN_ID
thot [-h HOST] [-p PORT] run expedient EXPEDIENT_ID
thot [-h HOST] [-p PORT] run cancel RUN_ID
thot [-h HOST] [-p PORT] run cleanup EXPEDIENT_ID
```

`thot run` and `thot run list` both list saved runs. To create a run, pass a
`.thot.tgz` or `.thot.zip` project archive; the CLI resolves the path to an
absolute path before sending it to the daemon. Without `--deliveries`, the
engine uses `delivery_folder` from the archive's root `configuration.yaml`;
relative paths resolve from the archive's containing directory and absolute
paths are accepted. A delivery override can point anywhere the daemon can read.
The engine processes direct `.ova` children and does not map results to
students.

Successful run and expedient responses are printed as human-readable YAML,
including multiline `stdout` and `stderr` fields. The HTTP API continues to use
JSON. Student commands retain their existing output formats: `get students`
prints a table, `get students csv` prints CSV, and `import students` prints its
JSON response.

Currently, `run status` requires an explicit run ID. Project archives must be
passed explicitly; automatic project discovery and loading a project directory
are not wired into the run flow. Cleanup uses the persisted manifest and checks
recorded VM identity and storage ownership.

API settings resolve from command-line options, then the first available file
among `./thot-config.yaml`, `./.thot-config.yaml`, and
`~/.config/thot/thot-config.yaml`. Files accept scalar `host`/`server` and
`port` keys. Command-line options override the file.

CSV import recognizes common Catalan, Spanish, and English headers. Unknown
columns are ignored, malformed rows are reported on stderr, and validation of
student data remains in the server.
