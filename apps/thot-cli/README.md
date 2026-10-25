# `thot` command-line client

The CLI talks to `thotd`; it never invokes `VBoxManage` or sends guest
credentials. Its default API endpoint is `http://127.0.0.1:8081`.

```text
thot [-h HOST] [-p PORT] import students <file.csv>
thot [-h HOST] [-p PORT] get students [csv]
thot [-h HOST] [-p PORT] run create --project ID [--deliveries DIR]
thot [-h HOST] [-p PORT] run status RUN_ID
thot [-h HOST] [-p PORT] run expedient EXPEDIENT_ID
thot [-h HOST] [-p PORT] run cancel RUN_ID
thot [-h HOST] [-p PORT] run cleanup EXPEDIENT_ID
```

The run command reads the project archive `<ID>.thot.tgz` or
`<ID>.thot.zip`. Without `--deliveries`, the engine uses `delivery_folder` from
the archive's root `configuration.yaml`; an override must be under the server's
configured delivery root. The engine processes direct `.ova` children and does
not map results to students. Status and expedient commands return JSON.
Cleanup uses the persisted manifest and checks recorded VM identity and storage
ownership.

API settings resolve from command-line options, then the first available file
among `./thot-config.yaml`, `./.thot-config.yaml`, and
`~/.config/thot/thot-config.yaml`. Files accept scalar `host`/`server` and
`port` keys. Command-line options override the file.

CSV import recognizes common Catalan, Spanish, and English headers. Unknown
columns are ignored, malformed rows are reported on stderr, and validation of
student data remains in the server.
