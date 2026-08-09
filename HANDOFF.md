# MANA-LITE — Handoff / Onboarding

> Documento para que una sesión nueva pueda retomar el trabajo **sin leer nada más**.
> Lengua del proyecto: español (comentarios de código y mensajes en español).

## 1. El proyecto en una cápsula

`mana-lite` es un binario Rust que corre un pipeline de visión RTSP en tiempo real:
recibe keyframes H264 por RTSP (`rtp::Ingest`), los decodifica (`decoder`), los
procesa con un modelo de detección, mantiene una máquina de estados de detección
(`fsm`) y publica todo en un log JSONL. Es la base de la tesis de PLC: el log debe
ser **forense** — reconstruible corrida a corrida, con todos los eventos que
importan y las métricas que explican el comportamiento.

Estructura (módulos declarados en `src/main.rs`):

- `src/main.rs` — superloop, `struct App`, bootstrap, `frame_timestamp_ns`.
- `src/ingest.rs` — RTSP + dedup por bytes (`last_h264`), `ErrorWindow` importado.
- `src/pipeline.rs` — `PipelineState`: health (decoder/resize/frames/blind),
  watchdog de panics, `log_cycle_line`, `mark_health_fresh`, `on_keyframe`.
- `src/fsm.rs` — `FsmEngine` (idle/detected/blind), dwell timers, `force_safe_state`.
- `src/metrics.rs` — `MetricsEngine` (infer/cycle metrics, p95), reportes.
- `src/window.rs` — `ErrorWindow` (ventana deslizante compartida).
- `src/config/` — `AppConfig` (app.rs, fsm.rs, observability.rs, metrics.rs...).
- `src/logger/` — `LogManager` (trait `LogSink`), `JsonlHandler`, `event.rs`,
  `serialize.rs` (write_event → JSONL).
- `src/viz.rs` — display de estado en vivo.

## 2. Estado actual (crítico: el working tree NO está commiteado)

Se está implementando una mini-spec de 5 ítems + bonus, de a un ítem con revisión
del usuario entre cada uno. **Todo lo hecho hasta ahora está en el working tree,
sin commit.** Antes de tocar nada: `git status` y `git diff --stat` para verlo.

| ítem | Estado |
|---|---|
| Bonus + ítem 4 (reloj monotónico + una lectura por scan) | ✅ hecho |
| ítem 1 (watchdog de panics por densidad + FSM a blind) | ✅ hecho |
| ítem 3 (presupuesto de ciclo / cycle_overruns / p95) | ✅ hecho (verificado, sin commitear) |
| ítem 2 (apagado ordenado por señal) | ❌ **PENDIENTE — es el objetivo** |
| ítem 5 (dedup por hash) | ❌ pendiente |

Tests: 226 verdes (17 lib + 209 bin). Verificación: `cargo test` y protocolo
clippy (ver §7).

### Resumen de lo hecho (para no repetirlo)

- **Reloj (bonus + ítem 4):** `App` tiene `boot_wall: DateTime<Utc>` y
  `boot_instant: Instant` anclados juntos en bootstrap (una sola fuente de
  verdad). `frame_timestamp_ns(boot_wall, boot_instant, now)` produce el
  timestamp monotónico del frame. `cycle_now = Instant::now()` se lee UNA vez
  por iteración del loop y viaja a `process_keyframe` → `mark_health_fresh` /
  `on_keyframe`. NO re-anclar en rotación horaria (decisión tomada).
- **Watchdog (ítem 1):** `src/window.rs::ErrorWindow` compartido entre
  `ingest.rs` y `pipeline.rs`. Config: `panic_window_cycles` (20) +
  `max_panics_in_window` (3) — la ventana cuenta panics, el 4º dispara
  (`count > threshold`). En el catch del loop: `fsm.force_safe_state(cycle_now)`
  y si `on_panic()` → log + `break`. `force_safe_state` (fsm.rs ~línea 337)
  va a `blind` directo y purga `dwell_timers` + `state_entered_at`.
- **Presupuesto de ciclo (ítem 3):** `tick_cycle_at(now, processed)` mide el
  periodo real del scan contra `cycle_budget_ms` (default 50 = el
  `poll_timeout_ms`, "techo ~20 Hz"). Overrun SOLO en ciclos con trabajo real
  (`processed=true`); el ocio nunca overrunea. El tick va **después** del bloque
  de trabajo en el loop. p95 en aritmética entera (`div_ceil`, sin f64).
  `MetricsReport` + `cycle_min/max/p95_ms`, `cycle_overruns`, `cycle_budget_ms`.
  Línea `cycle:` en el texto (flag `cycle_line`, default true, observability.rs)
  y 5 campos nuevos en el evento metrics del JSONL (serialize.rs ~línea 560).

