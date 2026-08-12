# Fase 0 — Sanear la línea base

Plan de implementación. Ejecuta Ernesto; Claude revisa y pule al cierre.

**Referencias:** [ROADMAP.md](../../ROADMAP.md) · [ARCHITECTURE.md](../../ARCHITECTURE.md) ·
[ADR-033](../adrs/033-isolated-control-loop.md)

---

## Objetivo

Dejar la línea base verde y limpia antes de tocar la arquitectura.

No se puede refactorizar contra una base en rojo: con bugs conocidos en el punto
de partida, es imposible distinguir *"lo rompió el refactor"* de *"ya estaba
roto"*. Las compuertas de las fases 1 a 5 son los escenarios del `workshop/`, y
sólo sirven si hoy pasan.

**Criterio de salida.** Suite completa en verde, `01-ingest-only` y
`02-ingest-viz` pasando sus compuertas en corridas largas, historia de git limpia
y sin knobs muertos.

---

## Orden de ejecución

**Invertido respecto de la numeración del roadmap, a propósito.**

```
T1  Borrar image_max_res        ← antes de commitear
T2  Commitear la base           ← historia limpia
T3  Vencimiento del dedupe      ← su propio commit
T4  Correr las compuertas
```

`image_max_res` va primero porque si se commitea y después se borra, queda en la
historia el rastro de una decisión que ya sabemos equivocada. Borrándolo antes,
**nunca entra al repositorio**.

---

## T1 — Borrar `image_max_res` y la decimación

### Por qué

El knob achica la imagen mientras los overlays siguen en coordenadas de
resolución completa: desalinea. A cambio ofrece una reducción de bitrate que
`image_format = "jpeg"` ya da mejor —- 31,8× contra 4×— y **sin tocar la
resolución**, con lo cual no hay nada que compensar.

Un knob que hay que verificar visualmente antes de confiar es un knob que no
deberías tener. Y con esto la pregunta abierta del escenario 02 —- *"¿desalinea la
decimación?"*— se cierra por desaparición del caso.

### Qué tocar

**Código**

| Archivo | Qué sacar |
|---|---|
| `src/config/app.rs` | campo `image_max_res`, `default_viz_image_max_res()`, su línea en `impl Default`, y el bloque de doc con la tabla de factores |
| `src/viz/mod.rs` | campo `image_max_res`, parámetro de `VizBridge::new`, línea en `disabled()` |
| `src/viz/frame.rs` | fns `downscale_factor` y `decimate_rgb24`; en `log_frame`, la rama `scaled` y todo el manejo de `(pixels, width, height)` |
| `src/viz/tests.rs` | `image_max_res: 0` del literal |
| `src/app/bootstrap/observers.rs` | argumento `config.viz.image_max_res` |

**Tests a eliminar** (en `src/viz/frame.rs`): `factor_is_derived_from_target_resolution`,
`decimation_of_short_source_fills_black_instead_of_panicking`,
`decimation_divides_dimensions_and_payload`.

Conservar `jpeg_preserves_dimensions_and_shrinks_payload` y
`unknown_image_format_falls_back_to_raw`.

**Simplificación resultante en `log_frame`.** Sin decimación queda un solo eje de
decisión y el cuerpo se reduce mucho:

```rust
let complete = rgb.len() >= expected;
let result = match self.image_format {
    VizImageFormat::Jpeg { quality } if complete => {
        match encode_jpeg(rgb, header.width, header.height, quality) { … }
    }
    _ => log_frame_rgb24(rec, path, header, rgb),
};
```

Si al terminar el cuerpo no quedó más simple que antes, algo se arrastró de más.

**Configuración y workshop**

- `workshop/scenarios/02-ingest-viz/mana.toml` — sacar la línea y la tabla del
  comentario; dejar sólo el contraste raw vs jpeg.
- `workshop/scenarios/02-ingest-viz/run-variant.sh` — eliminar las variantes
  `c-raw-downscaled` y `d-jpeg-downscaled`; quedan `a-raw-native` y
  `b-jpeg-native`, que son las que responden la pregunta que importa.
- `workshop/scenarios/02-ingest-viz/README.md` — sacar la tabla de modos con
  `image_max_res` y **la sección de verificación en el viewer sobre alineación**:
  esa pregunta ya no existe. Conservar la verificación de que el ROI cae bien con
  `b-jpeg-native`, que sigue siendo válida como control.
- `workshop/scenarios/home-1/02-ingest-viz/` — mismo tratamiento; es una copia y
  se desincroniza si se la olvida.
- `docs/wiki/2.1-appconfig-and-pipeline-toggles.md` — fila de la tabla `[viz]` y
  la mención en el párrafo siguiente.
