# mana-lite — Roadmap

Estado del proyecto y hacia dónde va. Actualizado 2026-08-11.

**Orientación:** [BIGPICTURE.md](BIGPICTURE.md) (qué es y por qué) ·
[ARCHITECTURE.md](ARCHITECTURE.md) (cómo está construido) ·
[HANDOFF.md](HANDOFF.md) (retomar el trabajo) · `docs/adrs/` (decisiones)

---

## El norte

`HANDOFF.md` define el sistema en una frase que conviene tener presente al
priorizar cualquier cosa:

> **mana-lite es un PLC cuyo dispositivo de campo es una cámara.** El programa
> corre a cadencia fija y **tiene que emitir salida en cada tick aunque el campo
> esté muerto.**

Ese invariante es el criterio de prioridad de todo el roadmap. Lo que lo protege
va primero; lo que lo pone en riesgo se corrige antes que cualquier feature.

**Desde el 2026-08-11 el sistema lo cumple.** Las tres etapas —ingesta,
percepción y control— corren con dueños de ejecución distintos y bordes que no
bloquean, así que el lazo mantiene cadencia aunque la inferencia tarde más que un
periodo, aunque el visor sature el enlace o aunque percepción entre en pánico.

Medido en el escenario 03, antes y después:

| | antes | después |
|---|---|---|
| `cycle` p95 | 305–348 ms | **200–201 ms** |
| atraso del lazo, p95 | 101–154 ms | **1,4–3,3 ms** |
| vencimientos incumplidos | 5 por ventana | **0** |
| latencia de inferencia | 194–217 ms | 194–217 ms (igual) |

La inferencia no se optimizó: dejó de cobrárselo al lazo.

---

## Track A — Arquitectura de ejecución

Aislar el lazo de control. Decidido en
[ADR-033](docs/adrs/033-isolated-control-loop.md),
[ADR-034](docs/adrs/034-slots-and-queues.md) y
[ADR-035](docs/adrs/035-observability-port.md).

| Fase | Entrega | Riesgo | Estado |
|---|---|---|---|
| **0** | Línea base verde y sin knobs muertos | bajo | ✅ **cerrada** 2026-08-11 |
| **1** | El lazo mide y declara su propio atraso | bajo | ✅ **cerrada** 2026-08-11 |
| **2** | La visualización no puede frenar el control | medio | ✅ **cerrada y verificada** 2026-08-12 |
| **3** | La cadencia se cumple de verdad | **alto** | ✅ **cerrada** 2026-08-11 |
| **4** | La clase de bug de cancelación desaparece | medio | ✅ **cerrada** 2026-08-11 |
| **5** | El sistema declara su degradación | medio | ✅ **superada por la arquitectura** |

### Reglas del track

**Cada fase entrega valor sola.** Si el plan se detiene en cualquier punto, lo
entregado sigue siendo una mejora y el sistema queda consistente.

**Cada fase tiene una compuerta medible** —- un escenario del `workshop/` con
criterio escrito antes de correr. No se avanza con la anterior en rojo.

**Ninguna fase cambia el dominio.** `ProcessImage`, `SceneEvent`, el catálogo de
señales y la política clínica quedan intactos. Esto es plomería.

**El orden es por capacidad de medir, no por dolor.** Si fuera por dolor, la
Fase 2 iría primera. Cada fase deja instalado el instrumento con el que se
verifica la siguiente.

### Fase 0 — Sanear la línea base ✅

Cerrada el 2026-08-11. [Plan](docs/sprints/fase-0-sanear-base.md).

Lo entregado:

- Ingesta cancelación-segura (`select!` ya no descarta keyframes drenados).
- El bridge de Rerun distingue contrapresión de desconexión, y `connected`
  significa que hay viewer.
- El presupuesto de ciclo mide de verdad —- estaba estructuralmente muerto.
- JPEG como única palanca de enlace; `image_max_res` borrado antes de entrar a
  la historia.
- Vencimiento del dedupe de keyframes, con validación cruzada contra
  `data_stale_ms`.
- `seen` deja de enmascararse con `processed`: el par vuelve a conservarse.
- Banco de escenarios `workshop/` con compuertas automatizadas.

Compuertas al cierre, corridas de 180 s: escenario 01 en 185/185 keyframes, sin
`stale`/`blind`, sin reconexiones; escenario 02 `b-jpeg-native` en 179/179 con
una sola conexión.

### Fase 1 — La forma del lazo ✅

Cerrada el 2026-08-11. [Plan y cierre](docs/sprints/fase-1-forma-del-lazo.md).

Vencimiento explícito en vez de `interval.tick()`, y el atraso del scan como
medición publicada. No mejora la cadencia: produce el número que justifica las
fases siguientes. **Ese número:**

> El bloqueo del lazo por keyframe es de **221 ms de media contra un periodo de
> 200 ms —el 110%—, y lo supera en 47 de 51 keyframes.** La inferencia no
> retrasa un scan: se come más de un periodo entero.

