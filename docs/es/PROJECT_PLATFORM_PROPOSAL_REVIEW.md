# Revisión de la propuesta de plataforma

Estado: revisión de ingeniería, 2026-11-15. La propuesta se concreta en [requisitos](requirements.md), [arquitectura](architecture.md), [modelo](data-model.md), [interfaces](api-spec.md), [plan](implementation-plan.md) y [ADR](decisions/README.md). Las decisiones están **propuestas**, pendientes del prototipo y aceptación; esta documentación no instala ni verifica una sandbox.

## Estado observado y objetivo

El README raíz infradescribía el POC al excluir asociaciones y notas; el catálogo y la guía del POC ya las documentan. Existe ejecución externa con permisos del daemon y estado SQLite/ficheros. El workspace también contiene trabajo previo de separación de VirtualBox en `thot-virtualbox-agent`, tareas HTTP JSON v1, snapshots y carpetas `runs/`; sus README describen SSH hacia un host VirtualBox y rutas de artefactos compartidas. Eso aporta contratos y separación de responsabilidades, pero no aísla la ejecución general de código del alumno. Parte de la documentación heredada conserva el modelo anterior: se mantiene como referencia del POC, con advertencia, hasta verificar una versión publicada concreta.

Se propone un motor genérico local y una VM KVM desechable por unidad de evaluación, con agentes, Docker y posibles VM anidadas en su interior. Todos los pasos de una unidad comparten estado; distintas unidades quedan aisladas. El proyecto continúa siendo la unidad de trabajo, archivo, edición e intercambio del profesor.

## Revisión de acuerdos anteriores

| Tema | Resolución propuesta y motivo |
|---|---|
| Nombre | Conservar `configuration.thot.yaml`; alias legado con aviso y rechazo de coexistencia |
| Alumnado | Conservar prioridad YAML sobre CSV con aviso y campo ID configurable; UUID interno independiente |
| N×N | Conservar grupos y varios entregables; hacer explícita unidad conjunta, rol, tipo y cardinalidad |
| Rutas | Conservar originales y nombres saneados con sufijos; referencias por ID estable, no por nombre saneado |
| Timestamp | Conservar fecha legible de segundos, pero creación exclusiva y tratamiento de colisión son obligatorios aunque sólo haya un profesor |
| Run modificable | Mantener edición y ampliación dentro de la misma carpeta; añadir pequeñas revisiones para explicar qué configuración originó cada resultado |
| Saltar resultados existentes | Sustituir la regla de «existe resultado» por compatibilidad de huella; evita atribuir observaciones antiguas a código o pruebas modificados |
| Estado compartido | Conservar resultados no conserva VM vivas; reconstruir dependencias para pasos nuevos que las necesiten |
| Tests dentro del motor | Sólo parsing/evaluación puros y limitados; toda acción sobre código, red, discos o procesos va al laboratorio |
| Agente host VirtualBox | Transición del POC, no frontera objetivo ni dependencia de la nueva plataforma Linux KVM |
| Docker | Dentro de L1 para herramientas/servicios; no Docker del host ni socket del profesor |
| Base de datos | Índice reconstruible como objetivo; no eliminar SQLite antes de migrar y probar todo el estado |

Las revisiones no convierten el proyecto en un repositorio inmutable ni obligan al profesor a gestionar versiones. Permiten editar valores con una explicación comprobable de su efecto. Si se exige sobrescritura sin ninguna procedencia, debe aceptarse explícitamente la pérdida de trazabilidad; no es la recomendación de ingeniería.

## Dificultades y decisiones abiertas

