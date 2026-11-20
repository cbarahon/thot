# Plan de implementación

Estado: planificación propuesta, sin cambios de código en esta entrega. Cada fase requiere aceptación antes de habilitar la siguiente; no se fija calendario sin medir el prototipo de nesting. Dependencias: P0 → P1 → P2 → P3 → P4 → P5 → P6 → P7. P1 y la investigación de informes de P6 pueden avanzar mientras se certifica hardware; ninguna integración hostil se habilita antes de P2.

| Fase | Trabajo ordenado | Dependencia / salida comprobable |
|---|---|---|
| P0 — Viabilidad y amenazas | Inventariar cambios actuales y pruebas reales; prototipo aislado KVM L1 → Docker y L2; medir CPU/RAM/arranque; ensayar permisos libvirt, vsock, red y watchdog | Informe reproducible en host dedicado con CPU, distribución, kernel, QEMU/libvirt y Docker; decisión go/no-go de nesting. Si falla, recortar alcance o exigir otro hardware, nunca ejecutar en host |
| P1 — Contratos y persistencia | Publicar esquemas formales y fixtures válidos/inválidos de YAML, tareas, resultados y paquetes; fijar canonicalización; repositorio de carpeta, lock, diario y commit de revisión; reindexación sin SQLite | RF-01–05, 11–14; round-trip exportar/importar, mover carpeta, recuperar tras corte y rechazar colisión/escritura concurrente |
| P2 — Proveedor seguro | Imagen base reproducible y digest, supervisor mínimo, identidad Unix/vsock, cuotas externas, red denegada, leases y limpieza manifestada; instalador y diagnóstico | RF-09/10/17; matriz de aislamiento verificada en Linux real; ninguna ruta de fallback nativa |
| P3 — Agentes y resultados | Coordinador, protocolo con ACK y blobs, filesystem/exec/Git; proxy Git sin secretos en guest; captura limitada de streams, timeouts/cancelación; estado incierto e idempotencia | RF-05/07/11; desconectar durante envío y ejecución sin duplicar tarea; artefacto inválido no sale de staging; repositorio privado no filtra token |
| P4 — Laboratorio interactivo | Registro de recursos, DAG, bloqueos, Docker local L1, KVM anidado, discovery y SSH con clave fijada; perfiles ordinario/nested | RF-06–08; ejecutar el escenario Git → make → dos L2 → propiedades → SSH; dos unidades simultáneas sin puertos ni datos compartidos |
| P5 — Evaluación y edición | Adaptar checks/rúbricas actuales a resultados v2, barrera estadística, expedientes N×N, ausencia de entrega, revisiones, correcciones y ampliación con reconstrucción de estado | RF-12–15; mismas notas sobre fixtures del POC cuando las semánticas son compatibles; fallo técnico deja pendiente; corrección no pierde valor calculado |
| P6 — Experiencia docente | Plantillas, validación explicativa, resumen y HTML offline, PDF Unicode, CSV configurable, importación/exportación privada, backup/retención y guía de instalación | RF-16/18/20; piloto con profesor sin ayuda de comandos de hipervisor; nombres con acentos y símbolos correctos en PDF y copiar/buscar |
| P7 — Migración y publicación | Importador de SQLite/archivos/archives POC, verificación de evidencia y notas, comparación de informes, rollback y paquetes de instalación; documentar retirada gradual del legado | RF-19; restaurar backup y reabrir proyecto migrado sin BD original; aprobación del piloto y todos los gates de seguridad |

## Matriz mínima de validación

Las futuras pruebas unitarias cubrirán validación, selectores/cardinalidad, ciclos, cálculo de huellas, evaluación y revisiones. Las de contrato compartirán fixtures entre motor y agentes y comprobarán mensajes desconocidos, hash incorrecto, payload repetido/distinto, replay tardío y truncamiento. Las integraciones usarán Linux/KVM real y repositorios sintéticos sin datos personales.

La aceptación de seguridad debe intentar: leer/borrar un fichero señuelo del host; acceder a Docker/libvirt del host, LAN, metadatos y otro laboratorio por IPv4/IPv6; escapar mediante enlaces/traversal; agotar RAM, procesos, disco e inodos; enviar logs/blobs excesivos y JSON/YAML malicioso; comprometer un contenedor y comprobar que sigue dentro de L1. La adquisición debe probar redirecciones/DNS hacia destinos bloqueados y Git malicioso. Comprobar firewall desde el guest aunque tenga root, no sólo revisar configuración.

Inyectar caída del motor/supervisor/agente, reinicio del host, disco del proyecto lleno y cancelación durante creación de L2. Verificar que no se publica nota final incompleta, no se repite automáticamente una operación incierta, queda evidencia parcial y limpieza pendiente visible. Probar recuperación en el punto anterior y posterior a cada commit de revisión.

La prueba docente usa una actividad correcta, otra académicamente incorrecta y otra con fallo técnico: el profesor debe distinguirlas, consultar evidencia, corregir una nota y exportar. Comprobar entrega grupal, varios recursos por alumno, entrega tardía y prueba nueva que requiere VM ya destruida.

## Migración sin pérdida

1. Congelar y respaldar DB, proyectos y artefactos del POC; registrar versión concreta, sin atribuir a una compilación una validación de VM.
2. Importar a carpeta nueva, preservar IDs originales como metadatos y mapear rutas. Nunca modificar/borrar origen automáticamente.
3. Convertir configuración/alumnado/asociaciones y resultados mediante mapeo explícito. Un estado `passed` legado conserva su semántica; no se inventan observaciones ni garantías de aislamiento.
4. Comparar alumnado, asociaciones, conteos, hashes, notas y enlaces. Marcar artefactos ausentes y entradas no reproducibles.
5. Ofrecer preview de v2 y corregir incompatibilidades. Una OVA o script VBox no se convierte automáticamente a KVM; requerirá nueva imagen/actividad o seguirá en el despliegue legado explícito.
6. Cambiar el proyecto activo sólo tras validación. Rollback vuelve al backup/instalación anterior; no ejecuta un downgrade destructivo.

## Trabajo posterior y elementos absorbidos

Este plan sustituye `ROADMAP.md` y `TODO.md`: concentra las tareas para evitar prioridades contradictorias. La investigación pendiente de fuentes PDF debe comparar bibliotecas Rust, incrustación Unicode, licencia, tamaño, búsqueda/copia y coste de integración antes de sustituir el generador.

Después de P7, y sólo por demanda: interfaz web guiada; sincronización Moodle y evaluación de LTI 1.3; autenticación/roles/auditoría multiusuario; proveedor remoto con mTLS, scheduler distribuido, almacenamiento de objetos y cuotas por organización; otros hipervisores; paquetes de tests revisados/firmados y catálogo; análisis entre cohortes con privacidad. Hosting, marketplace y ayuda de IA son exploratorios, no compromisos. IA requeriría opt-in y revisión del profesor, sin calificación automática oculta.
