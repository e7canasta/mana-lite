# Big Picture — Arquitectura por Tiers

*Baseline: 2026-08-09. Normativo: [ADR-027](../adrs/027-tier-architecture.md) … [ADR-031](../adrs/031-scene-signal-table.md).*

---

## 1. Qué es este sistema

Mana Lite no es una aplicación de visión que además tiene lógica. Es un **PLC
cuyo dispositivo de campo resulta ser una cámara**.

Corre a **dos tasas**:

- **El campo** —RTSP, decode, ONNX— va a la velocidad que puede, con latencia
  variable y fallando seguido.
- **El programa** —tracker, presencia, ocupación, zonas, FSM, health— corre a
  **cadencia fija** y tiene que emitir salida en cada tick aunque el campo esté
  muerto.

Entre ambos hay un solo objeto: la **imagen de proceso** (`ProcessImage`),
congelada, fechada y con edad explícita. Todo el resto del diseño se deriva de
ahí.

### Por qué no DDD

Los intentos previos de ordenar esto usaron vocabulario de Domain-Driven Design
(bounded contexts, agregados, domain events). Es el registro equivocado: DDD
parte por sustantivos de negocio, y aquí no hay negocio — hay un lazo de control
con dos relojes. La partición correcta es por **clase de determinismo**, como en
cualquier middleware de control.

---

## 2. La única pregunta que ubica cualquier cosa

> **Si la entrada nunca vuelve a llegar, ¿este componente tiene que seguir
> produciendo salida correcta en cada tick?**
>
> Sí → capa de programa (T2). No → capa de campo (T1).

Esta pregunta reemplaza al juicio arquitectónico caso por caso. No es teórica:
el FSM ya tiene un estado `blind` que se recupera vía guard `data_fresh`, y
`Health` ya emite heartbeat de recuperación. El invariante ya está
implementado; lo que falta es convertirlo en regla de ubicación que el
compilador sostenga.

---

## 3. Los cuatro tiers

| Tier | Rol | Tasa | Semántica de fallo | Crates |
|---|---|---|---|---|
| **T0 · álgebra** | Tipos y matemática pura. Sin estado, sin reloj, sin tasa. | — | no falla | `mana-id`, `mana-geometry` |
| **T1 · campo** | Sensado y E/S: RTSP, decode, ONNX. Latencia no acotada. | variable | **fallar es normal** | `mana-media`, `mana-perception` |
| **T2 · programa** | Tracker, presencia, ocupación, zonas, FSM, health. | **fija** | **no puede fallar: tickea siempre** | `mana-control` |
| **T3 · reporte** | JSONL, métricas, Rerun, adaptadores. | best-effort | falla en silencio; nunca bloquea el tick | en el binario |

```
┌─ T3 · reporte ────────────────────────────────────────────┐
│  JSONL · métricas · Rerun · adaptadores                    │
│  best-effort — nunca bloquea el tick                       │
├─ T2 · programa ───────────────────────────────────────────┤
│  mana-control                                              │
│  track · fsm · presence · occupancy · zones · health       │
│  CADENCIA FIJA — tickea siempre — reloj inyectado          │
╞═══════════ ProcessImage ═══════════════════════════════════╡
│  congelada · fechada · con edad — pertenece a T2           │
├─ T1 · campo ──────────────────────────────────────────────┤
│  mana-perception · mana-media                              │
│  detection · cascade · depth · decode · h264               │
│  tasa variable — FALLAR ES NORMAL                          │
├─ T0 · álgebra ────────────────────────────────────────────┤
│  mana-id · mana-geometry                                   │
│  DomStr · domain_id! · bbox · iou · compact_mask           │
└────────────────────────────────────────────────────────────┘
```

### Dos consecuencias que no son negociables

**T2 no depende de T1.** No por estética: porque T2 debe seguir corriendo cuando
T1 está muerto. Un `mana-control` que no compila sin `mana-perception` es un lazo
de control que no puede sobrevivir a una cámara caída.

**`ProcessImage` pertenece a T2, no a T1.** Un PLC es dueño de su imagen de
proceso; los dispositivos de campo no saben que existe. El adaptador del runtime
(T3) la construye a partir de la salida de percepción; percepción nunca nombra el
tipo.

---

## 4. La matriz de dependencias es la arquitectura

En Rust, `pub(crate)` ya provee privacidad de módulo. Un crate separado compra
**una sola cosa** que los módulos no dan: un `Cargo.toml` que hace que una
dependencia prohibida **no compile**.

> Un crate se justifica si y solo si existe una dependencia que queremos volver
> imposible. Si la respuesta es "ninguna", no es un crate: es un módulo.

| | `mana-id` | `geometry` | `media` | `perception` | `control` |
|---|:-:|:-:|:-:|:-:|:-:|
| **mana-geometry** | — | — | — | — | — |
| **mana-media** | — | — | — | — | — |
| **mana-perception** | ✔ | ✔ | ✔ | — | **⛔** |
| **mana-control** | ✔ | ✔ | **⛔** | **⛔** | — |
| **mana-lite** *(bin)* | ✔ | ✔ | ✔ | ✔ | ✔ |

