# mana-lite — Architecture

Cómo está construido. Hilos, relojes, puertos e invariantes.

Todo lo que sigue está verificado contra el código, con referencia a archivo y
línea. El *por qué* conceptual está en [BIGPICTURE.md](BIGPICTURE.md).

---

## 1. Tres etapas, ningún borde que bloquee

El sistema corre en **tres etapas con dueños de ejecución distintos**, unidas por
bordes que no pueden hacer esperar a nadie (ADR-033, ADR-034):

```
[task tokio]      RTSP → demux → dedupe            async, sólo I/O de red
      │  Slot<RawKeyframe>
      ▼
[hilo percepción] decode → cascada → ProcessImage  CPU-bound, ~221 ms
      │  Slot<PerceptionOutput>          Cola<Vec<Event>>
      ▼                                        │
[task tokio]      scan() @ 200 ms  ◄────────────┘   trabajo acotado
      │  Slot<ControlDirective>  ──► vuelve a percepción
      ▼
      JSONL
```

La regla que decide la primitiva de cada borde es de ADR-034 y no se negocia por
conveniencia:

> Si perder el dato viejo es **correcto**, es una muestra: va en `Slot`.
> Si perderlo es un **bug**, es un evento: va en cola.

Un frame es una muestra —nadie quiere ver el cuarto como estaba hace ocho
segundos—; una detección del JSONL es un evento, y perderla rompe la auditoría.

### 1.1 El lazo de control es un temporizador puro

`App::run` (`src/app/mod.rs`) ya no arbitra entre trabajo y reloj. Su `select!`
sólo elige entre el vencimiento del scan y las señales de apagado:

```rust
tokio::select! {
    () = tokio::time::sleep_until(scan_deadline.next().into()) => { /* scan_tick */ }
    _ = term.recv() => break,
    _ = &mut ctrl_c => break,
}
```

Todo lo que hace `scan_tick` está acotado por construcción: drenar la cola de
eventos, tomar la imagen de proceso del slot, avanzar el reloj, evaluar, emitir.
Ningún paso puede esperar a la red, a un modelo ni al disco de otra etapa.

**Nada con latencia variable puede vivir acá adentro.** Si algo la tiene, entra
como etapa, no como llamada.

### 1.2 El lazo cerrado: control decide qué mira percepción

Percepción **no es una fuente, es el actuador de un lazo**. El FSM decide qué
modelos corren (`fsm.current_models()`) y el tracker decide dónde recortar para
la cascada. Los dos viven del lado de control.

Por eso hay dos slots en direcciones opuestas y no un pipeline. Leer el tracker
directo desde el hilo de inferencia obligaría a un candado que ese hilo podría
estar sosteniendo justo cuando el lazo tiene que ticar — el bloqueo que la
arquitectura saca, reintroducido con otro nombre.

La directiva viaja como muestra: llega hasta un periodo desactualizada, y
alcanza. Antes de la separación ya se leía igual de vieja —el tracker se muta en
`scan()` a 5 Hz y percepción corre a 1 Hz—, así que no se perdió frescura.

### 1.3 Qué compensa cada etapa cuando la otra falla

| Falla | Qué pasa |
|---|---|
| percepción entra en pánico | descarta su imagen entera y sigue; el lazo envejece la evidencia y va a `blind` |
| la ingesta muere | el lazo lo detecta por `is_finished()` y emite `stage_died`: `blind` con causa |
| el visor satura el enlace | bloquea a percepción, no al lazo; el control mantiene cadencia |
| control se atrasa | percepción sigue con la directiva anterior; su slot pisa lo viejo y lo cuenta |
| la cámara muere | el slot de keyframes queda vacío; el lazo emite salida igual, en `blind` |

La última fila es el invariante de `HANDOFF.md`, y **es la primera versión del
sistema que lo cumple**: *el programa corre a cadencia fija aunque el campo esté
muerto.*

### 1.4 De dónde viene esto

