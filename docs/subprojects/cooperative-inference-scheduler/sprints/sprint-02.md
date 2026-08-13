# Sprint 2: Instrumento de capacidad

**Estado:** implementacion completada; corrida de hardware pendiente
**Objetivo:** distinguir costo real, politica temporal y falta de target

## Alcance

- gaps entre inicios reales por modelo: minimo, p50, p95 y maximo;
- atraso del inicio respecto de `next_due` para intervalos positivos;
- razones explicitas `not_due`, `due_but_gated` y `due_but_no_target`;
- contadores reservados para `urgent` y `urgent_expired`;
- ventana de reporte de 60 segundos para modelos a `0.5 Hz`;
- escenario `detect + face + pose + seg` con overlays `s/m` en `192` y `320`.

## Decisiones

Los gaps se miden cuando comienza el intento de inferencia, antes de llamar al
backend. Un resultado fallido sigue consumiendo una oportunidad de capacidad.

El atraso se calcula como:

```text
max(0, inicio_actual - (inicio_anterior + interval_min_ms))
```

El primer inicio no tiene gap ni `next_due`, por lo que no agrega muestras de
esas distribuciones.

`skip` se conserva como contador histórico del gate de cascada. En paralelo,
`due_but_no_target` hace explícito que el modelo estaba debido pero no encontró
un target. `gated` sigue contando modelos que el estado no solicitó, y
`due_but_gated` es su subconjunto que sí estaba debido.

Las urgencias todavía no tienen productor en runtime. Sus contadores salen en
cero hasta Sprint 3, pero ya tienen contrato de reporte para no cambiar el
formato cuando se agregue la cola cooperativa.

## Criterios de salida

- Los tests prueban gap, p50/p95/max y atraso contra `next_due`.
- El JSONL publica la configuración temporal, las distribuciones y las razones.
- El log textual publica la misma información por modelo.
- El workshop tiene una corrida larga reproducible de 60 segundos.
- Ningún blueprint de producción cambia su intervalo por este sprint.

## Corrida pendiente

La corrida formal requiere pesos ONNX, fuente RTSP y el host del workshop. Se
ejecuta desde la raiz:

```sh
MANA_SOURCE_URL=rtsp://192.168.1.6:8554/clip1 \
  cargo run --release -- --config workshop/scenarios/11-inference-capacity/mana.toml
```

Para el perfil `s/192`:

```sh
MANA_SOURCE_URL=rtsp://192.168.1.6:8554/clip1 \
MANA_BLUEPRINT_FILE=workshop/scenarios/11-inference-capacity/blueprint-s-192.toml \
  cargo run --release -- --config workshop/scenarios/11-inference-capacity/mana.toml
```
