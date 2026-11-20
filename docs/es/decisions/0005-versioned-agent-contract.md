# ADR-0005: Contrato de agente versionado y acotado

Estado: **propuesto**, 2026-11-15. Ámbito: plataforma objetivo v2; no implica implementación ni aprobación definitiva.

## Contexto

Separar pruebas del motor requiere controlar identidad, reenvíos, pérdida de conexión y salida no fiable. El HTTP v1 VirtualBox actual usa rutas compartidas y no basta para la frontera nueva.

## Decisión propuesta

Protocolo v2 con tareas tipadas, identidades, epoch/lease, eventos ACK, blobs por hash y resultados normalizados; Unix/vsock local. Agentes sin acceso al almacén de notas.

## Alternativas

Alternativas: comandos de shell remotos o reutilizar v1 sin cambios. No proporcionan las garantías requeridas de ámbito, transferencia y recuperación.

## Consecuencias

Hay que desarrollar conformidad y adaptador de transición; entrega de mensajes puede repetirse. No se promete exactamente una ejecución de efectos externos ni atestación de L1.

## Validación y revisión

P3 verifica pérdida/duplicación/cancelación y límites. Red remota requiere otro ADR con mTLS y autorización. Revisar este ADR si esas condiciones no se cumplen. Véanse [arquitectura](../architecture.md), [contratos](../api-spec.md) y [cuestiones abiertas](../PROJECT_PLATFORM_PROPOSAL_REVIEW.md).