| ID / prioridad | Dificultad o decisión | Propuesta / alternativas | Evidencia de cierre y fase |
|---|---|---|---|
| O-01 Bloqueante | Nesting varía por CPU, firmware y proveedor de VM | KVM L1/L2 como perfil inicial; exigir equipo certificado o limitar actividad | Arrancar dos L2, Docker y SSH con cuotas y medir degradación; P0 |
| O-02 Bloqueante | Privilegios mínimos reales del supervisor/libvirt | Servicio dedicado con operaciones allowlist; elegir conexión/polkit/LSM concretos | Demostrar que usuario motor/guest no controla dominios ajenos ni inyecta dispositivos; P0/P2 |
| O-03 Bloqueante | Agentes pueden compartir confianza con root del alumno en L1 | Perfil ordinario separa contenedores; nested informa confianza reducida; posible observador en otra VM futura | Definir qué afirmaciones académicas se aceptan bajo root hostil y ensayar falsificación; P0/P4 |
| O-04 Bloqueante | Egreso Git/dependencias con TLS, DNS e IPv6 | Proxy fuera de L1, allowlist y tokens de lectura; offline tras adquisición | Ensayo SSRF, redirección, LFS/submódulos y ausencia de secretos; P2/P3 |
| O-05 Alta | Qué imágenes y herramientas mantener | Imagen por digest, manifiesto reproducible, actualizaciones y rollback | Elegir distribución/versiones, responsable y política de parches; P0/P2 |
| O-06 Alta | Resultados reutilizables frente a estado vivo | Huellas y reconstrucción; snapshots de VM excluidos inicialmente | Mostrar plan antes de ampliar y coste real de reconstrucción; P4/P5 |
| O-07 Alta | Semántica de errores y justicia de notas | Pendiente por infraestructura, cero sólo por regla académica explícita | Profesor valida informes con casos límite y ausencia de entrega; P5/P6 |
| O-08 Alta | Cuotas y rendimiento en equipos docentes | Una unidad activa inicial, admisión por reserva; concurrencia posterior medida | Medir corpus de actividades, fijar máximos y hardware mínimo publicado; P0/P4 |
| O-09 Alta | Integridad frente a edición manual | Correcciones YAML y revisiones; hashes detectan accidentes, sin atestación criptográfica | Probar modificar resultado/configuración y recuperar sin falsear procedencia; P1/P5 |
| O-10 Alta | Almacenamiento de entradas y grandes discos | Exportación completa por defecto; ligera explícita y no reproducible | Medir tamaños, retención y tiempo de copia; decidir deduplicación segura; P1/P6 |
| O-11 Alta | Compatibilidad de actividades VirtualBox/OVA | Legado separado; migración docente a KVM, sin promesa de conversión | Matriz por actividad y limitaciones de imágenes/arranque; P7 |
| O-12 Alta | Permisos de carpetas y fallos de persistencia | Usuario/ACL explícitos, un escritor local, diario y commit atómico | Ensayo disco lleno/corte de corriente/reindexación y exportación; P1/P2 |
| O-13 Alta | Arranque SSH fiable de L2 creada por alumno | Registro de recursos con pinning obtenido por canal de provisión fiable | Resolver actividades que no entregan claves o readiness; sin desactivar verificación global; P4 |
| O-14 Media | Interfaz accesible y YAML complejo | Plantillas guiadas y validación; CLI inicial, web posterior | Piloto decide si CLI basta para primera entrega o interfaz guiada es gate; P6 |
| O-15 Media | Informes Unicode y LMS | Fuentes incrustadas licenciadas y exportación de columnas configurable | Comparativa de bibliotecas y prueba de importación Moodle real; P6 |
| O-16 Futura | Servidor remoto | Mantener contratos, añadir mTLS, tenancy y almacenamiento | ADR y amenazas nuevos antes de abrir acceso remoto; posterior a P7 |

## Condiciones para continuar

No anunciar «seguro para ejecutar cualquier entrega» ni eliminar el despliegue existente por la sola redacción del diseño. P0 debe resolver O-01–03 y fijar el alcance verificable; P2 debe demostrar aislamiento exterior antes de probar código hostil. La validación final exige hardware Linux/KVM real y profesor piloto. Las pruebas unitarias y la compilación no sustituyen esas evidencias.
