# Contratos e interfaces

Estado: especificación de diseño v2. Los endpoints y el YAML siguientes son propuestos, no comandos actualmente disponibles. La API `/api/v1` y el protocolo HTTP v1 de `thot-virtualbox-agent` existentes no cambian por redactar este documento. Se mantendrán separados durante migración.

## Reglas comunes

JSON UTF-8 para mensajes, YAML seguro sin tags ejecutables para documentos. `schema_version: thot.dev/v2` en configuración, `protocol_version: 2` en mensajes. Rechazar versión mayor desconocida; campos nuevos opcionales requieren capacidad anunciada. Claves YAML duplicadas/desconocidas, alias expansivos, números no finitos y rutas de escape son errores. Límites iniciales: mensaje JSON 1 MiB, documento YAML 4 MiB, profundidad 32, 1000 pasos por workflow, chunk de blob 256 KiB (antes de codificación base64); valores revisables mediante versión/política. Logs y blobs nunca se embeben sin límite en JSON.

ID UUID o identificador opaco generado, fechas RFC 3339 UTC, duración en milisegundos enteros, cantidades con unidad explícita MiB/GiB. SHA-256 sobre bytes de blobs; huellas de objetos sobre JSON canónico RFC 8785. No se canonicaliza YAML directamente. Diagnóstico común: `code`, `message` comprensible, `field`, `retryable`, `technical_detail` opcional redactado.

## Configuración declarativa de ejemplo

Ejemplo completo del flujo solicitado a nivel de contrato: los digest y la clave referenciada son valores que proporciona la plantilla/instalación. El tipo `image_ref` debe resolver a un digest instalado antes de ejecutar. Los comandos son listas argv, sin interpolación de shell. `${...}` es referencia tipada y sólo se permite en campos que la declaran; no evalúa expresiones ni sustituciones de comandos.

```yaml
schema_version: thot.dev/v2
project_id: 2d4d004f-4c0f-46fb-b69a-77591364d814
name: Laboratorio de redes
roster:
  identity_field: email
submissions_file: submissions.yaml
assignments_file: assignments.yaml
sandbox:
  profile: nested-lab
  image_ref: linux-lab-2026.11
  cpu: 4
  memory_mib: 12288
  disk_gib: 40
  deadline_seconds: 1800
  network_policy: acquire-then-offline
workflow:
  - id: checkout
    action: git.checkout@2
    inputs: {role: source, kind: git, cardinality: 1}
    with: {revision: submitted, destination: repo}
    timeout_seconds: 120
  - id: files
    action: fs.inspect@2
    requires: [checkout]
    with:
      root: '${steps.checkout.outputs.tree}'
      required_paths: [Makefile, scripts/verify.sh]
    timeout_seconds: 10
  - id: code
    action: exec.run@2
    requires: [files]
    with:
      cwd: '${steps.checkout.outputs.tree}'
      argv: [python3, /instructor/extern/check_code.py]
    timeout_seconds: 60
  - id: create_lab
    action: exec.run@2
    requires: [code]
    with:
      cwd: '${steps.checkout.outputs.tree}'
      argv: [make, lab-up]
    timeout_seconds: 600
  - id: discover
    action: kvm.discover@2
    requires: [create_lab]
    with: {scope: laboratory, names: [router, client]}
    timeout_seconds: 30
  - id: properties
    action: kvm.inspect@2
    requires: [discover]
    with: {resource: '${steps.discover.outputs.resources.router}'}
    timeout_seconds: 30
  - id: ready
    action: ssh.wait@2
    requires: [properties]
    with:
      resource: '${steps.discover.outputs.resources.router}'
      credential_ref: lab-router
    timeout_seconds: 90
  - id: verify
    action: ssh.exec@2
    requires: [ready]
    with:
      resource: '${steps.discover.outputs.resources.router}'
      credential_ref: lab-router
      argv: [sh, /opt/activity/verify.sh]
    timeout_seconds: 60
  - id: stop
    action: exec.run@2
    after: [verify]
    always: true
    with:
      cwd: '${steps.checkout.outputs.tree}'
      argv: [make, lab-down]
    timeout_seconds: 60
checks:
  - id: code_valid
    source: code.exit_code
    predicate: {equals: 0}
  - id: router_memory
    source: properties.observations.memory_mib
    predicate: {greater_or_equal: 1024}
  - id: functional
    source: verify.exit_code
    predicate: {equals: 0}
rubrics:
  - id: lab
    checks: [code_valid, router_memory, functional]
    calculation: weighted_avg
    weights: [0.2, 0.2, 0.6]
evaluation:
  - id: final
    rubrics: [lab]
    calculation: avg
    multiplier: 10
```