Las tres celdas ⛔ son el contenido normativo. `control ⛔ media` expresa que el
lazo de control nunca toca un frame, solo observaciones ya adaptadas.

**Esta matriz se escribe en los `Cargo.toml`, no en documentación.** No se añade
un lint ni un script de CI: el linker es el que la aplica. Un revisor externo lee
la arquitectura completa en cinco archivos de manifiesto.

### Estructura resultante: 5 libs + 1 bin

```
mana-id/          T0  DomStr + macro domain_id!
mana-geometry/    T0  bbox, iou, polygon, compact_mask, transform

mana-media/       T1  PixelFormat, RawFrame, decoder, buffer_pool, h264
mana-perception/  T1  detection, cascade, depth_map, backend ONNX
mana-control/     T2  track, fsm, presence, occupancy, zones, health, kalman

mana-lite/ (bin)  T3  app, config, logger, metrics, viz, ingest, adaptadores
```

### Crates que se disuelven

| Crate | Uso real medido (2026-08-09) | Destino |
|---|---|---|
| `mana-types` | `DetectionV1`, `SceneEntityV1`, `ZoneV1`: **0 usos**. `DetectionBatchV1`, `SceneMsgV1`, `RoiCommandV1`: solo desde funciones de `mana-viz` que nadie llama. | `PixelFormat` + `RawFrameV1` → `mana-media`. Resto borrado. |
| `mana-viz` | 8 funciones públicas, **2 usadas** (`boxes2d_from_xyxy`, `FrameSize`). | → `src/viz/` |
| `mana-rtsp` | 64 líneas de helper H.264, un consumidor. | → `mana-media` |

Los tipos `*V1` son contrato IPC de Full Mana OS (iceoryx2), aspiracional aquí.
Su lugar es el repositorio de Full Mana OS. Acá se borran; git los recuerda.

---

## 5. El invariante que ningún crate puede hacer cumplir

`Instant::now()` está en la libstd — no hay `Cargo.toml` que lo prohíba. Hoy hay
puertas al reloj de pared dentro de T2:

- `ScanInstant::now()` — `core/mana-control/src/scan.rs:14`
- `Health::new()`, `Health::touch()`, `Health::evaluate()` — `health.rs:33,48,65`

Un solo uso en producción rompe la reproducibilidad del lazo: el mismo input deja
de producir la misma secuencia de estados.

Se cierra **con un tipo, no con una regla**: `ScanInstant` construible únicamente
desde `ScanTimeline`, que ya existe y avanza en múltiplos exactos de
`scan_period_ms`. Una regla en un documento no sobrevive a la rotación de gente;
un constructor privado sí. Ver [ADR-029](../adrs/029-injected-clock.md).

Verificación, sin herramienta nueva:

```sh
! grep -rn 'Instant::now\|SystemTime::now\|Utc::now' \
    --include='*.rs' core/mana-control/src \
  | grep -v 'cfg(test)'
```

---

## 6. Estado real, medido

La extracción de `mana-control` y `mana-perception` cerró en `91af4e8`. El
Sprint 0 residual reconectó la evidencia de profundidad al lazo (el adaptador
había quedado llamando `reset_depth` en lugar de `set_depth`) y desforkeó
`DomStr`.

| Área | Estado | Evidencia |
|---|---|---|
| Contrato PLC de `scan()` | **sólido** | `scan(state, image, now) -> Vec<SceneEvent>` |
| Compilar-en-boot del FSM | **sólido** | `FsmProgram::compile() -> Result<_, Vec<String>>` |
| Imagen de proceso | **existe** | `core/mana-control/src/lib.rs:92` |
| Compilación del workspace | **verde** | 0 errores (`cargo check --workspace --all-targets`) |
| Evidencia depth → FSM | **reconectada** | `wire_depth_evidence` en `src/app/mod.rs`; test `depth_evidence_reaches_fsm` |
| Frontera T1 → T2 | **violada (path hack)** | `src/lib.rs` `#[path]` a `cascade.rs`; resuelve `track`/`kalman` vía re-exports |
| Frontera T2 → T3 | **limpia** | `grep -rn 'logger' core/mana-control/src` → 0 |
| Reloj inyectado en T2 | **violado** | `ScanInstant::now()`, `Health::{new,touch,evaluate}` — marcados `FIXME(ADR-029)` |
| Vocabulario de dominio | **mecanismo compartido** | `DomStr` + `domain_id!` en `mana-control`; `ModelId`/`ClassName` en el binario |
| Legibilidad del lazo | **crítica** | `scan()` = línea 28 de ~3547 caracteres en un archivo de 31 líneas |
| `src/lib.rs` | **1 hack restante** | el `#[path]` de cascade |
| Red de seguridad | **verde, incompleta** | suite pasa; ningún test tocaba depth end-to-end vía `App` antes del residual |

### Lo que descalifica antes que cualquier debate de crates

