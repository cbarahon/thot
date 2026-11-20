# ADR-0003: Carpeta portable como fuente de verdad

Estado: **propuesto**, 2026-11-15. Ámbito: plataforma objetivo v2; no implica implementación ni aprobación definitiva.

## Contexto

El profesor necesita mover, archivar, inspeccionar y corregir una evaluación sin depender de una instalación de base de datos.

## Decisión propuesta

YAML y evidencia nativa en carpeta con snapshots, manifiestos y referencias relativas; SQLite sólo índice reconstruible tras migración. Mantener CSV/PDF/HTML para intercambio.

## Alternativas

Alternativas: BD central como autoridad o sólo exportaciones finales. Ambas dificultan portabilidad completa o recalificación.

## Consecuencias

Requiere escritor exclusivo, commit recuperable, inventario y cuotas; no se soporta edición activa desde sistemas sin locking fiable. Paquetes contienen datos personales.

## Validación y revisión

P1/P7 deben demostrar traslado, recuperación, reconstrucción e importación sin pérdida. Revisar este ADR si esas condiciones no se cumplen. Véanse [arquitectura](../architecture.md), [contratos](../api-spec.md) y [cuestiones abiertas](../PROJECT_PLATFORM_PROPOSAL_REVIEW.md).