Con eso, el argumento de la Fase 3 deja de ser teórico. También quedó medido, de
yapa, el costo de bloqueo del bridge de visualización —`late max` 26,7 ms con
JPEG—, que es el antes/después de la Fase 2.

Lo entregado: `ScanDeadline` anclado en `boot_instant` con test de no-deriva
contra `ScanTimeline`; línea `dline:` con la distribución en µs; evento JSONL
`health`/`scan_deadline` por ventana, independiente de `metrics_event`;
escenario `03-ingest-infer`.

### Fase 2 — El visor en su propio hilo ✅

El visor tiene hilo propio y un `Slot<VizBatch>` que **descarta en vez de
bloquear**. Un enlace saturado ya no puede frenar a ninguna etapa del pipeline:
ni al lazo de control, ni a percepción.

Se construyó distinto de lo planeado. La ADR pedía un trait `VizSink` de ~20
métodos para poder sustituir la implementación; eso resolvía un problema de
sustitución que nadie tenía. Lo que hay es un `VizHandle` que espeja la
superficie del bridge y encola cada dibujo con sus argumentos ya en propiedad —
el costo de prestado→propio se paga una vez, en un archivo, que era el punto de
diseño que la propia ADR declaraba. Sin trait: hay un solo dueño y un solo
implementador (ADR-028).

Un lote es un keyframe entero de dibujo y se descarta entero: medio frame de
overlays sobre el frame siguiente sería peor que no dibujar nada. Los lotes
pisados se publican como `viz_pisados`.

De paso se borró el trait `PipelineObserver`: al separar las etapas quedó con un
solo implementador y ningún doble de test.

**Verificado el 2026-08-12** sobre `a-raw-native`, la variante que en la Fase 0
produjo 41 s de scan bloqueado, 147 reconexiones RTSP y 24% de keyframes
perdidos. Ahora: cadencia de 200 ms clavada, cero incumplimientos, cero
reconexiones, cero keyframes perdidos — y `viz_pisados` subiendo.

El contador sube y nada más se mueve. Frames sin comprimir con un visor abierto
dejaron de poder tocar el pipeline.

### Fase 3 — Percepción a su hilo ✅

Decode e inferencia salieron del lazo. `App` pasó de 19 campos a 11 y dejó de ser
el pipeline: ahora es el lazo de control y nada más.

Lo que el plan no preveía y el código sí dijo: **es un lazo cerrado, no un
pipeline**. El FSM decide qué modelos corren y el tracker dónde recortar, así que
percepción es el actuador de un lazo. Por eso hay dos `Slot` en direcciones
opuestas — un solo slot no compilaba, y un candado sobre el estado de control
habría reintroducido el bloqueo con otro nombre.

### Fase 4 — Ingesta a su task ✅

Se fue el `select!` del camino caliente, y con él la clase de bug de cancelación
—no un bug, la clase entera. El lazo de control es ahora un temporizador puro:
su `select!` sólo elige entre el vencimiento y las señales de apagado.

Dos efectos de arrastre: `App` dejó de ser genérico sobre el reader —el genérico
sobrevivía a su motivo— y `FrameReader` pasó a declarar `Send` explícito, que era
lo que la advertencia del compilador venía pidiendo hace rato.

### Fase 5 — El reloj y la degradación ✅ *superada*

**El reloj, hecho.** Ya no depende del comportamiento por defecto de
`MissedTickBehavior::Burst`: `ScanDeadline` hace la aritmética explícita, anclada
en el mismo origen que `ScanTimeline`, con tests que fijan las dos formas de
deriva silenciosa.

**La degradación: la premisa venció.** El plan decía "el presupuesto pasa de
contar a actuar". Esa fase suponía un lazo que se bloquea por cosas que puede
soltar. Después de las fases 2, 3 y 4 **no queda nada que soltar**, y las
mediciones lo confirman: 1,3–2,1 ms de atraso contra un periodo de 200 ms, cero
incumplimientos, en el peor escenario que tenemos.

Cada acción candidata, una por una:

| Acción | Por qué ya no aplica |
|---|---|
| apagar viz | el visor no puede frenar a nadie desde la Fase 2 |
| saltear inferencia | el lazo no la espera desde la Fase 3 |
| bajar la cadencia | cambiaría los tiempos clínicos, que es justo lo que protegemos |
| escalar un aviso | eso es **declarar**, no actuar — y ya existe |

Construir el mecanismo igual sería construir un control sin planta. Lo que la
fase quería —que el sistema declare su degradación— **ya lo hace**:
`scan_deadlines_missed`, `stage_died`, `perception_panic`, `blind`, y los
contadores de descarte de cada borde.

**Lo único que queda de esta fase es una pregunta de configuración, no de
código:** `data_stale_ms = 10_000` y `stale_warn_ms = 5_000` son números
clínicos ya elegidos que gobiernan cuándo el sistema se declara ciego. Nadie los
revisó contra un caso real. Eso es una revisión de umbrales, no una fase.

---

## Track B — Deuda conocida

Independiente del Track A. Ninguna bloquea nada, todas están documentadas.

