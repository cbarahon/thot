# Thot

Automated assessment engine and CLI. The current POC imports VirtualBox OVAs,
collects technical observations and evidence, and creates delivery expedients
before student mapping or grading.

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

The POC uses a native Linux host with a dedicated unprivileged service account,
VirtualBox profile, and VM storage root. `thotd` runs on the host at
`127.0.0.1:8081`; guest HTTP/SSH forwards use loopback ports 8080 and 2222.
The CLI can list runs, create sequential runs with readable timestamp IDs,
inspect run status and expedients, request manifest-checked cleanup, and delete
inactive run records from the database. Student association and grading are
outside this POC.

Read [`docs/THOT_VM_ACTIVITY_POC.md`](docs/THOT_VM_ACTIVITY_POC.md) for the
activity contract and [`deploy/README.md`](deploy/README.md) for production
artifact installation, host configuration, and manual acceptance steps. The
static web UI is not required for this POC.

## Desarrollo

```bash
make build
```

El ejecutable y las pruebas usan el workspace Cargo del repositorio. `target/`
es salida de compilación local y no forma parte del despliegue. La configuración
de desarrollo está documentada en `dev/`.
