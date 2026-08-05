## ☕ **Mana Lite - Onboarding de Ingeniería**

*Café en mano, marcador en la otra. Vamos a la pizarra.*

---

### 🎯 **¿Qué problema resuelve Mana Lite?**

Mana Lite es un **pipeline de percepción clínica en un solo binario**. Tomas una cámara IP en una habitación de hospital y necesitas:

1. Recibir video RTSP en tiempo real
2. Decodificar frames H.264
3. Correr modelos ONNX de detección (personas, poses, caras)
4. Evaluar zonas espaciales (cama, silla, puerta)
5. Una máquina de estados clínica (idle → watching → alarmado → blind)
6. Emitir eventos JSONL a stdout para que otro sistema los consuma

**"Clinical-grade"** significa: nunca se te puede escapar un evento. Si alguien se levanta de la cama, tienes que saberlo en < 100ms. No hay "best-effort".

Todo esto corre en **un solo hilo, un solo proceso**. No channels, no spawn, no IPC. Un **superloop estilo PLC** (controlador lógico programable).

---

### 🏛️ **Arquitectura conceptual: el superloop de 4 fases**

```
┌─────────────────────────────────────────────────────┐
│              mana-lite (1 proceso, 1 hilo)           │
│                                                      │
│  ┌─────────┐   ┌──────────┐   ┌──────────┐         │
│  │ INGEST  │──▶│ DECODE   │──▶│ VIZ +    │──▶ loop │
│  │ RTSP    │   │ H264→RGB │   │ HEALTH   │         │
│  └─────────┘   └──────────┘   └──────────┘         │
│       │                             │               │
│       ▼                             ▼               │
│  ┌──────────────────────────────────────┐          │
│  │           EVENT LOGGER               │          │
│  │  JSONL → stdout + archivos rotativos │          │
│  └──────────────────────────────────────┘          │
│                                                      │
│  [placeholder: inferencia, zonas, FSM — v0.x]       │
└─────────────────────────────────────────────────────┘
```

Actualmente las fases de **inferencia, zonas y FSM** están diseñadas en papel (SPEC.md línea 225-250) pero **no implementadas**. Hoy el pipeline hace: ingesta → decodificación → visualización → logging.

---

### 📁 **Estructura del proyecto — 11 módulos en src/**

| Módulo | Líneas | Rol |
|--------|--------|-----|
| `main.rs:1-189` | 189 | Entry point, orquesta el superloop, parsea args |
| `config.rs:1-429` | 429 | Carga y valida los 4 archivos TOML |
| `ingest.rs:1-406` | 406 | Cliente RTSP (retina) + cola de frames + autoreconnect |
| `snapshot.rs:1-230` | 230 | Decode H.264→RGB con ffmpeg + saver de PNG/raw |
| `pipeline.rs:1-64` | 64 | Máquina de estados de la pipeline (runtime) |
| `metrics.rs:1-188` | 188 | Contadores + Health (blind/stale/recovered) |
| `viz.rs:1-119` | 119 | Puente a Rerun para visualización 3D |
| `logger/mod.rs:1-252` | 252 | Logger JSONL con buffer, rotación horaria |
| `logger/event.rs:1-188` | 188 | Tipos de eventos (Frame, Detection, Fsm, ...) |
| `logger/serialize.rs:1-178` | 178 | Serializador JSON manual (sin serde) |
| `error.rs:1-47` | 47 | Enums de error tipados con thiserror |

### 📦 **Crateras internas (workspace en std/)**

| Crate | Rol |
|-------|-----|
| `mana-rtsp` | 1 función: `h264::contains_idr()` para detectar keyframes |
| `mana-types` | Estructuras compartidas: `RawFrameV1`, `DetectionBatchV1`, etc. |
| `mana-video` | `SoftwareDecoder` — wrapper sobre `ffmpeg-next` |
| `mana-viz` | Funciones de logging para Rerun (`log_frame_rgb24`, `log_detections_2d`, etc.) |

---

### 🔄 **El superloop principal — byte por byte**

Este es el corazón. Cada iteración:

