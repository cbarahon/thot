# ADR-0006: Separar observación de calificación

Estado: **propuesto**, 2026-11-15. Ámbito: plataforma objetivo v2; no implica implementación ni aprobación definitiva.

## Contexto

Un fallo de plataforma, un programa con exit code no cero y un criterio académico incumplido tienen significados diferentes.

## Decisión propuesta

Agentes devuelven hechos y diagnósticos; motor calcula checks, rúbricas y notas desde evidencias persistidas. Error de infraestructura deja pendiente salvo intervención explícita.

## Alternativas

Alternativas: agente devuelve nota final o todo error equivale a cero. Dificultan explicar y recalificar con justicia.

## Consecuencias

Requiere estados operativos/académicos separados, reglas explícitas de timeout y correcciones con motivo; conserva evidencia cruda.

## Validación y revisión

P5/P6 comparan fallos técnicos y académicos y recalifican sin ejecutar código. Revisar este ADR si esas condiciones no se cumplen. Véanse [arquitectura](../architecture.md), [contratos](../api-spec.md) y [cuestiones abiertas](../PROJECT_PLATFORM_PROPOSAL_REVIEW.md).