| # | Deuda | Dónde | Cuándo |
|---|---|---|---|
| ~~B1~~ | ~~Los modelos ONNX se cargan aunque `pipeline.infer = false`~~ | — | ✅ cerrada 2026-08-11 |
| ~~B2~~ | ~~`README.md` describe un baseline de 5 ramas y `track = false`~~ | — | ✅ cerrada 2026-08-11 |
| ~~B3~~ | ~~Comentarios de código en inglés de la Fase 0~~ | — | ✅ cerrada 2026-08-11 |
| ~~B4~~ | ~~`workshop/scenarios/home-1/` es copia byte a byte del escenario 02~~ | — | ✅ borrada 2026-08-12 |
| ~~B7~~ | ~~El `Mutex<MetricsEngine>` en el camino del lazo~~ | — | ✅ descartada 2026-08-12: el piso bajó de 1,5-1,6 ms a 1,3-1,4 ms |
| B5 | La wiki generada (21 archivos, commit `ad24740d`) describe el super loop y tipos borrados | `docs/wiki/` | **regenerar**, no editar |
| B6 | Hay dos documentos de arquitectura (`ARCHITECTURE.md` de ejecución, `docs/ARCHITECTURE.md` de workspace) y el README ahora los distingue, pero conviene decidir si se funden | — | cualquier momento |

**Sobre B5.** No es una corrección de texto: la wiki es **generada** —permalinks
de GitHub clavados en un commit— y editarla a mano crea un segundo relato que
compite con el primero y se pierde en la próxima regeneración. Lo que
corresponde es correr el generador contra `HEAD`. Mientras tanto lleva un aviso
en `docs/wiki/1-overview.md` que dice qué describe que ya no existe y a dónde ir.

**Cerradas el 2026-08-11.** B1 era un condicional: con `pipeline.infer = false`
el bootstrap ya no construye sesiones ONNX, y la validación del catálogo sigue
corriendo en la etapa 2, así que no se debilitó ninguna verificación de arranque.
B2 tenía cinco afirmaciones falsas —`track = false`, el baseline de 5 ramas, un
link roto a `docs/ROADMAP.md`, "ADRs 001-024" con 35 en el árbol, y el tracker
descrito como prototipo sin Kalman cuando ya lo tiene.

**Sobre B4, resuelta.** No era una decisión de estructura: `home-1/02-ingest-viz/
mana.toml` era **byte a byte idéntico** al original —ni siquiera cambiaba la
cámara, que es lo único que justificaría un escenario por despliegue— y nada del
repo lo referenciaba. Un directorio que no expresa ninguna diferencia no es un
escenario por despliegue: es una copia sin mantener que deriva sola. Ya lo había
hecho dos veces: en la Fase 0 y otra vez en la Fase 2, cuando el arreglo del
`grep` de `dline:` entró en un `run-variant.sh` y no en el otro.

Borrada. Está en la historia de git si hace falta.

**Cuando aparezca un segundo despliegue**, la forma correcta ya existe en el
sistema de configuración: un overlay que traiga sólo `[source]`. El escenario
define *qué se prueba*, el overlay *contra qué*. Duplicar el archivo entero
vuelve a poner las dos cosas en el mismo lugar, que es lo que produjo esto.

---

## Track C — Producto

**Sin planificar. Requiere tu dirección.**

Lo que se sabe del estado actual, para que sirva de punto de partida:

- El caso clínico que corre hoy es prevención de caídas de cama:
  `idle → watching → bed_approaching → bed_alert` (`HANDOFF.md`).
- El blueprint activo es `detect-room-face`: cardinalidad de sala más recorte
  dinámico de cara, con FSM de ciclo de vida.
- El catálogo de señales v1 tiene nueve etiquetas y ya es un contrato
  ([ADR-032](docs/adrs/032-scene-signals-as-contract.md)): cambiar cuándo suena
  una alerta es editar un TOML, no un release.

Ese último punto es el que abre el juego a producto sin tocar Rust. Qué
escenarios clínicos siguen, qué umbrales por servicio, qué integraciones —- eso
es decisión tuya, no mía.

---

## Cómo se verifica cualquier cosa

`workshop/` enciende **una capa por vez**, cada escenario con criterios escritos
antes de correr.

| Escenario | Capa que agrega | Estado |
|---|---|---|
| `01-ingest-only` | RTSP → decode → JSONL | ✅ verde |
| `02-ingest-viz` | bridge de Rerun | ✅ verde en `b-jpeg-native` |
| `03-ingest-infer` | inferencia | ✅ verde — el escenario que midió el bloqueo y verificó su desaparición |

**Sin correr todavía:** la variante `a-raw-native` del escenario 02. Es la que
quedó degradada en la Fase 0 (41 s de bloqueo, 147 reconexiones) y la única que
puede cuantificar cuánto sobra de la Fase 2. Es la próxima corrida que vale.

Regla permanente de invocación: **siempre `cargo run`, nunca una ruta fija al
binario.** `target-dir` puede estar redirigido por configuración global de cargo,
y entonces un `./target/` viejo sigue siendo ejecutable sin actualizarse jamás
(`docs/wiki/1.1-getting-started.md` → *Build and Run*).
