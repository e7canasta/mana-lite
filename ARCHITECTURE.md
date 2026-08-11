# mana-lite — Architecture

Cómo está construido. Hilos, relojes, puertos e invariantes.

Todo lo que sigue está verificado contra el código, con referencia a archivo y
línea. El *por qué* conceptual está en [BIGPICTURE.md](BIGPICTURE.md).

---

## 1. El super loop

Todo el trabajo del sistema ocurre en **una sola task de tokio**, en
`App::run` (`src/app/mod.rs:65`):

```rust
loop {
    tokio::select! {
        kf = self.ingest.poll_freshest_keyframe() => { /* decode + inferencia */ }
        _  = scan_interval.tick()                 => { /* control + observabilidad */ }
    }
}
```

Dos ramas compiten. Hay que entender tres cosas de esta construcción antes de
tocar cualquier cosa que corra adentro.

### 1.1 `select!` cancela la rama perdedora

Cuando una rama gana, el future de la otra **se descarta**. Cualquier estado que
viva en la pila de ese future se pierde.

Esto no es teórico: fue el bug más caro de esta base de código.
`poll_freshest_keyframe` acumulaba el keyframe en una variable local mientras
incrementaba `keyframes_seen` sobre el engine. Al cancelarse, el contador
sobrevivía y el frame no —- el sistema informaba "5 vistos, 0 procesados" y el FSM
se iba a `data_stale` con el transporte perfectamente sano.

> **Invariante.** Todo future poleteado dentro del `select!` debe ser
> cancelación-seguro: sus observaciones se comprometen a `self` en el momento en
> que se hacen, nunca al final del bucle. Fijado por
> `ingest::tests::staged_keyframe_survives_cancellation`.

### 1.2 El trabajo pesado es síncrono y bloquea la task

`process_keyframe` (`src/app/mod.rs:145`) **no es `async`**. Decodificar y correr
la cascada de modelos ocurre en línea, bloqueando la task completa:

| Etapa | Costo medido |
|---|---|
| decode 1080p | ~14 ms |
| inferencia (`detect-fast`, 320px) | ~216 ms |

Mientras eso corre, `scan_interval.tick()` **no puede dispararse**. El scan
nominal de 5 Hz se detiene durante todo el procesamiento del keyframe.

Se ve en cualquier reporte de ciclo: `p95 200ms max 200ms min 1ms`. Ese
**`min 1ms` es la marca del tick que se recuperó de golpe** después de un bloqueo.

### 1.3 Los ticks perdidos se recuperan en ráfaga

`tokio::time::interval` (`src/app/mod.rs:71`) se construye sin
`set_missed_tick_behavior`, o sea con el default **`MissedTickBehavior::Burst`**:
los ticks que no pudieron dispararse salen inmediatamente, uno tras otro, hasta
alcanzar el reloj.

Eso no es un accidente afortunado: es **de lo que depende la corrección del reloj
de control**. Ver la sección 2.

---

## 2. Los dos relojes

El sistema tiene dos nociones de tiempo, y confundirlas es la forma más fácil de
romperlo en silencio.

### 2.1 Reloj real

`Instant::now()`. Lo usan las métricas (`tick_cycle_at`), la salud
(`data_stale_ms`) y la ingesta.

### 2.2 Reloj virtual de control

`ScanTimeline` (`core/mana-control/src/scan.rs:459`):

```rust
pub fn now(&self) -> ScanInstant {
    ScanInstant::from_instant(self.start + Duration::from_millis(self.tick * self.period_ms))
}
pub fn advance(&mut self) -> ScanInstant { self.tick += 1; self.now() }
```

**El tiempo de control es una cuenta de ticks por el periodo, no una lectura del
reloj.** Toda la política clínica —- confirmación de presencia, dwell del FSM,
histéresis de zonas —- corre sobre este reloj virtual.

### 2.3 Por qué eso funciona, y de qué depende

Dos scans pueden ejecutarse con 1 ms real de diferencia (una ráfaga de
recuperación) y el control creerá que pasaron 200 ms entre ellos. Sin embargo el
reloj virtual **no deriva**, porque `Burst` garantiza exactamente un tick por
periodo transcurrido: el tick que se recupera corresponde a tiempo real que sí
pasó durante el bloqueo.