`core/mana-control/src/scan.rs` está **minificado**: `scan()` es una sola línea
de ~3547 caracteres, `ControlState` es otra. El archivo que implementa el lazo
de control es ilegible para quien no lo escribió. Ningún revisor externo —Linux
Foundation, Eclipse, o un colega nuevo— puede auditar el lazo clínico así.

Esto también invalida la métrica de líneas en ambas direcciones. Contando
producción vs. tests, los god files reales son cuatro, y `fsm/mod.rs` no es uno
de ellos:

| Archivo | total | **producción** | tests |
|---|---:|---:|---:|
| `src/viz/mod.rs` | 1539 | **1180** | 359 |
| `src/logger/serialize.rs` | 830 | **830** | 0 |
| `src/app/mod.rs` | 762 | **~740** | ~22 |
| `src/config/model_loader.rs` | 719 | **616** | 103 |
| `core/…/scan.rs` | 31 | **~30** | 0 |
| `core/…/fsm/mod.rs` | 1197 | **13** | 1184 |
| `src/logger/mod.rs` | 920 | **336** | 584 |

`scan.rs` tiene 31 líneas y 18 líneas > 120 caracteres en todo `core/` — la
ilegibilidad no es cantidad de líneas, es densidad.

---

## 7. Cómo crece el diseño

Stress-test contra cinco crecimientos plausibles.

| Crecimiento | Toca | Veredicto |
|---|---|---|
| Nuevo modelo o tarea (pose, action recognition) | `mana-perception` + adaptador | **sano** — T2 no se entera |
| Nuevo sink (MQTT, webhook, HL7) | solo T3 | **sano** — `scan()` devuelve eventos, no escribe |
| Nueva fuente de evidencia (térmica, audio) | nuevo crate T1 + campo en `ProcessImage` | **sano** — el gemelo crece, es su trabajo |
| **Nueva regla de escena** | 6 ediciones, 4 archivos, todas en `mana-control` | **⚠ costo lineal** |
| **Multi-cámara** | `ControlState` y `ScanTimeline` no parametrizados por lazo | **🔴 no cubierto** |

### El costo de una regla de escena

Que las 6 ediciones caigan en un solo crate confirma que las fronteras son
correctas. Pero `FsmGuard` y `ProgramGuard` tienen **18 variantes cada uno**,
sincronizadas a mano, y `FsmSceneContext` son 7 booleanos planos donde cada
predicado nuevo es un campo más para siempre. A 40 guards duele; a 80 es un
pasivo.

**Lo que no se hace nunca:** fusionar `FsmGuard` y `ProgramGuard` para ahorrar
tipeo. Esa duplicación aparente es la separación compilar-en-boot /
ejecutar-determinista de un PLC.

**La salida** está en [ADR-031](../adrs/031-scene-signal-table.md), en estado
*Proposed*: la imagen de proceso de un PLC no es un struct de booleanos con
nombre, es una **tabla de señales etiquetadas**. Eso lleva el costo de O(6
ediciones) a O(1-2). No se implementa hasta cerrar el Sprint 4, pero cambia una
decisión inmediata: cada booleano plano que se agregue mientras tanto es deuda a
migrar.

### Multi-cámara

`ControlState` es instancia única y el catálogo FSM es global. N cámaras = N
`ControlState` + N `ProcessImage` con un programa compilado compartido. Los tiers
lo soportan conceptualmente —un PLC escala a N lazos— pero `ControlState` y
`ScanTimeline` no están parametrizados por lazo. **Si está en el roadmap a 12
meses, decidirlo durante el Sprint 2 es barato; después de 20 reglas más, no.**

---

## 8. Las tres reglas

1. **La pregunta de pertenencia.** *"¿Tiene que tickear con el campo muerto?"*
   Ubica cualquier archivo, tipo o función sin discutir.

2. **El `Cargo.toml` es el lint.** No escribir reglas de arquitectura en un
   documento que nadie lee: hacer que la dependencia prohibida no compile. La
   excepción son los relojes, que se prohíben con un tipo.

3. **Compilar en boot, ejecutar determinista.** Ya está en el FSM. Es el patrón a
   replicar cuando crezca la lógica de escena, no a abandonar.

---

## Referencias

- [ADR-027](../adrs/027-tier-architecture.md) — Tier Architecture by Determinism Class
- [ADR-028](../adrs/028-crate-boundaries.md) — Crate Boundaries as Compile-Time Enforcement
- [ADR-029](../adrs/029-injected-clock.md) — Injected Clock in the Program Layer
- [ADR-030](../adrs/030-shared-mechanism-owned-vocabulary.md) — Shared Mechanism, Owned Vocabulary
- [ADR-031](../adrs/031-scene-signal-table.md) — Scene Signal Table *(Proposed)*
- [ADR-001](../adrs/001-single-binary.md), [ADR-003](../adrs/003-plc-superloop.md) — la base que este trabajo hace cumplir
- [2-sprints.md](2-sprints.md) — el plan de ejecución
