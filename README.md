# Thot

Automated assessment engine and CLI. The current POC imports VirtualBox OVAs,
collects technical observations and evidence, and creates delivery expedients
with student mapping, checks, rubrics and grade reports.

## Estructura

- `apps/thotd/`: ejecutable del servicio de motor.
- `apps/thot-cli/`: CLI `thot` for student CSV operations and VM activity runs.
- `crates/`: reusable domain, API, engine, and runner components.
- `web/`: interfaz web, todavía no implementada.
- `runners/`: future runner integrations.
- `deploy/`: native VirtualBox POC installation guide and deployment assets.
- `dev/`: herramientas y automatización de desarrollo.
- `docs/`: documentación del proyecto.

## VM activity POC

The description below is the legacy native-host deployment. The workspace also
contains a separate [VirtualBox HTTP v1 agent](apps/thot-virtualbox-agent/README.md)
and [folder-backed run changes](apps/thotd/README.md); consult those guides for
that configuration. Neither deployment provides the proposed general sandbox.

The POC uses a native Linux host with a dedicated unprivileged service account,
VirtualBox profile, and VM storage root. `thotd` runs on the host at
`127.0.0.1:8081`; guest HTTP/SSH forwards use loopback ports 8080 and 2222.
The CLI can list runs, create sequential runs with readable timestamp IDs,
inspect run status and expedients, request manifest-checked cleanup, and delete
inactive run records from the database. Student association and grading are included in the POC.

Read [`docs/THOT_VM_ACTIVITY_POC.md`](docs/THOT_VM_ACTIVITY_POC.md) for the
activity contract and [`deploy/README.md`](deploy/README.md) for production
artifact installation, host configuration, and manual acceptance steps. The
`make deploy` target publishes a versioned GitHub release; Linux servers pull
the binaries with the release installer and never receive an inbound GitHub
connection. The static web UI is not required for this POC.

## Proposed platform

The [documentation guide](docs/README.md) separates the existing VirtualBox POC
and in-workspace HTTP v1 agent from the proposed Linux KVM + Docker sandbox
architecture. The proposal includes requirements, data and API contracts,
implementation phases and architectural decisions; it is not implemented yet.

## Desarrollo

```bash
make build
```

El ejecutable y las pruebas usan el workspace Cargo del repositorio. `target/`
es salida de compilación local y no forma parte del despliegue. La configuración
de desarrollo está documentada en `dev/`.