## 3. El ítem 2 (OBJETIVO): apagado ordenado por señal

**Mini-spec (textual del usuario):**
> Superloop se convierte en un `select!` entre el poll de RTSP y
> `tokio::signal::ctrld::terminate()`; cuando termina, en vez del
> `shutdown("loop_exit")` inalcanzable, `log.shutdown("signal")`; `LogManager`
> implementa `Drop` con flush.

**Aceptación (manual):** `kill -TERM` sobre el proceso corriendo produce un
JSONL (o stdout) cuya **última línea** es el evento shutdown — no truncada.

### Contexto del código actual

- **Cargo.toml:29** — `tokio = { version = "1", features = ["macros", "rt", "net", "time"] }`
  ⚠️ **FALTA la feature `"signal"`** → hay que agregarla. (Linux, así que
  `tokio::signal::unix::signal(SignalKind::terminate())` es válido.)
- **main.rs:548-603** — `async fn run(&mut self, config)`:
  ```rust
  async fn run(&mut self, config: &AppConfig) -> Result<()> {
      loop {
          let cycle_now = Instant::now();
          let mut processed = false;
          if let Some(kf) = self.ingest.poll_freshest_keyframe().await {
              processed = true;
              let result = catch_unwind(AssertUnwindSafe(|| {
                  self.process_keyframe(kf, config, cycle_now);
              }));
              match result {
                  Ok(()) => { let _ = self.state.on_ok(); }
                  Err(e) => {
                      // downcast msg → log → fsm.force_safe_state(cycle_now)
                      // si self.state.on_panic() → log + break   (watchdog densidad)
                  }
              }
          }
          self.drain_ingest_counters();
          self.evaluate_fsm_wildcard(config);
          self.state.evaluate_health(cycle_now, ...);
          self.log.flush();
          self.viz.tick();
      }
      #[allow(unreachable_code)]
      self.log.shutdown("loop_exit");   // ← se reemplaza por shutdown("signal")
      Ok(())
  }
  ```
- **main.rs:328-334** — `let mut log: Box<dyn LogSink> = Box::new(log);` (el
  `LogManager` concreto vive dentro del trait object; al dropearse el Box al
  final de `main()` se ejecuta el `drop` concreto — base para el `Drop`).
- **logger/mod.rs:16-20** — trait `LogSink { emit; flush; shutdown(reason) }`.
- **logger/mod.rs:32-121** — `LogManager` con `shutdown(reason)` (emite evento
  `Event::Meta { event: "shutdown", detail: reason, attrs: [uptime] }` + flush).
- **logger/mod.rs:176-195** — `JsonlHandler::handle_event` (buffer: `Vec<Event>`)
  y `flush_buffer` (drain → `write_event` → `write_buf`). El flush normal pasa
  por `BufWriter`; un `kill -9` dejaría líneas truncadas — eso es justo lo que
  el shutdown evita.
- El `break` por densidad de panics ya existe en el loop (parte del ítem 1);
  no romperlo.

### Plan de implementación sugerido

1. `Cargo.toml:29` → features `["macros", "rt", "net", "time", "signal"]`.
2. En `run()`, antes del `loop` (una sola vez, fuera del loop para no reinstalar
   el handler cada iteración):
   ```rust
   let mut term = tokio::signal::unix::signal(SignalKind::terminate())?;
   ```
   (con `use tokio::signal::unix::{SignalKind, signal}`).
3. Convertir el poll en un `select!`:
   ```rust
   tokio::select! {
       kf = self.ingest.poll_freshest_keyframe() => {
           if let Some(kf) = kf {
               // ...todo el cuerpo actual del if (processed, catch_unwind, ...)
           }
       }
       _ = term.recv() => break,
   }
   ```
4. Reemplazar `#[allow(unreachable_code)] self.log.shutdown("loop_exit")` por
   `self.log.shutdown("signal")`.
5. `impl Drop for LogManager { fn drop(&mut self) { self.flush(); } }` — red de
   seguridad para caminos que no pasan por `run()` (bootstrap que falla, etc.).
6. Tests (ver §6) y verificación clippy (§7).

### Trampas conocidas (NO pisarlas)

- ⚠️ **En `select!` el branch del poll debe ser `kf = ... => { if let Some(kf) = kf {...} }`,
  NUNCA `Some(kf) = ... => {...}`.** El poll resuelve `None` cuando expira el
  timeout del ingest (ocio normal); si el patrón no matchea, `select!` descarta
  el resultado y queda esperando solo el otro branch → el loop se cuelga en ocio
  (los ciclos de 50 ms desaparecen).
