# Thot

Sistema de corrección y evaluación automatizada.

## Estructura

- `apps/thotd/`: ejecutable del servicio de motor.
- `apps/thot-cli/`: CLI `thot` para importar estudiantes desde CSV y consultar estudiantes.
- `crates/`: componentes Rust reutilizables del motor.
- `web/`: interfaz web, todavía no implementada.
- `runners/`: entornos de ejecución de pruebas, todavía no implementados.
- `deploy/`: configuración de despliegue, todavía no completada.
- `dev/`: herramientas y automatización de desarrollo.
- `docs/`: documentación del proyecto.

## Funcionalidad disponible

`thotd` importa registros de estudiantes enviados como JSON y permite consultar
los estudiantes guardados. La importación descarta registros inválidos o
duplicados de forma individual y devuelve el motivo de cada descarte.

El servicio escucha por defecto en `127.0.0.1:8765` y guarda los datos en
`./data/thot.sqlite3`. La CLI `thot` importa CSV y consulta estudiantes. La
interfaz web todavía no está implementada.

## Desarrollo

```bash
make build
make test
```

El ejecutable y las pruebas usan el workspace Cargo del repositorio. `target/`
es salida de compilación local y no forma parte del despliegue. La configuración
de desarrollo está documentada en `dev/`.
