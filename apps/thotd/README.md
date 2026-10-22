# thotd — Thot engine (primera funcionalidad)

Implementación inicial en Rust de la API de estudiantes. El workspace Cargo mantiene
separados el ejecutable, la API HTTP, los casos de uso y los modelos.

## Arquitectura

- `thot-core`: contratos JSON y modelos.
- `thot-engine`: casos de uso y persistencia SQLite.
- `thot-api`: rutas HTTP y traducción de errores a respuestas HTTP.
- `apps/thotd`: configuración del proceso y arranque del servidor.

## Requisitos

Rust stable (edition 2024) y Cargo.

## Ejecución local

```bash
cargo run -p thotd
```

Por defecto escucha en `127.0.0.1:8765` y guarda los datos en `./data/thot.sqlite3`.
Las variables de entorno disponibles son:

- `THOTD_HOST` (por defecto `127.0.0.1`)
- `THOTD_PORT` (por defecto `8765`)
- `THOTD_DATABASE` (por defecto `./data/thot.sqlite3`)
- `RUST_LOG` (por defecto `info`)

Para permitir conexiones desde otros contenedores, configure `THOTD_HOST=0.0.0.0`.

El despliegue con Docker todavía no está configurado.

## API

### `GET /health`

Comprueba que el proceso responde.

### `POST /import/students`

```json
{
  "students": [
    {
      "source_row": 2,
      "name": "Anna",
      "surname": "Garcia",
      "email": "anna@example.org",
      "external_id": "A001",
      "group": "ASIX2",
      "center": "Institut Exemple",
      "birth_date": null
    }
  ]
}
```

`source_row` es opcional y permite conservar la fila original del CSV. La CLI
`thot` convierte CSV y resuelve sus cabeceras; el motor recibe registros ya
normalizados y aplica la validación de negocio.

Respuesta:

```json
{
  "imported": 1,
  "discarded": 0,
  "rejected": []
}
```

Cada registro inválido o duplicado se rechaza de forma independiente. El correo se
considera único sin distinguir mayúsculas y minúsculas. No se actualizan registros
existentes durante una importación.

### `GET /get/students`

Devuelve `{ "students": [...], "total": N }`, ordenados por identificador interno.

## Pruebas

```bash
cargo test --workspace
```

## Ejemplo con curl

```bash
curl -s http://127.0.0.1:8765/health
curl -s http://127.0.0.1:8765/get/students
curl -s -X POST http://127.0.0.1:8765/import/students \
  -H 'content-type: application/json' \
  -d '{"students":[{"source_row":2,"name":"Anna","email":"anna@example.org"}]}'
```

## Alcance deliberado de esta versión

No incluye todavía la web ni funcionalidades de evaluación. La API acepta JSON
normalizado; la CLI `thot` realiza la conversión CSV. El servicio no depende de
Docker.