- `term.recv()` devuelve `Option<()>`; el branch `_ = term.recv() => break`
  espera a que llegue la señal; si el handler se cierra devuelve `None` (no
  ocurre acá, es un-only signal). No anidar `select!`s ni reinstalar el handler.
- El cuerpo del branch conserva `processed`/`cycle_now`/`catch_unwind`/watchdog
  tal cual; el ítem 3 depende de que `tick_cycle_at` siga después del bloque.
- El `shutdown` ya emite el evento con uptime; no duplicar flush manual después.
- La variable `log` es `Box<dyn LogSink>` — el `Drop` del `LogManager` corre vía
  drop-glue del trait object, sin cambios en la estructura.

## 4. Decisiones de diseño pendientes (preguntar al usuario si aplica)

- ¿Distinguir razones de salida? (`shutdown("signal")` vs `shutdown("panic")` en
  el break del watchdog — hoy solo existe `"loop_exit"` inalcanzable). La
  mini-spec pide `"signal"`; lo mínimo es solo eso.
- ¿Añadir también SIGINT (`tokio::signal::ctrl_c()`) al select? La spec pide
  solo `terminate()`; lo mínimo es solo eso.
- **Commitear el estado actual ANTES de empezar** (todo el trabajo previo está
  sin commitear — proponérselo al usuario, no commitear por cuenta propia).

## 5. Reglas de la casa

- Comentarios y docs en español; sin comentarios triviales.
- No agregar dependencias nuevas sin preguntar.
- Nada de `unsafe`, `unwrap` en rutas no-test, ni `anyhow` en errores nuevos
  (usar errores propios/`io::Result` como el resto).
- Seguir los patrones existentes: `new`/`new_at(now)` para tests, `MetricsReport`
  con campos agrupados, flags en `MetricsTextConfig` con default true.
- No tocar clippy preexistente (ver §7).

## 6. Verificación

- **Tests:** `cargo test` — deben quedar 226+ (17 lib + 209 bin + los nuevos).
- **Tests nuevos sugeridos** (infraestructura ya existente en
  `logger/mod.rs::mod tests`):
  - `LogManager` con un handler de prueba: `shutdown("signal")` → el último
    evento emitido es `Event::Meta` con `event=="shutdown"`, `detail=="signal"`,
    y el buffer quedó drenado (flushed).
  - `drop(manager)` → se llama `flush` (handler de prueba que registra flush).
  - Si se quiere: test del `select!` es difícil en unit; la aceptación es
    manual. No romper los tests de logger existentes
    (p. ej. `health_blind_has_message`, ~línea 705).
- **Aceptación manual:** correr el binario (puede ser sin `save_dir`, a stdout,
  o con config de rotación) y `kill -TERM <pid>`; la última línea debe ser
  `{"type":"meta","event":"shutdown","detail":"signal",...}` con newline final,
  no truncada. Con `save_dir`: el archivo JSONL termina en esa línea.
- **Clippy (protocolo):**
  ```bash
  cargo clippy --all-targets --bin mana-lite
  ```
  El repo tiene ~580 warnings preexistentes ("deuda de casa"). Método:
  `git stash` → guardar warnings de HEAD → `git stash pop` → comparar por
  módulo/archivo. Clases aceptadas (preexistentes, no ampliar): `const fn` en
  defaults de serde, `more than 3 bools` (MetricsTextConfig), `too many lines`
  (serialize.rs). NO introducir warning de clases nuevas ni en código nuevo.

## 7. Lo que NO tocar (acuerdos cerrados)

- `min_hits`, `update_single_person`, `ignore_persons` — no-goals sancionados
  de la tesis.
- No re-anclar el reloj en rotación horaria (decisión del ítem 4).
- No cambiar semántica del watchdog (ventana por ciclos, `count > threshold`).
- No tocar `ErrorWindow` ni `force_safe_state` (ítem 1 cerrado).
- **ítem 5 (dedup por hash: `last_h264: Option<Vec<u8>>` →
  `last_digest: Option<u64>` en ingest.rs:100-106)** NO se hace en esta sesión.
- No refactors fuera del alcance; cada ítem termina con resumen de semántica
  para que el usuario lo revise ANTES de seguir.

## 8. Contexto externo que puede servir

- Base de HEAD: sesión previa que cerró el gate de salud de decodificación
  (`mark_health_fresh` solo cuando el frame decodifica bien, `on_keyframe` con
  health, tests). Todo lo de la mini-spec está encima, sin commitear.
- El usuario revisa cada ítem ("Andá de a uno y te los reviso igual que los
  anteriores") — al terminar el ítem 2, presentar resumen de semántica y
  esperar OK antes del ítem 5.