Hasta la Fase 3 (2026-08-11) todo corría en una sola task, con un `select!` entre
la ingesta y el reloj. Eso dejaba tres marcas que conviene reconocer al leer
reportes viejos o ramas anteriores:

- **`min 1ms` en la línea de ciclo** era el tick recuperado en ráfaga después de
  un bloqueo, no un ciclo rápido.
- **El invariante de cancelación-seguridad** —"todo future dentro del `select!`
  se compromete a `self` en el momento"— era obligatorio porque la rama perdedora
  se descartaba. Con la ingesta en su propia task ya no hay rama perdedora y la
  regla dejó de hacer falta; el test que la fija
  (`ingest::tests::staged_keyframe_survives_cancellation`) se conserva porque
  ahora protege el aborto en el apagado.
- **El bloqueo medido**: decode ~10 ms más inferencia ~211 ms daban 221 ms de
  media, el **110% de un periodo de scan**, superado en 47 de 51 keyframes. Ese
  número —el producto de la Fase 1— es lo que justificó la separación.

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

Dos scans pueden ejecutarse con 1 ms real de diferencia —una recuperación en
ráfaga— y el control creerá que pasaron 200 ms entre ellos. Sin embargo el reloj
virtual **no deriva**, porque se recupera exactamente un tick por periodo
transcurrido: el tick recuperado corresponde a tiempo real que sí pasó.

Desde la Fase 1 eso no depende del comportamiento por defecto de una primitiva
de tokio sino de aritmética explícita, en `ScanDeadline::arrive`
(`src/app/deadline.rs`):

```rust
let late = now.saturating_duration_since(self.next);
self.next += self.period;   // desde el vencimiento anterior, nunca desde `now`
```

> **Invariante crítico.** El próximo vencimiento se calcula **desde el
> vencimiento anterior**, y la grilla se ancla en `boot_instant`, el mismo
> origen que `ScanTimeline`. Las dos condiciones son necesarias:
>
> - Con `next = now + period`, cada atraso correría el reloj hacia adelante de
>   forma permanente y **todos los tiempos clínicos durarían más de lo que
>   dicen**.
> - Con el primer vencimiento en `origin + period` en vez de en `origin`, el
>   tiempo de control quedaría **un periodo entero por detrás** del de pared para
>   siempre, y todas las edades clínicas se subestimarían.
>
> Ninguna de las dos rompe un test ni produce un error: son fallas silenciosas
> con daño clínico. Fijadas por `deadline::tests::deadlines_do_not_drift_under_lateness`
> y `deadline::tests::control_time_tracks_wall_time_through_lateness`, que corre
> `ScanDeadline` y `ScanTimeline` en paralelo y verifica que no se separan.

El atraso que `arrive` devuelve no se descarta: se publica como distribución en
la línea `dline:` y en el JSONL. Ver §5.1.

---

## 3. Los puertos entre subsistemas

La documentacion activa en `docs/README.md` y `docs/subprojects/` declara los
subsistemas de Ingesta, Inferencia,
Control y Observabilidad. La calidad de las fronteras no es pareja.

| Frontera | Puerto | Estado |
|---|---|---|
| Percepción → Control | `ProcessImage` / `SceneSample` | declarado y respetado |
| Control → dominio | `SceneEvent` | declarado y respetado |
| Percepción → Observabilidad | `VizHandle` → `Slot<VizBatch>` | declarado y respetado |
| Percepción → Control (eventos) | `Cola<Vec<Event>>` | declarado y respetado |

### 3.1 La costura, y lo que quedó de ella

La frontera de observabilidad era la peor del sistema y **la separación de
etapas la resolvió por estructura, no por abstracción**.

Lo que había: un trait `PipelineObserver` honrado en un solo método, con los
demás sitios entrando por `FanoutObserver.viz` —campo `pub`— y un `viz_mut()`
que devolvía el tipo concreto. La visualización corría en el hilo del control y
podía frenarlo. Medido en 180 s con frames sin comprimir
(`workshop/scenarios/02-ingest-viz`, variante `a-raw-native`):