- `docs/wiki/6-visualization-(rerun-integration).md` — la tabla de *Frame
  Encoding and Link Budget* pasa a dos filas (raw / jpeg) y se va el párrafo
  sobre el lado mayor y `div_ceil`.

`workshop/runs/` está en `.gitignore`; no hay que tocarlo.

### Verificación

```sh
cargo test --release
grep -rn 'image_max_res\|decimate\|downscale' src/ docs/wiki/ workshop/scenarios/
```

El `grep` tiene que volver vacío. Si queda una mención en un comentario, es deuda
de documentación que contradice el código.

---

## T2 — Commitear la base

### Agrupación propuesta

Cuatro commits. El árbol tiene cambios entrelazados —- `src/viz/mod.rs` toca
liveness y encoding a la vez— así que **no gastes una hora en cirugía con
`git add -p`**: cuatro commits coherentes valen más que siete quirúrgicos.

**1. `fix(ingest): hacer cancelación-seguro el drenaje de keyframes`**

`src/ingest.rs`

En el cuerpo: que el keyframe vivía en la pila del future, que `select!` descarta
la rama perdedora, que el contador `seen` sobrevivía y el frame no, y la firma
observable (`0 processed (N seen)` con `dup` y `kf_dropped` en cero). Mencionar
que la Fase 4 vuelve innecesario el invariante.

**2. `fix(viz): distinguir contrapresión de desconexión y encodear JPEG`**

`src/viz/{connection,mod,frame,tests}.rs`, `src/config/app.rs`,
`src/app/bootstrap/observers.rs`

En el cuerpo: que `connect_grpc_opts` es lazy y por eso `connected` sólo se
declara tras un flush exitoso; que `Timeout` es contrapresión y `Failed` es
desconexión; que el backoff no crecía porque vivía en `Inner::Disconnected`; y
que JPEG baja el payload 31,8× sin tocar resolución.

**3. `fix(metrics): el presupuesto de ciclo no medía nada`**

`src/metrics/{mod,tests}.rs`, `src/app/mod.rs`

En el cuerpo: que la condición exigía `processed` y el único llamador de
producción lo pasaba en `false` incondicionalmente, con lo cual el contador no
podía subir jamás; y que la exención tapaba justo el caso importante, un scan
bloqueado en visualización.

**4. `docs: arquitectura, ADRs 033-035, roadmap y banco de escenarios`**

`ARCHITECTURE.md`, `BIGPICTURE.md`, `ROADMAP.md`, `docs/adrs/03{3,4,5}-*.md`,
`docs/sprints/`, `docs/wiki/*`, `workshop/`, `.gitignore`

### Antes de commitear

- `cargo test --release` en verde.
- `cargo clippy --release --all-targets` sin *warnings nuevos*. Hay warnings
  preexistentes en `ultralytics-inference` y dos en `mana-lite`; no los arregles
  acá, sólo asegurate de no sumar.
- Confirmar que `.gitignore` incluye `workshop/runs/` y que no se coló ningún
  `.jsonl` ni config generada.

---

## T3 — Vencimiento del dedupe de keyframes

### El problema

`poll_freshest_keyframe` descarta un keyframe cuyo digest coincide con el
anterior emitido, y **esa comparación no vence**:

```rust
let digest = h264_digest(&kf.h264);
if self.last_digest == Some(digest) {
    self.keyframes_dup += 1;
    return None;
}
```

Ante una escena completamente inmóvil, un encoder puede emitir IDR
byte-idénticos. La supresión no termina nunca, el sistema queda ciego, y **la
propia supresión dispara `data_stale`**: aguas arriba no hay forma de distinguir
"la escena no cambió" de "el stream murió".

En un sistema clínico esa confusión no es aceptable. `data_stale` significa
"perdí la señal" y tiene que seguir significando eso.

### La corrección

Emitir igual si hace demasiado que no se emite nada:

```rust
let kf = self.staged.take()?;
let digest = h264_digest(&kf.h264);
let since_emit_ms = self.last_keyframe_at.elapsed().as_millis() as u64;
if should_suppress(self.last_digest, digest, since_emit_ms, self.dedup_max_suppress_ms) {
    self.keyframes_dup += 1;
    return None;
}
```

### Testeabilidad: extraer la decisión

`last_keyframe_at` es un `Instant`, así que probar el vencimiento exigiría
manipular el reloj. En vez de inyectarlo —- que obligaría a cambiar la firma de un
future que vive dentro de un `select!`— **extraé la decisión a una función pura**:

```rust
/// Decide si un keyframe duplicado debe suprimirse.
///
/// La supresión vence: ante una escena inmóvil un encoder puede emitir IDR
/// byte-idénticos indefinidamente, y sin vencimiento la propia supresión
/// terminaría disparando `data_stale` — volviendo indistinguible "la escena no
/// cambió" de "el stream murió".
fn should_suppress(
    last_digest: Option<u64>,
    digest: u64,
    since_emit_ms: u64,
    max_suppress_ms: u64,
) -> bool {
    last_digest == Some(digest) && since_emit_ms < max_suppress_ms
}
```

