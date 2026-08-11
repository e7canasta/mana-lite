# 01 — Ingesta pura

## Hipótesis

La cadena RTSP → demux → dedupe de keyframes → decode → JSONL entrega frames a
cadencia estable y sostenida, sin reconexiones ni degradación, con todas las
capas superiores apagadas.

## Qué está activo

|Capa|Estado|
|---|---|
|Ingesta RTSP + decode|**activa**|
|Inferencia|apagada (`pipeline.infer = false`)|
|Tracking|apagado|
|Zonas / FSM|apagados (catálogos omitidos)|
|Presencia|apagada (`presence.enabled = false`)|
|Rerun|apagado (se homologa en 02)|

## Cómo correr

```sh
cargo run --release -- --config workshop/scenarios/01-ingest-only/mana.toml
```

Dejarlo correr al menos 60 s: la falla que este escenario expone no aparece en
los primeros 5 s.

## Criterios de aceptación

Sobre las líneas `mana_lite::pipeline`, una vez pasada la primera ventana:

1. **Cadencia sostenida** — `keyframes processed` por ventana debe igualar a
   `seen`, ventana tras ventana. Un `processed` que cae a 0 mientras `seen` se
   mantiene es falla, no inactividad.
2. **Cadencia acorde al GOP** — con GOP de 1 s, `gap p50 ≈ 1000ms`. Una p95 muy
   por encima de la p50 indica pérdida de keyframes en transporte.
3. **Sin reconexiones** — cero líneas `rtsp reconnect attempt` en toda la
   corrida.
4. **Decode acotado** — `decode avg` estable (~12-18 ms para 1080p). Creciente
   indica presión de memoria.
5. **Sin stale ni blind** — el JSONL no debe contener eventos `stale` ni
   `blind`:

   ```sh
   grep -c '"event":"stale"\|"event":"blind"' workshop/runs/01-ingest-only/*.jsonl
   ```

## Modos de falla conocidos

### El drenaje de keyframes no era cancelación-seguro — CORREGIDO 2026-08-11

**Síntoma.** Todas las ventanas informan `0 keyframes processed (N seen)` con
`decode 0ms avg`, y el JSONL acumula `stale` y después `blind`. El transporte
está sano: `seen` sigue contando al ritmo del GOP y los `timeouts` de retina son
normales. Ningún contador de descarte lo explica — ni `dup:`, ni `kf_dropped:`.

**Causa.** `poll_freshest_keyframe` acumulaba el keyframe más fresco en una
variable local del future:

```rust
let mut latest: Option<Frame> = None;
loop {
    match self.reader.next_frame().await {
        Some(frame) => { self.keyframes_seen += 1; latest = Some(frame); }
        None => break,
    }
}
```

Ese future se poletea dentro de un `tokio::select!` (`src/app/mod.rs:75`) que
compite contra `scan_interval.tick()`. `select!` **descarta la rama perdedora**,
y con ella el `latest` acumulado — después de haber incrementado
`keyframes_seen` sobre el engine. El keyframe se contaba como visto y se perdía
sin dejar rastro en ningún contador de descarte.

El bucle sólo termina cuando el reader reporta agotamiento, es decir cuando pasa
`poll_timeout_ms` sin frames. Con `poll_timeout_ms = 50` y un stream a ~25 fps,
ese silencio casi nunca ocurre antes del tick de scan de 200 ms: el drenaje era
cancelado sistemáticamente. La condición se agrava justo después de un IDR,
cuando el encoder emite su ráfaga de p-frames y el hueco de 50 ms es aún menos
probable — o sea, la pérdida estaba sesgada precisamente contra los keyframes.

**Evidencia.** Con `poll_timeout_ms = 5`, la ingesta sostenía 1.0 Hz y
`processed == seen`; con 50, caía a 0 procesados de forma permanente. Ese
contraste aisló la cancelación como causa, descartando el dedupe por digest
(`dup` valía 0 durante toda la corrida, con `ingest_dup = true` en
`config/metrics.toml`).

**Corrección.** El keyframe drenado pasó a vivir en el engine (`staged`), no en
la pila del future, y `pframes_dropped` se acumula en el momento en vez de al
final del bucle. Cancelar el future ya no pierde trabajo: el keyframe queda
staged y lo emite la llamada siguiente. Fijado por
`ingest::tests::staged_keyframe_survives_cancellation`.

### El dedupe por digest no tiene ventana de vencimiento (latente)

No es lo que se observó arriba, pero sigue en pie: `poll_freshest_keyframe`
descarta un keyframe cuyo digest coincide con el anterior procesado
(`src/ingest.rs`), y esa comparación no vence. Ante una escena completamente
inmóvil, un encoder que emita IDR byte-idénticos suprimiría indefinidamente, y
**la propia supresión dispararía `data_stale`** — aguas arriba no habría forma
de distinguir "la escena no cambió" de "el stream murió". Verificable subiendo
`dup:` en la línea de métricas mientras `processed` cae a 0.

### El modelo se carga aunque la inferencia esté apagada

`pipeline.infer = false` corta la ejecución en `src/app/mod.rs:182`, pero no la
carga: el bootstrap construye igual la sesión ONNX y el log muestra
`model detect-fast: loaded (detect)`. Un escenario "sin inferencia" paga de
todos modos el arranque y la memoria del modelo. No invalida la medición de
ingesta, pero conviene saberlo antes de interpretar tiempos de arranque o uso de
VRAM.