`submissions.yaml` declara para cada entrega `submission_id`, `inputs: [{input_id, role: source, kind: git, url: https://github.com/example/student-work.git, revision: <ref>}]`; el motor fija commit y hashes tras adquirir. Los identificadores y refs reales se introducen por importación, no escribiendo una plantilla por alumno. La imagen proporciona Python, make, herramientas KVM y agentes; el perfil conecta `make lab-up` con libvirt **de L1**. `kvm.discover` inventaría dominios de ese laboratorio, requiere nombres únicos y produce referencias verificadas; no acepta una dirección host arbitraria. `ssh.wait` resuelve endpoint privado y clave de host mediante el registro de recursos; si la imagen no permite establecer una clave fiable falla el preflight de la actividad. El script remoto debe haberse instalado por la actividad; SSH no supone que el host lo copie mágicamente.

Cada acción define esquema de entrada, salida, capacidades, efectos, cardinalidad y resultado de éxito operativo. Para `exec.run`/`ssh.exec`, `requires` considera éxito sólo `completed` y exit code 0; códigos aceptados adicionales se declaran con `accepted_exit_codes`. `fs.inspect` devuelve lista de encontrados/ausentes y su éxito exige los requeridos presentes. Un check puede inspeccionar una acción completada con exit no aceptado. `after` espera cualquier estado terminal. `always` omite el requisito de éxito, pero exige entradas disponibles; si no existen, se salta con motivo. La destrucción exterior ocurre incluso si `stop` no puede ejecutarse.

El agente SSH convierte argv a una orden remota mediante escape POSIX verificado por argumento, sin concatenar valores sin escapar; sólo admite guest Linux con shell compatible. Scripts complejos se distribuyen como archivos.

No hay bucles ni scripts embebidos en el DSL inicial; repeticiones acotadas, dependencias explícitas y scripts docentes en `extern/` resuelven casos avanzados. El motor valida referencias y tipos antes de ejecutar. Límites efectivos son el mínimo de política del administrador, run y paso. Valores iniciales del perfil ordinario: 60 s/paso si no se declara, 1800 s/laboratorio, 10 MiB por stream, 100 MiB de artefactos/paso, 1000 ficheros/paso y 256 procesos/contenedor. El perfil nested requiere cuotas específicas para sus servicios; siguen contenidos en RAM/CPU/disco/tiempo de L1.

## Puertos internos del motor

| Interfaz | Operaciones y responsabilidad |
|---|---|
| ProjectRepository | `validate`, `snapshot`, `lock`, `import`, `export`; sin ejecutar contenido |
| ResultStore | `append_attempt`, `commit_revision`, `read`, `verify_integrity`; único escritor de resultados finales |
| SandboxProvider | `capabilities`, `preflight`, `create`, `status`, `renew`, `stop`, `destroy`; nunca evalúa |
| AgentDispatcher | `submit`, `status`, `cancel`, `events`, `upload_blob`; lleva tareas a un laboratorio autorizado |
| Evaluator | resultados confirmados + reglas + correcciones → checks/rúbricas/notas; sin I/O de alumno ni red |

