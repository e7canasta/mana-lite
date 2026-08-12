# Sprint 1: Cadencia normal por modelo

**Estado:** completado
**Objetivo:** implementar gating temporal cooperativo sin workers ni urgencias
**Duracion:** una entrega tecnica, no una duracion de calendario

## Resultado esperado

Un blueprint puede declarar un intervalo minimo para cada regla. La percepcion
ejecuta el modelo solo cuando esta debido y mantiene la politica actual de
latest-wins si el ciclo tarda mas que la fuente.

## Alcance

- agregar `interval_min_ms` opcional a `CascadeRule`;
- default `0`, equivalente al comportamiento actual;
- mantener `last_started_at` por modelo en el scheduler;
- comprobar el intervalo antes de ejecutar roots y children;
- marcar una ejecucion cuando comienza el intento real;
- distinguir `not_due` de `skip` por falta de target;
- conservar el orden topologico y todos los gates actuales;
- agregar tests unitarios y de integracion minima;
- documentar el campo en un blueprint de ejemplo sin cambiar el perfil activo
  de produccion.

## Fuera de alcance

- solicitudes urgentes;
- `InferenceRequest` y validacion cruzada;
- workers por modelo;
- cache de masks o keypoints;
- ejecucion same-frame dinamica;
- cambios en `mana-control`;
- cambios en la semantica de slots.

## Tareas

### S1.1 Contrato de configuracion

- [x] Agregar `interval_min_ms` con `serde(default)`.
- [x] Validar el valor por tipo y documentar `0` como cada keyframe elegible.
- [x] Actualizar fixtures de `CascadeRule`.

### S1.2 Estado del scheduler

- [x] Agregar estado `last_started_at` por modelo.
- [x] Exponer `is_due(model, now)`.
- [x] Exponer `mark_started(model, now)`.
- [x] Evitar catch-up y doble ejecucion del mismo modelo en un ciclo.

### S1.3 Integracion de percepcion

- [x] Aplicar el gate temporal antes de `InferEngine::run`.
- [x] Mantener roots antes que children.
- [x] Mantener `primary_root_valid` coherente cuando el root no se ejecuta.
- [x] No mezclar resultados de frames distintos.

### S1.4 Instrumentacion

- [x] Contar modelos omitidos por intervalo.
- [x] Mantener separados `not_due`, `skip` y `gated`.
- [x] Mostrar el motivo en la linea por modelo cuando este habilitado.
- [x] Verificar que un modelo sin llamadas no emita latencias falsas.

### S1.5 Verificacion

- [x] Test de primer run inmediato.
- [x] Test de intervalo no vencido.
- [x] Test de intervalo vencido.
- [x] Test de atraso sin catch-up.
- [x] Test de gates despues del intervalo.
- [x] Test de compatibilidad con reglas sin intervalo.
- [x] Test de slot latest-wins ya existente permanece verde.
- [x] `cargo test --workspace --release`.
- [x] `git diff --check`.

## Criterios de aceptacion

1. Una regla sin `interval_min_ms` se comporta como hoy.
2. Una regla con `interval_min_ms = 2000` no inicia dos ejecuciones con menos
   de 2000 ms entre sus inicios.
3. Si el modelo tarda 3000 ms, el scheduler no ejecuta tres llamadas para
   recuperar el tiempo perdido.
4. Si llegan dos keyframes durante una ejecucion, el proximo ciclo consume el
   mas fresco disponible.
5. Un child no debido no aparece como `skip` por falta de target.
6. Un child debido sin target sigue apareciendo como skip de cascade.
7. El control no cambia su cadencia ni sus contratos.

## Riesgo principal

Un root primario omitido no puede producir una `SceneSample` valida nueva sin
una politica de cache de evidencia. Sprint 1 debe mantener el root primario en
intervalo `0` en los perfiles activos y dejar cualquier root lento para una
decision posterior con tests de frescura.

## Evidencia que debe entregarse

- diff completo de la etapa;
- salida literal de los tests;
- una corrida sintetica o de fixture que muestre `not_due`;
- una corrida con blueprint multi-modelo y ventana suficientemente larga;
- explicacion de cualquier cambio de golden o de frecuencia observada.
