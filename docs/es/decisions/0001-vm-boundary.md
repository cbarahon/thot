# ADR-0001: VM KVM como frontera exterior

Estado: **propuesto**, 2026-11-15. Ámbito: plataforma objetivo v2; no implica implementación ni aprobación definitiva.

## Contexto

Ejecutar entregas puede lanzar procesos, Docker y nuevos hipervisores. Un contenedor del host comparte su kernel y no satisface el objetivo de contención elegido.

## Decisión propuesta

Crear una L1 KVM desechable por unidad y usar Docker/KVM anidado exclusivamente dentro. Prohibir acceso al filesystem, dispositivos y sockets del profesor.

## Alternativas
requirements.md: requisitos funcionales y no funcionales.



architecture.md: arquitectura propuesta y justificación.



data-model.md: modelo de datos.



api-spec.md: contratos e interfaces.



implementation-plan.md: tareas ordenadas y dependencias.



decisions/: decisiones arquitectónicas (ADR).
Alternativas: procesos nativos, Docker sobre host y host dedicado por actividad. Las dos primeras reducen aislamiento; la tercera aumenta coste operativo.

## Consecuencias

Mayor consumo, arranque y dependencia de nesting; hace falta certificación Linux real. No elimina escapes del hipervisor.

## Validación y revisión

P0/P2 deben validar nesting, permisos, red, cuotas y destrucción forzada. Revisar este ADR si esas condiciones no se cumplen. Véanse [arquitectura](../architecture.md), [contratos](../api-spec.md) y [cuestiones abiertas](../PROJECT_PLATFORM_PROPOSAL_REVIEW.md).