## Motor, supervisor y laboratorio

Motor → supervisor por socket Unix con permisos y verificación de UID del peer; supervisor → coordinador por virtio-vsock, asociado al CID, laboratorio y nonce de arranque. Handshake negocia protocolo/capacidades y token efímero de sesión, transmitido por canal de arranque dedicado sin guardarlo en disco portable. El host sólo acepta la conexión del CID asignado. Esta autenticación atribuye una sesión; no atesta integridad de una L1 comprometida. No se expone el supervisor al alumno ni a la red.

Mensajes JSON con prefijo de longitud de 32 bits big endian, máximo 1 MiB, `request_id`, `type`, `protocol_version`, `laboratory_id`, `lease_epoch` y `payload`. Capabilities anuncia acciones/versiones/esquemas, digests, nesting efectivo y cuotas. Preflight resuelve recursos y rechaza faltantes antes de arrancar entregas. Mensajes: `hello`, `capabilities`, `task.submit`, `task.status`, `task.cancel`, `event`, `lease.renew`, `blob.begin/chunk/commit`, `laboratory.stop/destroy`. El enlace del supervisor y el del coordinador usan allowlists distintas; el coordinador no recibe autoridad para crear recursos L0.

Payload de tarea (en producción todos los IDs opacos se generan):

```json
{
  "task_id": "task-opaque", "attempt_id": "attempt-opaque",
  "run_id": "run-opaque", "revision_id": "revision-opaque", "unit_id": "unit-opaque",
  "action": "exec.run@2",
  "inputs": [{"blob_id": "blob-opaque", "sha256": "<64 hex>", "target": "repo"}],
  "parameters": {"cwd": "repo", "argv": ["make", "lab-up"]},
  "limits": {"timeout_seconds": 600, "stdout_bytes": 10485760},
  "resource_refs": [], "retry_policy": "never"
}
```

No se envían rutas del host ni credenciales en este payload. Las rutas son relativas al workspace o referencias de recursos verificadas. Identidad de tarea persistida antes del envío. Mismo `task_id` y hash devuelve el estado existente; payload distinto es conflicto. Timeout de transporte no implica que el proceso no arrancara: consultar, no reenviar con un ID nuevo. Cada reinicio/lease nuevo invalida epoch anterior; resultado tardío queda como diagnóstico y nunca sustituye el confirmado. En L1 nueva se genera otro intento y se reconstruye el estado, no se afirma exactamente una ejecución.

Heartbeat cada 10 s, lease 30 s, eventos con secuencia creciente y ACK tras persistir; reconexión solicita secuencia siguiente y deduplica. Resultados/blobs pendientes se conservan hasta ACK o límite de almacenamiento. Expiración detiene L1 mediante watchdog exterior. Cancelar es idempotente, concede 10 s y fuerza parada antes de 30 s; si no puede matar toda la tarea, destruye L1. No se reintenta automáticamente un `make` o SSH con efectos desconocidos. Sólo acciones declaradas idempotentes pueden reintentarse de forma acotada (dos intentos iniciales como máximo).

## Resultado normalizado

```yaml
protocol_version: 2
task_id: task-opaque
attempt_id: attempt-opaque
status: completed
process:
  exit_code: 2
  signal: null
started_at: '2026-11-15T10:00:00Z'
finished_at: '2026-11-15T10:00:03Z'
duration_ms: 3000
stdout: {blob_id: stdout-opaque, bytes: 120, truncated: false, sha256: '<64 hex>'}
stderr: {blob_id: stderr-opaque, bytes: 80, truncated: false, sha256: '<64 hex>'}
observations: {}
resources: []
artifacts: []
diagnostics: []
```