```rust
loop {
    let loop_start = Instant::now();
    metrics.tick_cycle();  // paso 0: incrementar contador de ciclos

    // ── PASO 1: INGESTA ──
    // Drain de todos los frames RTSP, quédate solo con el último I-frame
    if let Some(kf) = ingest.poll_freshest_keyframe().await {
        // ── PASO 2: DECODIFICACIÓN ──
        // H.264 (Annex-B) → YUV → RGB24 packed (usa ffmpeg-next)
        let (frame_buf, decode_us) = decoder.decode_timed(&kf.h264);

        // ── PASO 3: ACTUALIZAR ESTADO ──
        // frame_count++, health.touch(), log frame ingest event
        state.on_keyframe(decode_us, &mut metrics, &mut health, &mut log);

        // ── PASO 4: SNAPSHOTS ──
        // Guardar H.264 raw + PNG (si hay decode) a ./snapshots/
        snapshots.save(&kf.h264, frame_buf.as_ref());

        // ── PASO 5: VISUALIZACIÓN ──
        // Enviar frame RGB + métricas a Rerun en 0.0.0.0:9876
        if let (Some(fb), Some(ref mut v)) = (frame_buf.as_ref(), viz.as_mut()) {
            v.log_frame(&header, &fb.rgb, loop_start.elapsed().as_micros());
        }
    }

    // ── PASO 6: CONTADORES RETINA ──
    // Sacar contadores de reconexión, timeouts, SSRC changes
    if let Some(c) = ingest.drain_retina_counters() { ... }

    // ── PASO 7: STUB FSM + HEALTH + METRICS ──
    state.on_fsm_stub(&mut log);      // por ahora: hardcodeado idle→watching
    state.evaluate_health(&mut health, &mut log, &mut metrics);
    log.flush();                      // emitir todos los eventos a stdout/archivo

    // ── PASO 8: VIZ TICK ──
    if let Some(ref mut v) = viz { v.tick(); }  // flush Rerun

    // ── Demo mode: salir después de 5 frames ──
    if state.should_exit() { break; }
}
```

**Punto crítico**: el bloqueo está en `ingest.poll_freshest_keyframe()` — espera hasta que haya un timeout (50ms por defecto) o llegue un I-frame nuevo. Esto es intencional: no queremos loops ocupados (busy-wait).

---

### 📊 **Sistema de eventos — el latido clínico**

El `Logger` emite líneas JSONL. Cada tipo de `Event`:

```rust
Event::Meta { event, detail, attrs }    // startup, model_loaded, shutdown
Event::Health { event, frame_id, ... }  // heartbeat, stale, blind
Event::Frame { frame_id, is_keyframe, decode_ms }  // cada I-frame ingerido
Event::Detection { frame_id, model, infer_ms, detections }  // futuro
Event::Zone { zone, event, class, frame_id }  // futuro
Event::Fsm { from, to, trigger, dwell_ms }  // transiciones de estado
Event::Metrics(MetricsReport)  // reporte de ventana (cada N segundos)
```

**Por qué serializador manual y no serde**: cada frame genera una línea. A 30fps son 2.5M líneas/día. El serializador manual de `serialize.rs`:
- Usa un `Vec<u8>` reusable como buffer (evita allocs por evento)
- Escribe `u64` sin formateo de string (más rápido)
- Escapa JSON inline sin scans extras
- No depende de `serde_json` (unos 50KB de binario menos)

---

### 🚦 **Sistema de Health — los 3 estados**

```
          ┌─────────┐
          │  NORMAL │  frame cada < 5s
          └────┬────┘
               │
        ┌──────▼──────┐
        │    STALE     │  sin frame por > 5s (data_stale_ms/2)
        └──────┬──────┘
               │
        ┌──────▼──────┐
        │    BLIND     │  sin frame por > 10s (data_stale_ms)
        └──────────────┘
               │
               ▼
          RECOVERED    (cuando vuelve un frame)
```

Implementado en `metrics.rs:135-183`. La transición es unilateral con histéresis: no oscila.

---

### 🔌 **Ingesta RTSP — el sistema nervioso**

`ingest.rs` tiene 3 componentes:

1. **`RetinaReader`** — cliente RTSP real usando `retina` crate (Rust puro, sin GStreamer). Abre sesión, hace SETUP de streams "video", PLAY, y drena `CodecItem::VideoFrame`.

2. **`ErrorWindow`** — anillo de errores recientes. Si >25 de 128 operaciones fallan → trigger de reconexión. Evita reconexiones espurias.