```
enlace de viz saturado
  └─ el flush bloquea el hilo del pipeline ............ 41 segundos
     └─ retina no se poletea, el socket RTP se llena
        └─ los errores RTP superan el umbral
           └─ 147 reconexiones RTSP
              └─ 24% de los keyframes perdidos
```

Lo que hay ahora: el `VizBridge` tiene **dueño único** —el hilo de percepción— y
el lazo de control no puede alcanzarlo. Los tres dibujos que produce control
(ocupancia, estado del FSM, cajas de entidades) viajan dentro de
`ControlDirective`, por el mismo slot que ya lleva la realimentación.

El trait desapareció, y esa es la parte que conviene entender antes de
reintroducirlo: al partir las etapas quedó con **un solo implementador y ningún
doble de test que lo usara**. Un trait que no vuelve imposible ningún error es un
módulo con pasos de más — el mismo criterio que ADR-028 aplica a los crates.

Y desde la Fase 2 el bridge **ni siquiera lo tiene percepción**: vive en un hilo
propio detrás de un `Slot<VizBatch>`, y lo que percepción sostiene es un
`VizHandle` que encola dibujos con sus argumentos ya en propiedad. La
contrapresión de rerun sigue existiendo —`re_chunk` no ofrece política de
descarte— pero ahora sólo puede bloquear al hilo que no tiene a nadie esperándolo.

Un lote es **un keyframe entero de dibujo** y se descarta entero: medio frame de
overlays sobre el frame siguiente sería peor que no dibujar nada. Los lotes
pisados se publican como `viz_pisados` (§5.1.2).

**Medido el 2026-08-12** sobre la variante `a-raw-native` —frames sin comprimir,
6,2 MB cada uno, con visor abierto—, que es la corrida que produjo la cadena de
la Fase 0:

| | Fase 0 | ahora |
|---|---|---|
| scan bloqueado | **41 s** | — |
| reconexiones RTSP | 147 | **0** |
| keyframes perdidos | 24% | **0** |
| `cycle` p95 | — | 200 ms (min 199) |
| atraso del lazo | — | p95 2,1 ms · **0 incumplidos** |
| `viz_pisados` | no existía | **sube: 2 → 5 → …** |

El contador sube y nada más se mueve. Ésa es la prueba: el enlace satura, se
descartan lotes de dibujo, y ninguna etapa del pipeline se entera.

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
> `metrics::tests::a_stalled_cycle_without_work_still_trips_the_budget`, y
confirmado en campo contra un scan real de 41 s.

### 5.1.1 Periodo y atraso son dos ejes, no uno

El presupuesto mide **periodo**: cuánto pasó entre dos scans. Esa magnitud **se
autocorrige** —un scan que arranca tarde empuja al siguiente, que arranca de
inmediato— así que el promedio se ve sano aunque el lazo esté incumpliendo.

Lo que no se autocorrige es el **atraso de vencimiento**: cuánto después de su
deadline arrancó cada scan. Es lo que un PLC llama incumplimiento, y se publica
aparte, en la línea `dline:` y en el evento JSONL `health`/`scan_deadline`:

```
dline: 26 deadlines in 5s | late min 0.4ms p50 1.1ms p95 1.6ms max 2.2ms | 0 missed (>5.0ms)
```

Tres decisiones de esa línea que no son obvias:

- **Se publica en µs, no en ms.** Un lazo sano vive por debajo del milisegundo;
  en ms enteros el reporte diría `0` tanto cuando cumple como cuando el
  instrumento está roto.
- **`missed` lleva tolerancia de 5 ms**, que es el piso medido del temporizador
  de tokio (~1,5–2,1 ms según el estado de la máquina) con margen. Con el umbral
  en `late > 0` el contador daba ~96% de incumplimientos **en el escenario de
  control**, y no distinguía un lazo sano de uno bloqueado 188 ms. La tolerancia
  gobierna sólo el contador; la distribución va sin recortar.
