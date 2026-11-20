# Documentación de Thot

## Estado actual

El [catálogo implementado](IMPLEMENTED_CATALOG.md), la [guía del POC](THOT_VM_ACTIVITY_POC.md) y la [instalación](../deploy/README.md) describen el POC VirtualBox, sus pruebas, calificación e informes. El workspace contiene además una transición al [agente VirtualBox HTTP v1](../apps/thot-virtualbox-agent/README.md) y [carpetas de ejecución](../apps/thotd/README.md); algunos textos del POC aún reflejan el despliegue anterior. No debe confundirse esta separación con aislamiento del código externo.

## Plataforma propuesta v2

Todos los documentos siguientes son diseño, no funcionalidad publicada:

| Documento | Contenido |
|---|---|
| [Requisitos](requirements.md) | Requisitos funcionales/no funcionales y criterios de aceptación |
| [Arquitectura](architecture.md) | Modelo genérico, KVM + Docker, seguridad, operación y requisitos Linux |
| [Modelo de datos](data-model.md) | Proyecto portable, identidades, resultados, expedientes y edición |
| [Contratos e interfaces](api-spec.md) | YAML de workflow, tareas/resultados, protocolo, API y artefactos |
| [Plan de implementación](implementation-plan.md) | Fases, dependencias, pruebas, migración y evolución posterior |
| [Decisiones arquitectónicas](decisions/README.md) | ADR con alternativas y consecuencias |
| [Revisión de propuesta](PROJECT_PLATFORM_PROPOSAL_REVIEW.md) | Dificultades, riesgos y decisiones pendientes |

`ROADMAP.md` y `TODO.md` se han sustituido por el plan de implementación, que conserva el trabajo de informes Unicode y las opciones de evolución. Se mantienen las guías del POC porque siguen siendo necesarias para operar/migrar la implementación existente.
