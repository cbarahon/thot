# Requisitos de la plataforma Thot

Estado: propuesta de arquitectura, 2026-11-15. Ninguna capacidad nueva descrita aquí está implementada por este documento. Los contratos objetivo se denominan v2 para distinguirlos del POC; su primera revisión es `thot.dev/v2`. Véanse [arquitectura](architecture.md), [modelo](data-model.md), [interfaces](api-spec.md) y [plan](implementation-plan.md).

## Propósito y actores

El profesor prepara una actividad, incorpora entregas, ejecuta una evaluación y entiende o corrige sus resultados sin administrar hipervisores. Un administrador instala y mantiene Linux, las imágenes y las políticas; puede ser la misma persona, pero la operación cotidiana no debe exigir esos conocimientos. El alumno aporta código, repositorios, discos o documentos que se consideran no fiables. Los agentes son programas deterministas de ejecución y observación; no se presupone ningún modelo de IA.

Se conserva la metodología entrega → pruebas/evidencias → checks → rúbricas → evaluación → expediente/informes. El motor decide la calificación. Una acción puede producir una observación válida aunque encuentre un defecto académico; una avería de plataforma nunca debe convertirse silenciosamente en un cero.

## Requisitos funcionales

| ID | Requisito verificable | Criterio de aceptación |
|---|---|---|
| RF-01 | Proyecto portable con configuración YAML, alumnado, asociaciones, entregas y resultados | Copiar una exportación completa a otra instalación permite abrir informes y recalificar sin la base de datos original ni conexión a Internet |
| RF-02 | Configuración declarativa versionada, plantillas comentadas y validación previa | Los errores indican fichero, línea/campo, causa y corrección; ninguna entrada inválida inicia código |
| RF-03 | Importar alumnado CSV/YAML con campo identificador configurable | Detectar identificadores vacíos/duplicados; YAML tiene prioridad con aviso cuando coexiste CSV |
| RF-04 | Asociaciones N×N y entregas con varios recursos tipados | Grupos y entregas de OVA + PDF funcionan sin duplicar ejecución; selector con cardinalidad errónea se rechaza antes de ejecutar |
| RF-05 | Adquirir repositorios Git HTTPS y ficheros locales | Registrar commit resuelto, hashes y copia portable; submódulos y LFS requieren habilitación explícita y cuotas |
| RF-06 | Laboratorio común para todos los pasos de una unidad de evaluación | Un paso crea servicios/VM y otro agente los inspecciona por identificadores estables en el mismo laboratorio |
| RF-07 | Acciones de Git, ficheros, procesos, Docker, KVM y SSH | Cubrir el escenario completo de [interfaces](api-spec.md); capturar stdout, stderr, código, señal, tiempos y observaciones |
| RF-08 | Dependencias declarativas y limpieza incondicional | Rechazar ciclos; bloquear dependientes según política; limpiar también al cancelar, agotar tiempo o perder agente |
| RF-09 | Aislar entregas del host y entre sí | Código del alumno nunca se ejecuta en el motor/host; las pruebas de aislamiento del plan son condición de publicación |
| RF-10 | Límites efectivos por paso, laboratorio y ejecución | Un bucle, fork bomb o disco lleno se detiene sin agotar la reserva del host y deja un diagnóstico |
| RF-11 | Resultados normalizados y evidencias verificables | Un agente no puede escribir expedientes ni notas; motor valida esquema, tamaño, identidad y hashes |
| RF-12 | Recalificar sin volver a ejecutar | Cambiar checks/rúbricas produce una revisión vinculada a resultados conservados |
| RF-13 | Ampliar una ejecución | Incorporar entregas/pruebas reutiliza sólo resultados compatibles; pasos nuevos que necesitan estado perdido reconstruyen dependencias |
| RF-14 | Intervención manual comprensible | Editar YAML de correcciones con autor/motivo y validar produce una revisión; el informe distingue nota calculada y corregida |
| RF-15 | Expedientes para todo el alumnado | Mostrar ausencia de entrega separada de error técnico; grupos comparten evidencia y conservan notas individuales |
| RF-16 | Informes y exportación | HTML local, PDF Unicode y CSV configurable para plataforma docente, con explicación prueba → nota y sin secretos |
| RF-17 | Recuperación, cancelación y limpieza | Tras reiniciar no se repite automáticamente una acción con efectos inciertos; residuos quedan identificados y reclamables |
| RF-18 | Interfaz sencilla | Validar, ejecutar, revisar, corregir y exportar son operaciones únicas; resumen en español con enlace a detalle técnico |
| RF-19 | Importación del POC | Transformar archivos/SQLite sin borrar origen, conservar evidencias y advertir incompatibilidades de VirtualBox |
| RF-20 | Importación/exportación con privacidad | Modos completo y compartible; el segundo omite alumnado, notas, secretos y entregas por defecto |

## Requisitos no funcionales

| ID | Requisito y objetivo inicial |
|---|---|
| RNF-01 Seguridad | VM KVM obligatoria como frontera exterior en la primera versión; fallo de preflight bloquea, sin degradación a proceso nativo |
| RNF-02 Consistencia | Un solo escritor por proyecto, escrituras atómicas y diario recuperable; ningún resultado parcial se presenta como final |
| RNF-03 Portabilidad | Referencias relativas bajo el proyecto y formatos abiertos UTF-8; secretos, sockets y discos temporales fuera de la exportación |
| RNF-04 Trazabilidad | Versiones, digests, entradas, configuración, revisiones, correcciones y evidencia referenciadas; hashes detectan cambios, no certifican autenticidad |
| RNF-05 Disponibilidad | Lease inicial de 30 s, heartbeat de 10 s, cancelación con gracia de 10 s y apagado forzado antes de 30 s; comprobar estos objetivos bajo carga |
| RNF-06 Rendimiento | Admisión por RAM/CPU/disco reservados; cero sobreasignación de RAM de laboratorios; medir arranque y tiempo por entrega antes de fijar un SLA |
| RNF-07 Mantenibilidad | Motor independiente de KVM, Docker, SSH y LMS mediante contratos versionados y pruebas de conformidad |
| RNF-08 Usabilidad | Un profesor piloto completa una plantilla y el ciclo de evaluación sin editar XML de libvirt, redes ni unidades systemd |
| RNF-09 Privacidad | Permisos privados por defecto, credenciales fuera del proyecto, borrado/retención configurables y exportaciones explícitas |
| RNF-10 Compatibilidad | Negociación de versión/capacidades; rechazar versión mayor desconocida, claves desconocidas y funciones no disponibles antes de ejecutar |
| RNF-11 Operación sin red | Tras descargar imágenes/dependencias y adquirir repositorios, ejecución y consulta offline con red denegada por defecto |

## Alcance y exclusiones

Primera plataforma: Linux x86_64, KVM/QEMU y libvirt para VM exterior; Docker dentro de ella. Virtualización anidada para actividades que crean VM KVM. Sin soporte inicial para macOS, Windows, Kubernetes, ejecución distribuida, múltiples profesores concurrentes ni ejecución directa de código del alumno en Docker del host. VirtualBox se conserva en el POC durante la migración, sin prometer compatibilidad automática.

La protección busca contener código hostil y errores accidentales; no existe garantía absoluta frente a vulnerabilidades del kernel o hipervisor. La instalación debe mantenerse actualizada y las actividades que requieren privilegios adicionales deben superar aceptación específica. No se promete que evidencias generadas en una máquina controlada por código hostil sean incorruptibles.