> **Invariante crítico.** El reloj de control se mantiene alineado con el reloj
> de pared **únicamente** porque los ticks perdidos se recuperan uno a uno.
> Cambiar `MissedTickBehavior` a `Skip` o `Delay` haría que el reloj virtual
> corriera más lento que el real, y **todos los tiempos clínicos configurados
> durarían más de lo que dicen**, sin ningún error visible. Si alguna vez se
> toca esa configuración, hay que reemplazar `ScanTimeline` por una derivación
> del reloj real en el mismo commit.

---

## 3. Los puertos entre subsistemas

`docs/wiki/1-overview.md` declara cuatro subsistemas: Ingesta, Inferencia,
Control y Observabilidad. La calidad de las fronteras no es pareja.

| Frontera | Puerto | Estado |
|---|---|---|
| Percepción → Control | `ProcessImage` / `SceneSample` | declarado y respetado |
| Control → dominio | `SceneEvent` | declarado y respetado |
| Pipeline → Observabilidad | `PipelineObserver` | **declarado y evadido** |

### 3.1 La costura

`PipelineObserver` (`src/app/observer.rs:10`) es la abstracción correcta: define
`on_occupancy`, `emit`, `flush`, y existe un `NullObserver` para tests. La wiki
la documenta como el mecanismo de fan-out (`docs/wiki/6.2`).

Pero el código la honra en **un solo método**. Los demás sitios entran por
`FanoutObserver.viz`, que es un campo `pub`, salteándose la abstracción —- y
`viz_mut()` devuelve `&mut VizBridge`, el **tipo concreto**, así que aunque se la
usara no permitiría sustituir la implementación.

Consecuencia directa y medida: **la visualización corre en el hilo del control y
puede frenarlo.** rerun aplica contrapresión al productor cuando su batcher se
llena (`max_bytes_in_flight`, y `re_chunk` no ofrece política de descarte), de
modo que un enlace saturado bloquea el `log()` del pipeline. Observado: un scan
de **8,2 segundos** drenando frames de 6,2 MB, con 8 keyframes descartados
durante la parada.

> **Regla.** Un subsistema sin puerto declarado termina cableado inline. La
> ausencia de puerto para Observabilidad no es una omisión cosmética: es la causa
> de que una preocupación de depuración pueda degradar el lazo de control de un
> sistema clínico.

Ver la sección 6 para el estado del trabajo sobre esto.

---

## 4. Configuración

Tres capas, resueltas en `App::bootstrap` (`src/app/bootstrap/`):

```
config/mana.toml
  └─ inference.blueprint_file → blueprints/<perfil>/blueprint.toml
        ├─ model_overlay      → models.toml del perfil, sobre config/models.toml
        └─ fsm / zones / depth-rules   (todos Option)
```

`zones_file`, `fsm_file` y `depth_rules_file` son `Option<PathBuf>`
(`src/config/app.rs:468-472`): un despliegue mínimo puede omitirlos y no se
compila lo que no existe. Es lo que hace posible el escenario de ingesta pura del
workshop.

### 4.1 Validación al arrancar

El bootstrap rechaza configuraciones inconsistentes en vez de degradarlas: que el
`primary_model` exista, que las zonas referidas por guardas estén definidas, que
un blueprint con `requires_tracking` no corra con `pipeline.track = false`
(`src/app/bootstrap/catalogs.rs:187`), y que cada guarda `signal` sea evaluable
contra el catálogo v1.

Esto es aplicación de la regla cultural del proyecto: **un knob que se declara
tiene que gobernar algo**. El comentario de `parse_dwell`
(`core/mana-control/src/fsm/guard.rs`) lo dice sin rodeos —- aceptar un valor que
no se aplica es "la misma clase de mentira que un knob que se ignora".

---

## 5. Observabilidad

Tres salidas, con propósitos distintos:

| Salida | Para qué | Camino |
|---|---|---|
| JSONL | registro auditable, esquema v2 | `src/logger/` |
| Métricas | reportes cada `report_interval_s` | `src/metrics/` |
| Rerun | inspección visual en vivo | `src/viz/` |

### 5.1 El presupuesto de ciclo

`[health] cycle_budget_ms` enfrenta el **periodo real** entre scans contra un
límite. Mide periodo, no trabajo: un ciclo ocioso duerme hasta el tick y cae muy
por debajo del presupuesto por construcción, así que si el periodo se pasa es
porque algo bloqueó el lazo —- que es exactamente lo que el presupuesto existe
para delatar.

