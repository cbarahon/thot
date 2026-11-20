# ADR-0004: Edición y ampliación con procedencia ligera

Estado: **propuesto**, 2026-11-15. Ámbito: plataforma objetivo v2; no implica implementación ni aprobación definitiva.

## Contexto

Se desea ajustar valores o incorporar entregas/pruebas sin repetir todo, conservando explicaciones fiables de los resultados.

## Decisión propuesta

Run modificable con revisiones pequeñas; edición de entradas/correcciones validada. Reutilizar por huella de entradas, paso, dependencias, imágenes y política. Reconstruir estado vivo cuando se perdió L1.

## Alternativas

Alternativas: sobrescribir sin historial o inmutabilidad completa con run nuevo para cada cambio. La primera pierde atribución y la segunda dificulta el uso cotidiano.

## Consecuencias

Introduce selección de resultados por revisión y reporte de correcciones. No garantiza ahorrar ejecución si la prueba nueva necesita máquinas anteriores.

## Validación y revisión

P5 debe demostrar cambios de rúbrica, código, entrega tardía, corrección individual y dependencia viva. Revisar este ADR si esas condiciones no se cumplen. Véanse [arquitectura](../architecture.md), [contratos](../api-spec.md) y [cuestiones abiertas](../PROJECT_PLATFORM_PROPOSAL_REVIEW.md).