- **`p50` va junto a `p95`** porque el atraso es bimodal por construcción: los
  ciclos que no chocan con trabajo se quedan en el piso, y los que sí saltan a la
  latencia de la etapa que los bloqueó.

Compuerta que mantiene honesta la tolerancia: **el escenario 01 debe informar
`missed 0`**. Si vuelve a marcar incumplimientos sin que nada bloquee el lazo, el
piso se movió y la constante hay que volver a medirla, no subirla.

### 5.1.1.1 La edad de la evidencia: el único número clínico

Todo lo demás que se mide acá —periodo, atraso, latencia de inferencia,
descartes— es **salud del motor**: dice si la máquina está sana, no si la
decisión se tomó sobre algo actual. La pregunta que hace un revisor de incidente
es otra:

> Cuando el FSM dijo `bed_alert`, ¿de cuándo era lo que vio?

```
evid:  25 scans con evidencia in 5s | edad min 4ms p50 402ms p95 806ms max 812ms
```

Se mide contra el **reloj de control**, no contra el de pared: es la edad tal
como la percibió la decisión.

Dos cosas que hay que entender antes de leer esa línea:

- **El piso no es cero y no debería serlo.** Con keyframes a 1 Hz y un lazo a
  5 Hz, cuatro de cada cinco scans deciden sobre evidencia que ya tenían. Un p50
  cerca de medio intervalo de keyframe es lo sano.
- **Lo que hay que mirar es el `max` contra `health.data_stale_ms`.** Si se
  acerca, el sistema está decidiendo sobre evidencia que casi califica de
  obsoleta, y eso no lo delata ninguna de las otras líneas.

Sólo se registra cuando hay evidencia. Sin observaciones la edad vale
`u64::MAX`, y ese centinela dentro de una distribución la arruina para siempre;
la ausencia ya la cuenta `blind_cycles`. Fijado por
`metrics::tests::a_window_without_evidence_reports_zero_not_a_sentinel`.

### 5.1.2 Los bordes entre etapas se instrumentan

Cada `Slot` publica lo que pisó, en la línea de ingesta:

```
ingest: ... | kf_pisados:3, img_pisadas:1
```

`kf_pisados` significa que percepción no dio abasto con la cámara;
`img_pisadas`, que produjo dos evidencias entre dos scans. **Descartar es la
degradación correcta para una muestra; descartarla en silencio no lo es** —
un borde sin instrumentar es un borde sobre el que no se puede razonar cuando
algo va mal (ADR-034).

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

### 6.1 El dibujo de control viaja por la directiva — *provisorio*

Los tres dibujos que produce el lazo de control (ocupancia, estado del FSM, cajas
de entidades) viajan dentro de `ControlDirective` en vez de ir directo al hilo
del visor. Funciona y no cuesta nada, pero mezcla dos cosas en un mismo mensaje:
qué debe correr percepción, y qué debe dibujarse.

Es residuo de haber hecho la Fase 3 antes que la 2: cuando el `VizBridge` tenía
dueño único en percepción, era la única vía. Ahora que el visor tiene hilo
propio, control podría tener su propio `VizHandle` y la directiva volvería a ser
sólo `models` + `tracks`. No es urgente: la única consecuencia observable es que
el visor dibuja el estado de control con hasta un periodo de retraso.

### 6.2 El dedupe de keyframes no vence — *latente*

`poll_freshest_keyframe` descarta un keyframe cuyo digest coincide con el
anterior procesado, y esa comparación no tiene ventana. Ante una escena
completamente inmóvil, un encoder que emita IDR byte-idénticos suprimiría
indefinidamente, y **la propia supresión dispararía `data_stale`**: aguas arriba
no habría forma de distinguir "la escena no cambió" de "el stream murió".