3. **Reconexión con jitter** — backoff exponencial (1s→2s→4s...→30s) con jitter aleatorio (mitad del intervalo). Evita tormentas de reconexión.

4. **`AnyReader` enum** — polimorfismo manual para alternar entre `RetinaReader` (producción) y `QueuedReader` (tests/demo).

---

### 📐 **Decisiones de diseño importantes**

| Decisión | Por qué |
|----------|---------|
| **Rotación horaria de logs** | No queremos un JSONL de 2GB. Cada hora: `mana-2026-08-04T15.jsonl`. |
| **Atomic writes** en snapshots | Escribir a `.tmp`, luego `rename()`. Si el proceso muere, no hay archivo corrupto. |
| **Rerun como visualización** | Protocolo gRPC con batching. El blueprint se envía una vez (vista de cámara + señales). |
| **`PipelineState` en módulo separado** | Extraído de `main.rs` en el último refactor para testearlo en aislamiento. |
| **Config por archivos, no CLI flags** | `mana-lite --config mana.toml` es el único argumento. Todo lo demás está en TOML. |
| **`#![allow(dead_code)]` intencional** | Las variantes de `Event`, `Error`, `FsmGuard` etc. son API contract — están diseñadas aunque no se usen aún. |

---

### 🧪 **Tests — 26 passing, 0 failing**

| Módulo | Tests | Cobertura |
|--------|-------|-----------|
| `config.rs` | 5 | Carga de los 4 TOML + validación FSM |
| `ingest.rs` | 8 | Keyframe polling, dedup, P-frame dropping |
| `snapshot.rs` | 5 | Decode, atomic write, sin directorio |
| `logger/mod.rs` | 8 | JSONL escaping, floats negativos, FSM, detección |

Ejecutar: `cargo test -p mana-lite`

---

### 🚧 **Lo que NO está implementado aún (espacio para contribuir)**

1. **Inferencia ONNX** — `ort` crate está en Cargo.toml, `ModelEntry` está listo, pero no hay `infer.rs`. Punto de entrada: la SPEC describe cargar el modelo del `default_model`, preprocesar con `imgsz`, correr `session.run()`, postprocesar NMS.

2. **Zonas espaciales** — `ZoneCatalog` y `ZoneEntry` están definidos. Necesitas un `ZoneEngine` que tome `Vec<Detection>` y devuelva `Vec<ZoneChange>` evaluando si cada bbox intersecta cada zona.

3. **FSM real** — `FsmCatalog`, `FsmTransition`, `FsmGuard` están completos. `on_fsm_stub()` hardcodea `idle→watching`. Necesitas un `FsmEngine` que evalúe guards con dwell timers.

4. **Cascade/timers** — la SPEC habla de `interval_min_ms` por modelo (ej: modelo caro cada 5 frames, modelo barato cada frame). No existe hoy.

5. **`DetRecord` desacoplado de `mana-types`** — hoy `DetRecord` en `logger/event.rs:79` tiene campos planos. `DetectionBatchV1` en `mana-types` es más rico (track_ids, keypoints, máscaras). Hay que reconciliarlos.

---

### 🔧 **Cómo extender el pipeline**

Patrón para añadir una fase al superloop:

1. **Crear módulo** (ej: `src/infer.rs`)
2. **Definir struct** con estado (ej: `InferEngine { session: ort::Session, ... }`)
3. **Agregar fase al loop** en `main.rs` entre INGEST y PUBLISH
4. **Emitir eventos** con `log.emit(Event::nuevo_tipo(...))` — añadir variante a `event.rs` + serialización en `serialize.rs`
5. **Testear** con `QueuedReader` (frames de demo) + modelo ONNX de prueba

**Principio**: cada fase es una función que toma `&mut self` + borrows de las fases anteriores. Nada de canales. Nada de clones en el hot path.

---

### 📍 **Resumen del mapa mental**

```
mana.toml
  ├── source.url → RetinaReader (RTSP, reconnect con jitter)
  ├── inference.* → [POR IMPLEMENTAR: ORT, zonas, FSM]
  ├── health.* → Health monitor (blind/stale/recovered)
  ├── output.* → Logger (JSONL rotativo, stdout)
  ├── viz.* → VizBridge (Rerun gRPC, blueprint)
  └── ingest.* → IngestEngine (drain frames, keyframe dedup)

El loop: ingest → decode → state → snapshots → viz → health → log.flush → repeat
```

