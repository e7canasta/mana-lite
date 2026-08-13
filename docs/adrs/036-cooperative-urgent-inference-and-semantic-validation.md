# ADR-036: Urgencias Cooperativas y Validacion Semantica

**Status:** Accepted — implementado
**Date:** 2026-08-13

## Contexto

La cascada necesita ejecutar un modelo fuera de su intervalo normal cuando una
fuente produce evidencia incierta. Hacer una llamada directa desde un modelo o
bloquear el control romperia la propiedad cooperativa del runtime.

Ademas, los keypoints, mascaras y payloads de modelos son evidencia interna de
percepcion. El control solo necesita un resultado semantico estrecho, fechado y
con calidad.

## Decision

Las solicitudes urgentes pasan por `CascadeScheduler` y respetan el mismo orden
topologico y los mismos gates que una ejecucion normal:

- la request tiene `model_key`, `reason`, `priority`, `requested_at` y
  `expires_at`;
- salta una sola vez `interval_min_ms`, pero no salta parent, clase, cantidad,
  confianza, region, tracking, crop ni orden topologico;
- prioridad mayor gana; en empate gana la request mas antigua;
- el TTL maximo es cinco segundos;
- la vista de requests se congela al inicio del keyframe;
- se admite como maximo una urgencia por keyframe;
- se consume al marcar el inicio, antes del backend;
- una inferencia en curso nunca se interrumpe;
- las requests transitorias viven en una cola durable, no en un slot
  `latest-wins`;
- una request persistente se deriva de `ControlDirective` y se reemplaza cuando
  cambia la directiva.

La validacion face/pose se produce en percepcion y cruza al control solo como:

```rust
FacePoseValidation {
    valid: bool,
    quality: f32,
    frame_number: u64,
}
```

`None` significa que no hubo validacion; `Some(valid = false)` significa que la
validacion se ejecuto y rechazo la evidencia. El timestamp y la edad vienen del
wrapper de evidencia del `SceneSample`. Keypoints, mascaras y geometrias
internas no cruzan el puerto de control.

## Consecuencias

- Una fuente puede pedir pose sin crear workers ni acoplarse al control.
- La urgencia no produce catch-up ni starvation ilimitado.
- El control mantiene su cadencia fija y recibe solo semantica auditable.
- La ejecucion same-frame dinamica queda fuera hasta que una medicion demuestre
  que la latencia de un keyframe adicional es inaceptable.
- La validacion de hardware y la politica clinica de consumo siguen siendo
  responsabilidades del despliegue, no del scheduler.