> **Histórico.** Hasta 2026-08-11 la condición era
> `if processed && delta > budget`, y el único llamador de producción pasaba
> `processed: false` incondicionalmente (`src/app/mod.rs:199`). El contador no
> podía subir nunca y todo `0 overruns` impreso era una tautología. La exención
> tapaba justamente el caso importante: un scan bloqueado en la visualización no
> procesa keyframes, así que quedaba exento. Fijado por
> `metrics::tests::a_stalled_cycle_without_work_still_trips_the_budget`.

### 5.2 El bridge de Rerun

Dos distinciones que el bridge debe mantener y que no son obvias:

- **Tener un sink no es tener un viewer.** `connect_grpc_opts` es lazy: devuelve
  `Ok` sin haber contactado a nadie. `Inner::Connected` significa "hay dónde
  escribir"; la vida se establece con el primer `flush` que devuelve `Ok`, y sólo
  ahí se loguea `viz: connected`.
- **Contrapresión no es desconexión.** `flush_with_timeout` devuelve `Timeout`
  (el enlace vive, no drenó a tiempo) o `Failed` (no hay viewer). Colapsarlos
  hacía que un frame grande sobre un enlace lento se leyera como caída, se
  soltara la conexión, se reenviara el blueprint y se reseteara el layout del
  viewer cada dos segundos.

### 5.3 Presupuesto del enlace

Un frame 1080p RGB24 son 6.220.800 B. A un keyframe por segundo son ~50 Mbit/s
sostenidos. `[viz] image_format = "jpeg"` baja el payload ~32× **sin tocar la
resolución**, que es lo que importa: los overlays se loguean en coordenadas de
píxel nativas, así que conservar las dimensiones evita tener que compensar
geometría.

---

## 6. Deuda estructural conocida

En orden de importancia.

### 6.1 Observabilidad sin puerto — *en curso*

La visualización corre en el hilo del control y puede frenarlo (§3.1). La
corrección es completar el seam que ya está declarado:

1. Un trait `VizSink` con la superficie de los 20 métodos —- el puerto faltante.
2. `VizBridge` lo implementa directo (síncrono).
3. Un `VizRelay` lo implementa reenviando a un hilo propio por una cola acotada
   que **descarta en vez de bloquear**. rerun no ofrece esa política, así que hay
   que ponerla afuera.
4. `FanoutObserver.viz` pasa a `Box<dyn VizSink>`, con lo que los sitios de
   llamada existentes siguen compilando por deref.

El punto de diseño: el costo de convertir prestado→propio se paga **una vez, en
el relay**, no repartido por el pipeline.

### 6.2 El dedupe de keyframes no vence — *latente*

`poll_freshest_keyframe` descarta un keyframe cuyo digest coincide con el
anterior procesado, y esa comparación no tiene ventana. Ante una escena
completamente inmóvil, un encoder que emita IDR byte-idénticos suprimiría
indefinidamente, y **la propia supresión dispararía `data_stale`**: aguas arriba
no habría forma de distinguir "la escena no cambió" de "el stream murió".

### 6.3 El presupuesto cuenta pero no actúa — *abierto*

`cycle_overruns` ya es una medición real (§5.1), pero contar no es actuar. Un
overrun sostenido debería degradar algo —- apagar viz, bajar calidad— o al menos
escalar el aviso.

### 6.4 Los modelos se cargan aunque la inferencia esté apagada — *menor*

`pipeline.infer = false` corta la ejecución (`src/app/mod.rs:182`) pero no la
carga: el bootstrap construye igual la sesión ONNX. Un despliegue sin inferencia
paga el arranque y la memoria del modelo.

---

## 7. Cómo se verifica

`workshop/` contiene escenarios que encienden **una capa por vez**, cada uno con
criterios de aceptación escritos antes de correr. Con el pipeline completo
prendido hay seis sospechosos ante cualquier anomalía; con la escalera, hay uno.

| Escenario | Capa que agrega |
|---|---|
| `01-ingest-only` | RTSP → decode → JSONL |
| `02-ingest-viz` | bridge de Rerun |

Regla de invocación: **siempre `cargo run`, nunca una ruta fija al binario.**
`target-dir` puede estar redirigido por configuración global de cargo, en cuyo
caso un `./target/` viejo en el repo sigue siendo ejecutable y no se actualiza
jamás. Ver `docs/wiki/1.1-getting-started.md` → *Build and Run*.