Se prueba exhaustivamente sin tocar el reloj, y el camino de integración queda de
una línea. Si preferís consistencia con [ADR-029](../adrs/029-injected-clock.md)
e inyectar el reloj, es defendible —- pero es más cambio por el mismo resultado, y
la Fase 4 va a rehacer esa firma igual.

### Configuración

En `[ingest]`, `src/config/app.rs`:

```rust
/// Cuánto puede suprimirse un keyframe idéntico antes de emitirlo igual.
///
/// Debe quedar por debajo de `[health] data_stale_ms`: si no, una escena
/// inmóvil sigue derivando en `data_stale` y este knob no gobierna nada.
#[serde(default = "default_dedup_max_suppress_ms")]
pub dedup_max_suppress_ms: u64,   // default: 5_000
```

Default 5.000 ms = mitad del `data_stale_ms` por defecto (10.000).

**Validación al arrancar, obligatoria.** En el bootstrap, rechazar
`dedup_max_suppress_ms >= health.data_stale_ms` con un diagnóstico claro. Es la
regla cultural del proyecto: un knob que se declara tiene que gobernar algo, y
con ese valor no gobernaría. Seguí el patrón de `catalogs.rs:187`
(`pipeline.track`) para el estilo del error.

### Propagación

`IngestEngine::new` pasa a recibir el valor. Tres sitios:
`src/app/bootstrap/observers.rs:69`, `src/ingest.rs:387` (helper de tests),
`src/app/tests.rs:231`.

### Tests

1. `should_suppress` — matriz completa: digest distinto (nunca suprime), digest
   igual dentro de ventana (suprime), digest igual fuera de ventana (emite),
   `max_suppress_ms = 0` (nunca suprime).
2. Integración: los tests existentes de dedupe siguen pasando con una ventana
   holgada.
3. Config: la validación rechaza `dedup_max_suppress_ms >= data_stale_ms`.

### Métricas — ya está hecho

`keyframes_dup` ya se drena en `IngestCounters` y el flag `ingest_dup` ya está en
`true` en `config/metrics.toml`, así que sale como `dup:N` en la línea de
ingesta. **No hay trabajo de instrumentación acá.** (Fue lo que permitió
falsificar el diagnóstico inicial de este mismo bug: `dup` valía 0 y por eso la
causa era otra.)

Commit: `fix(ingest): dar vencimiento a la supresión de keyframes duplicados`.

---

## T4 — Compuertas

```sh
./workshop/scenarios/02-ingest-viz/run-variant.sh a-raw-native  180
./workshop/scenarios/02-ingest-viz/run-variant.sh b-jpeg-native 180
cargo run --release -- --config workshop/scenarios/01-ingest-only/mana.toml   # ≥180s
```

| Compuerta | Criterio |
|---|---|
| Suite | `cargo test --release` en verde |
| Escenario 01 | deriva acumulada ≤ 1, cero ventanas muertas, sin `stale` ni `blind` en el JSONL |
| Escenario 02 `b-jpeg-native` | una sola línea `viz: connected`, cero reconexiones rtsp |
| Higiene | `grep` de `image_max_res` vacío |

**`a-raw-native` no es compuerta de esta fase.** Satura el enlace a propósito y
hoy es no determinista —- degrada en unas corridas y no en otras. Correrla igual y
**anotar el resultado en el README del escenario**: es la línea base contra la
que se mide la Fase 2, que es la que debe volverla determinista.

Corridas de 180 s, no de 45. La degradación de `a-raw-native` no apareció en los
primeros 45 s de la corrida que la mostró.

---

## Qué voy a revisar al cierre

Para que sepas contra qué se mide:

1. **Que el borrado sea borrado.** Sin código muerto "por si acaso", sin
   menciones huérfanas en comentarios o docs.
2. **Que `log_frame` haya quedado más simple**, no igual de complejo con una rama
   menos.
3. **Que la validación del nuevo knob exista** y falle con un mensaje que diga
   qué hacer, no sólo qué pasó.
4. **Que los mensajes de commit expliquen el porqué**, no el qué. El `git log` es
   donde se busca la razón de una decisión seis meses después.
5. **Que los tests fijen el comportamiento, no la implementación.** Un test que
   se rompe al renombrar una variable interna no está fijando nada.
6. **Que la wiki no contradiga al código.** Es la regla permanente del proyecto.
7. **Que las corridas de compuerta estén anotadas** con sus números, no con "dio
   bien".

Cuando termines, avisame y hago la pasada de revisión y pulido.