### 6.2.1 ~~El `Mutex<MetricsEngine>` en el camino del lazo~~ — *descartada*

Percepción y control comparten el motor de métricas por `Mutex`, y eso es un
candado que el lazo toma: si percepción fuera desalojada sosteniéndolo, el lazo
esperaría un quantum del scheduler.

**Medido el 2026-08-12**, escenario 01, `late max`:

```
antes del corte    1,5 – 1,6 ms   (1,96 – 2,12 en una corrida previa)
después            1,3 – 1,4 ms
```

Bajó. El candado no está en el camino crítico, y el lazo quedó más puntual que
antes porque hace menos trabajo por tick. No hay nada que refactorizar.

La razón de dejarlo anotado en vez de arreglarlo preventivamente: contadores por
etapa fusionados al reporte son más código y más superficie de error para
resolver un problema que la medición dice que no existe.

### 6.3 ~~El presupuesto cuenta pero no actúa~~ — *sin planta que controlar*

`cycle_overruns` y `scan_deadlines_missed` cuentan y no actúan, y eso quedó bien
así. La deuda suponía un lazo que se bloquea por cosas que puede soltar; después
de separar las etapas no queda ninguna: el visor no puede frenar a nadie, el lazo
no espera a la inferencia, y bajar la cadencia cambiaría los tiempos clínicos que
el sistema existe para respetar.

Medido: 1,3–2,1 ms de atraso contra un periodo de 200 ms, cero incumplimientos,
en el peor escenario disponible. Un controlador de degradación acá sería un lazo
de control sin planta.

**Lo que sí queda es un residuo chico de supervisión**, y conviene no
sobredimensionarlo: las etapas *informan* que murieron (`stage_died`,
`perception_panic`) y nadie las reinicia. En la práctica son difíciles de matar
—los pánicos de percepción se atrapan y la etapa sigue; retina reconecta sola—
así que la superficie real es un pánico fuera del `catch_unwind` del cuerpo del
hilo. Vale anotarlo, no vale una fase.

### 6.4 ~~Los modelos se cargan aunque la inferencia esté apagada~~ — *cerrada*

Cerrada el 2026-08-11. Con `pipeline.infer = false` el bootstrap construye el
motor desde un catálogo vacío y no crea ninguna sesión ONNX. La validación del
catálogo y del blueprint sigue corriendo en la etapa 2, así que un catálogo roto
sigue siendo falla de arranque aunque la inferencia esté apagada.

---

## 7. Cómo se verifica

`workshop/` contiene escenarios que encienden **una capa por vez**, cada uno con
criterios de aceptación escritos antes de correr. Con el pipeline completo
prendido hay seis sospechosos ante cualquier anomalía; con la escalera, hay uno.

| Escenario | Capa que agrega |
|---|---|
| `01-ingest-only` | RTSP → decode → JSONL |
| `02-ingest-viz` | bridge de Rerun (`a-raw-native` / `b-jpeg-native`) |
| `03-ingest-infer` | inferencia — el escenario que midió el bloqueo del lazo |
| `04-infer-track` | tracking y cascada — el primero que ejercita la realimentación |
| `05-clinical` | zonas, FSM y presencia — la pila completa, blueprint de producción |

Los peldaños 01 y 03 tienen blueprint propio porque aíslan una capa. **04 y 05
usan los blueprints de producción** (`detect-face` y `detect-room-face`): a esa
altura lo que hay que validar es lo que se despliega. Quedan sin cobertura
`detect-face-pose-seg` y `detect-room-raw`.

Regla de invocación: **siempre `cargo run`, nunca una ruta fija al binario.**
`target-dir` puede estar redirigido por configuración global de cargo, en cuyo
caso un `./target/` viejo en el repo sigue siendo ejecutable y no se actualiza
jamas. Ver `workshop/ONBOARDING.md` y `workshop/MANUAL.md` para *Build and Run*.