`platform_error` añade diagnóstico como `IMAGE_BOOT_FAILED`, `AGENT_LOST`, `DISK_QUOTA`, `SSH_AUTH_FAILED`, `PROTOCOL_INVALID` o `CLEANUP_FAILED`; el motor clasifica si requiere revisión. Timeout de un comando académico puede originar check no superado si la regla lo declara expresamente; un timeout de arranque de plataforma deja nota pendiente. Resultados inválidos se registran como error de protocolo, nunca como éxito. Los streams pueden estar vacíos; los campos de proceso son null para acciones sin proceso.

Los recursos anunciados llevan `resource_id`, tipo, propietario laboratorio/tarea, atributos de esquema y endpoint privado, sin rutas host. El supervisor/registro valida pertenencia. Los artefactos se suben por chunks con offset y hash, reserva de cuota y cierre con hash total. El motor copia a staging sin seguir enlaces, valida contenido/tamaño y confirma atómicamente; no acepta rutas de escritura elegidas por un agente. Los chunks viajan como mensajes JSON base64 y respetan el límite de frame de 1 MiB; un blob grande requiere varios frames. Sólo ACK final permite enlazarlos desde `result.yaml`. Los streams parciales permanecen accesibles con marca de incompletos.

## API de usuario local propuesta

API HTTP JSON `/api/v2` sobre socket Unix por defecto; un adaptador loopback exige token local, permisos de fichero y protección de origen/CSRF si incorpora navegador. Ninguna ruta admite shell ni rutas arbitrarias fuera de proyectos registrados. Registrar una carpeta requiere acceso del usuario local autenticado; futura autorización multiusuario queda fuera de alcance.

| Método y ruta | Petición principal | Respuesta |
|---|---|---|
| POST `/projects` | directorio autorizado o importación validada | 201, ID y advertencias |
| POST `/projects/{id}/validate` | configuración actual | 200, errores/capacidades/estimación; sin efectos de alumno |
| POST `/projects/{id}/runs` | selección unidades, política | 202, run y operación |
| GET `/runs/{id}` | — | 200, progreso, notas pendientes y limpieza |
| POST `/runs/{id}/cancel` | motivo | 202, estado de cancelación |
| POST `/runs/{id}/extend` | cambios seleccionados, revisión base | 202, nueva revisión y plan de reutilización |
| POST `/runs/{id}/regrade` | reglas, revisión base | 202, revisión sin ejecutar acciones |
| POST `/runs/{id}/overrides` | correcciones, revisión base | 201, revisión validada |
| GET `/runs/{id}/events?after=N` | cursor | SSE con ID secuencial y replay; polling equivalente |
| GET `/runs/{id}/expedients/{student_id}` | revisión opcional | 200, expediente y procedencia |
| GET `/artifacts/{id}` | ID autorizado | bytes, tipo seguro y hash; descarga por defecto |
| POST `/projects/{id}/exports` | completo/compartible/ligero y revisión | 202, paquete e inventario |
| POST `/runs/{id}/cleanup` | recurso pendiente | 202, limpieza de recursos propios |

Mutaciones requieren `Idempotency-Key`, persistida con hash y respuesta durante la vida del run; `extend`, `regrade` y correcciones requieren revisión base/`If-Match`. Conflictos de versión/bloqueo responden 409/412. Otras respuestas: 400 JSON inválido, 401/403 autenticación/ámbito, 404 ID no visible, 413 límite, 422 validación/capacidad, 503 admisión no disponible. Error: `request_id`, `code`, `message`, `errors[]` con campo y solución. Operaciones asíncronas incluyen `operation_id` y URI consultable; progreso no equivale a confirmación de resultado. SSE nunca transporta secretos ni streams binarios.

Importar/exportar es un contrato de archivos: rutas relativas, inventario SHA-256, tamaños y versión. Rechazar enlaces, traversal, dispositivos, archivos excesivos y bombas de descompresión. Importar no ejecuta ni instala imágenes automáticamente. Exportación excluye credenciales y datos temporales; informes completos contienen datos personales y requieren protección.
