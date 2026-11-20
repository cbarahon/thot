# ADR-0002: Laboratorio común por unidad de evaluación

Estado: **propuesto**, 2026-11-15. Ámbito: plataforma objetivo v2; no implica implementación ni aprobación definitiva.

## Contexto

Pruebas y agentes necesitan interactuar con servicios y VM creados por pasos anteriores sin mezclar trabajos de alumnos.

## Decisión propuesta

Compartir workspace, red y registro de recursos entre agentes de una unidad explícita; separar unidades mediante L1 distintas. Motor conserva el DAG y las notas.

## Alternativas

Alternativas: sandbox por paso pierde estado; una VM para toda la clase permite contaminación. Comparaciones conjuntas se declaran como unidad propia.

## Consecuencias

Permite escenarios Git/make/KVM/SSH. Root en L1 puede falsear observaciones del mismo laboratorio; el perfil nested declara esa limitación.

## Validación y revisión

P4 demuestra interacción entre agentes y ausencia de acceso entre unidades. Revisar este ADR si esas condiciones no se cumplen. Véanse [arquitectura](../architecture.md), [contratos](../api-spec.md) y [cuestiones abiertas](../PROJECT_PLATFORM_PROPOSAL_REVIEW.md).
