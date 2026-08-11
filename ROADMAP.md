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

**Hoy el sistema no lo cumple.** No sólo el campo puede frenar el programa: una
visualización de depuración también. Medido: 41 segundos de scan bloqueado, 147
reconexiones RTSP inducidas, 24% de keyframes perdidos —- por tener un visor
abierto. Corregir eso es el Track A y es la prioridad uno.

---

## Track A — Arquitectura de ejecución

Aislar el lazo de control. Decidido en
[ADR-033](docs/adrs/033-isolated-control-loop.md),
[ADR-034](docs/adrs/034-slots-and-queues.md) y
[ADR-035](docs/adrs/035-observability-port.md).

| Fase | Entrega | Riesgo | Estado |
|---|---|---|---|
| **0** | Línea base verde y sin knobs muertos | bajo | ✅ **cerrada** 2026-08-11 |
| **1** | El lazo mide y declara su propio atraso | bajo | 📋 [planificada](docs/sprints/fase-1-forma-del-lazo.md) |
| **2** | La visualización no puede frenar el control | medio | pendiente |
| **3** | La cadencia se cumple de verdad | **alto** | pendiente |
| **4** | La clase de bug de cancelación desaparece | medio | pendiente |
| **5** | El sistema declara su degradación | medio | pendiente |

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

### Fase 1 — La forma del lazo 📋

[Plan detallado](docs/sprints/fase-1-forma-del-lazo.md).

Vencimiento explícito en vez de `interval.tick()`, y el atraso del scan como
medición publicada. No mejora la cadencia: produce el número que justifica las
fases siguientes.

### Fase 2 — El puerto de observabilidad

`VizSink` como trait, `VizRelay` con hilo propio y `Slot<T>` que descarta en vez
de bloquear, `FanoutObserver.viz` a `Box<dyn VizSink>`. Los sitios de llamada no
se tocan.

Compuerta: la variante `a-raw-native` del escenario 02 —- hoy degradada— pasa a
ser inofensiva para el lazo, con el contador de frames pisados subiendo.

### Fase 3 — Percepción a su hilo

Decode e inferencia salen del lazo. Es la fase que hace que el sistema cumpla su
propio invariante. También la de mayor riesgo: es donde el `App` de 19 campos se
parte. Mitigación: los campos **ya están agrupados por subsistema**.

### Fase 4 — Ingesta a su hilo

Desaparece el `select!` del camino caliente, y con él la clase de bug de
cancelación —- no un bug, la clase entera.

### Fase 5 — El reloj y la degradación

Reloj de control derivado del reloj monotónico (se borra la dependencia
silenciosa de `MissedTickBehavior::Burst`), y el presupuesto pasa de contar a
actuar.

---

## Track B — Deuda conocida

Independiente del Track A. Ninguna bloquea nada, todas están documentadas.

| # | Deuda | Dónde | Cuándo |
|---|---|---|---|
| B1 | Los modelos ONNX se cargan aunque `pipeline.infer = false` | `ARCHITECTURE.md` §6.4 | cae natural en Fase 3 |
| B2 | `README.md` describe un baseline de 5 ramas y `track = false` que ya no es el actual | — | cualquier momento |
| B3 | Comentarios de código en inglés introducidos en la Fase 0, contra la regla de `HANDOFF.md` | `src/viz/`, `src/ingest.rs` | cualquier momento |
| B4 | `workshop/scenarios/home-1/` es copia byte a byte del escenario 02 | — | decidir estructura |

**Sobre B4.** Una copia idéntica se desincroniza sola —- ya hubo que actualizarla
a mano en la Fase 0. Si la intención es tener escenarios por despliegue (una
cámara por cuarto), lo que corresponde no es duplicar el escenario sino separar
*qué se prueba* de *contra qué se prueba*: el escenario define el test, un
overlay define la cámara. Requiere decisión.

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
| `03-ingest-infer` | inferencia | Fase 1 |

Regla permanente de invocación: **siempre `cargo run`, nunca una ruta fija al
binario.** `target-dir` puede estar redirigido por configuración global de cargo,
y entonces un `./target/` viejo sigue siendo ejecutable sin actualizarse jamás
(`docs/wiki/1.1-getting-started.md` → *Build and Run*).
