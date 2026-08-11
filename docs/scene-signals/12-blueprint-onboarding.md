# Manual Operativo de Blueprints

**ID:** MANA-BP-OPS-001
**Version:** 1.0
**Estado:** operativo despues de la Etapa D
**Audiencia:** Operaciones, Integracion, ML y Clinical/Protocol
**Estilo:** runbook tipo SAP: roles, datos maestros, escenarios, procedimiento,
evidencia y rollback

## 1. Objetivo

Este manual explica como seleccionar, configurar, validar y operar un blueprint
de Mana Lite ahora que el motor de señales y el gemelo visible estan activos.

El resultado esperado de una operacion es doble:

- La FSM conserva la decision clinica existente sin cambiar thresholds,
  histéresis, dwell, prioridades, zonas, Health ni profundidad.
- El JSONL permite reconstruir el ciclo mediante `scene_signals`, sus nueve tags,
  sus ausencias y el `ControlStamp` que lo correlaciona con la FSM.

El blueprint selecciona modelos y gates de inferencia. La politica clinica vive
en `mana.toml`, `zones.toml` y el `fsm.toml` correspondiente; no se debe
esconder una decision clinica dentro de un gate de inferencia.

## 2. Modelo Operativo

### 2.1 Flujo de un ciclo

```text
RTSP/frame
   -> inferencia y consolidacion T1
   -> ProcessImage
   -> scan() T2
        -> SignalTable nueva
        -> SceneSignalsSnapshot completo
        -> SceneEvent::SceneSignals
        -> guards y transiciones FSM
   -> batch de SceneEvent
   -> mapper T3
   -> Event::SceneSignals + eventos legacy
   -> JSONL best-effort
```

`mana-control` no serializa, no escribe archivos y no conoce sinks. Si el logger
falla, el tick de control y su decision no deben bloquearse ni cambiar.

### 2.2 Datos maestros

| Objeto | Archivo principal | Responsable funcional | Contiene |
|---|---|---|---|
| Seleccion de perfil | `config/mana.toml` | Operaciones | `blueprint_file`, pipeline y rutas |
| Catalogo de modelos | `config/models.toml` | ML/Integracion | ONNX, confianza, NMS y tareas |
| Overlay de blueprint | `config/blueprints/<name>/models.toml` | Integracion/ML | tuning local reproducible |
| Grafo de inferencia | `config/blueprints/<name>/blueprint.toml` | Integracion | primary, children y gates |
| Zonas semanticas | `config/zones.toml` | Clinical/Integracion | cama, puerta, dwell y histéresis |
| Programa FSM | `config/blueprints/<name>/fsm.toml` | Clinical/Protocol | estados, roles, guards y dwell |
| Politica de tiempo | `[presence]`, `[tracking]`, `[health]` | Clinical/Operaciones | timers monotónicos y salud |
| Salida forense | `config/metrics*.toml` | Operaciones | eventos JSONL y nivel |
| Presentacion | `config/viz.toml`, `config/rerun.toml` | Operaciones | Rerun, panels y series |

## 3. Roles Y Segregacion

| Rol | Puede cambiar | Debe entregar antes de promover |
|---|---|---|
| Operaciones | `blueprint_file`, salida, logs, Rerun y transporte | evidencia de arranque, salud y rollback |
| Integracion | `blueprint.toml`, rules y overlays | validacion de catalogos y gates |
| ML | catalogos de modelos y artefactos | latencias, precision y filtros por modelo |
| Clinical/Protocol | `fsm.toml`, `zones.toml` y timers clinicos | aprobacion de thresholds, dwell y transiciones |
| Auditoria | lectura de JSONL y goldens | correlacion por `scan_seq` y `evidence_frame_id` |

Ningun rol debe mezclar en un mismo cambio el blueprint, thresholds de modelo,
zonas clinicas y transiciones FSM. Si el cambio toca dos dominios, dividirlo en
dos entregas o registrar explicitamente la dependencia.

## 4. Catalogo De Blueprints

### `detect-room-raw`

Uso: calibracion temporal de cardinalidad raw.

```toml
[inference]
blueprint_file = "config/blueprints/detect-room-raw/blueprint.toml"

[pipeline]
infer = true
track = false
zones = false
fsm = false
```

Valida `empty`, `single` y `multiple` con el conteo raw y los timers de
ocupacion. No valida continuidad de identidad, face, pose, segmentacion, zonas
clinicas ni la FSM.

### `detect-face`

Uso: detect primario y face con coste reducido para una persona estable.

```toml
[inference]
blueprint_file = "config/blueprints/detect-face/blueprint.toml"

[pipeline]
infer = true
track = true
zones = false
fsm = false
```

Usa `detect-fast -> face-yolo`. El child necesita exactamente una persona
elegible y el track confirmado. Es apropiado para validar el costo y el gate de
face, no para probar reglas de cama.

### `detect-room-face`

Uso: perfil 24/7 de cardinalidad, continuidad espacial y ciclo de vida facial.

```toml
[inference]
blueprint_file = "config/blueprints/detect-room-face/blueprint.toml"
fsm_file = "config/blueprints/detect-room-face/fsm.toml"
zones_file = "config/zones.toml"

[pipeline]
infer = true
track = true
zones = true
fsm = true
```

Usa la cascada `detect-fast -> face-yolo`. `face-yolo` corre cuando el gate de
cardinalidad permite exactamente una persona. El crop facial dinamico, la zona
semantica `zones.bed` y el ROI fijo `face_dwell` son tres objetos distintos.

### `detect-face-pose-seg`

Uso: enriquecimiento estable de una persona con face, pose y segmentacion.

```toml
[inference]
blueprint_file = "config/blueprints/detect-face-pose-seg/blueprint.toml"

[pipeline]
infer = true
track = true
zones = false
fsm = false
```

No arrancar este perfil con `track = false`: sus children dependen de tracks
confirmados. La segunda persona debe quedar fuera del gate de exactamente una
persona antes de que los children corran.

## 5. Configuracion De Produccion

### 5.1 Seleccion central

En `config/mana.toml` se selecciona el perfil y sus catálogos:

```toml
[inference]
model_catalog = "config/models.toml"
blueprint_file = "config/blueprints/detect-room-face/blueprint.toml"
depth_rules_file = "config/depth-rules.toml"
zones_file = "config/zones.toml"
fsm_file = "config/blueprints/detect-room-face/fsm.toml"
```

Las rutas se resuelven desde la raiz de ejecucion. Antes de arrancar, confirmar
que todos los archivos y artefactos de modelos existen.

### 5.2 Politica de sala y salud

Los tiempos son milisegundos reales del reloj monotónico, no cantidad de frames:

```toml
[presence]
enabled = true
class = "person"

[presence.poi]
on_ms = 200
off_ms = 16000

[presence.occupancy]
single_confirm_ms = 3000
empty_confirm_ms = 8000
multiple_confirm_ms = 5000
multiple_exit_ms = 5000
require_confirmed_tracks = false

[health]
data_stale_ms = 10000
stale_warn_ms = 5000
```

No multiplicar estos valores para compensar la frecuencia de I-frame. Si la
fuente o el dispositivo cambia, medir la latencia y revisar la politica con
Clinical/Protocol.

### 5.3 Observabilidad minima obligatoria

`scene_signals_events` debe permanecer en `true` en los perfiles operativos:

```toml
[metrics.jsonl]
presence_events = true
scene_signals_events = true
fsm_events = true
face_dwell_events = true
zone_events = true
depth_events = true
```

`scene_signals` es informativo y se persiste por defecto incluso con
`jsonl_level = "info"`. `presence` y `face_dwell` siguen siendo debug en el
logger actual; usar `jsonl_level = "debug"` durante calibracion.

### 5.4 Guard generico de señal

Un guard simple se declara en el `fsm.toml`:

```toml
[[fsm.transitions]]
from = "searching"
to = "detected"
dwell = "500ms"
guards = [
    { type = "signal", tag = "persona.presente", op = "==", value = true },
    { type = "signal", tag = "cara.confianza", op = ">=", value = 0.80 },
]
```

Reglas del contrato:

- `Bool` acepta `==` y `!=`.
- `Count` acepta igualdad y comparaciones ordenadas.
- `Ratio` acepta `>=`, `<=`, `>` y `<`; no acepta igualdad exacta.
- `Label` acepta `==` y `!=` contra el conjunto cerrado del catalogo.
- Una señal ausente no coincide, ni siquiera con `!=`.
- `zone_*`, `data_*` y `depth_rule` siguen siendo guards especializados.
- Los guards `signal` no se colocan en transiciones wildcard.

El bootstrap acumula errores de tag, tipo, operador, rango y label antes de
iniciar el motor. Corregir el catalogo; no forzar un valor por defecto.

## 6. Contrato Del Gemelo Visible

Cada `scan()` produce un `SceneSignalsSnapshot` con catalog v1 y nueve tags en
orden estable:

| Tag | Tipo | Ausencia esperada |
|---|---|---|
| `cara.confianza` | `ratio` | sin cara seleccionada |
| `cara.en_borde` | `bool` | nunca en productor v1 |
| `cara.en_dwell` | `bool` | sin ROI `face_dwell` configurado |
| `cara.estuvo_dentro` | `bool` | sin `FsmEngine` |
| `cara.modelo_corrio` | `bool` | nunca en productor v1 |
| `cara.presente` | `bool` | nunca en productor v1 |
| `ocupacion.cardinalidad` | `label` | nunca en productor v1 |
| `persona.cantidad` | `count` | nunca en productor v1 |
| `persona.presente` | `bool` | nunca en productor v1 |

La línea JSONL tiene esta forma resumida:

```json
{"type":"scene_signals","scan_seq":481,"evidence_frame_id":42,
 "observations_age_ms":0,"depth_age_ms":null,"catalog_version":1,
 "signals":[{"tag":"cara.confianza","kind":"ratio","value":0.84},
             {"tag":"cara.en_dwell","kind":"bool","absent":true}]}
```

Para un incidente, leer juntos:

1. `scan_seq`: ciclo de control.
2. `evidence_frame_id`: frame que aporto la observacion.
3. `observations_age_ms` y `depth_age_ms`: frescura de la evidencia.
4. `signals`: valores y ausencias, sin inferir ausencias como `false`.
5. `fsm` y `face_dwell`: transicion confirmada y timers de la FSM.
6. `health`: stale/blind/recovered y razon operativa.

## 7. Escenarios Operativos

### BP-S01 — Calibrar cardinalidad raw

**Perfil:** `detect-room-raw`.
**Objetivo:** validar filtros, presencia y timers con video conocido.
**Configuracion:** `track = false`, `zones = false`, `fsm = false`.
**Evidencia:** `presence`, `scene_signals`, resumen de inferencia.
**Aprobacion:** `empty -> single -> multiple -> empty` en los tiempos esperados.
**No aprobar:** face, continuidad de `track_id` o salida de cama con este perfil.

### BP-S02 — Validar face con una persona

**Perfil:** `detect-face`.
**Objetivo:** medir costo y calidad del child `face-yolo`.
**Configuracion:** `track = true`, `zones = false`, `fsm = false`.
**Evidencia:** `entity`, `detection`, `scene_signals`; Rerun de crop y boxes.
**Aprobacion:** face corre con un track elegible, se omite con cero o dos personas.
**No aprobar:** politica de cama o transiciones FSM.

### BP-S03 — Operar ciclo de vida facial

**Perfil:** `detect-room-face`.
**Objetivo:** ejecutar cardinalidad, zonas y FSM facial en una habitacion.
**Configuracion:** `track = true`, `zones = true`, `fsm = true`.
**Secuencia nominal:** `idle -> searching -> detected` o `in_bed`; borde y
  ausencia siguen sus dwell declarados.
**Evidencia:** `presence`, `zone`, `scene_signals`, `face_dwell`, `fsm`, `health`.
**Aprobacion:** las transiciones conservan thresholds y el snapshot del mismo
  `scan_seq` explica cada decision.

### BP-S04 — Operar enriquecimiento multi-modelo

**Perfil:** `detect-face-pose-seg`.
**Objetivo:** activar face, pose y segmentacion sólo con una persona estable.
**Configuracion:** `track = true`; zones/FSM según la necesidad del despliegue.
**Evidencia:** skips con cero/dos personas y calls con un track confirmado.
**Aprobacion:** un falso positivo aislado no activa children y un dropout no
  fabrica observacion fresca.

### BP-S05 — Persona sin cara

**Precondicion:** perfil con face y una persona presente.
**Resultado esperado:** `cara.presente = false`, `cara.confianza` ausente; con
  ROI configurado, `cara.en_dwell` es `false`, no ausente.
**Operacion:** no convertir la ausencia de confianza en cero ni elevarla a
  evento de error. Revisar `face_model_ran` en `face_dwell` y el snapshot.

### BP-S06 — Segunda persona

**Precondicion:** una sesion facial activa recibe dos personas sostenidas.
**Resultado esperado:** `ocupacion.cardinalidad = "multiple"`, la FSM aplica su
  salida de seguridad y `cara.estuvo_dentro` se limpia por la regla del latch.
**Operacion:** confirmar `multiple_confirm_ms`; no reducirlo para ocultar un
  problema de tracking.

### BP-S07 — Camara stale/blind

**Precondicion:** la evidencia deja de actualizarse.
**Resultado esperado:** `observations_age_ms` crece, Health emite stale/blind y la
  FSM puede ir a `safe = blind`.
**Operacion:** un valor visible dentro de `scene_signals` no significa frescura;
  correlacionar siempre con las edades del `ControlStamp` y `health`.

### BP-S08 — Falla de sink

**Precondicion:** JSONL, stdout o handler de salida no está disponible.
**Resultado esperado:** puede perderse el reporte, pero T2 completa el scan y
  la FSM conserva la decision.
**Operacion:** revisar warnings del logger y el estado del proceso; no reiniciar
  el motor clínico como primera respuesta.

## 8. Procedimientos Operativos

### SOP-BP-01 — Alta de un blueprint

1. Crear `config/blueprints/<name>/blueprint.toml`.
2. Declarar `primary_model`, la lista completa de `models` y una rule root.
3. Declarar cada child con `requires`, `requires_class`, cardinalidad y
   `same_frame` explícitos.
4. Usar `requires_tracking = true` si alguna regla depende de tracks.
5. Crear un overlay `models.toml` sólo si el tuning es local al blueprint.
6. Seleccionar el archivo desde `config/mana.toml`.
7. Agregar README del perfil con objetivo, pipeline y límites.
8. Registrar el caso en `docs/guides/blueprint-selection.md` si cambia la
   matriz de selección.

### SOP-BP-02 — Validar antes de arrancar

Desde la raíz del repositorio:

```sh
git status --short --branch
cargo test --workspace
cargo test --test fsm_catalogs_compile
cargo fmt --all -- --check
git diff --check
```

Confirmar manualmente:

- Las rutas de modelos, zonas, depth, FSM y métricas existen.
- El blueprint referencia un modelo primario presente en el catalogo.
- `pipeline.track` coincide con `requires_tracking`.
- `zones = true` sólo cuando `zones_file` es válido.
- `fsm = true` sólo cuando el programa compila con sus referencias.
- `scene_signals_events = true` en el perfil operativo.

### SOP-BP-03 — Arrancar y promover

Para una ejecución local o de servicio:

```sh
cargo run -- --config config/mana.toml
```

Para el artefacto optimizado:

```sh
cargo build --release
./target/release/mana-lite --config config/mana.toml
```

Promover sólo después de observar una ventana completa que cubra presencia,
ausencia, una segunda persona y stale/blind. Guardar el JSONL de evidencia y la
revision de configuración usada.

### SOP-BP-04 — Monitorizar en vivo

El logger rota bajo `output.save_dir`; el perfil base usa `./logs` y rotacion
horaria. Consultas practicas:

```sh
rg '"type":"scene_signals"' logs/
rg '"type":"(fsm|health|face_dwell|presence)"' logs/
rg '"event":"(stale|blind|recovered)"' logs/
```

Durante una investigación, agrupar por `scan_seq`, no por timestamp de pared.
El `ControlStamp` es la autoridad de correlación dentro del ciclo.

### SOP-BP-05 — Diagnosticar un incidente

1. Identificar el `scan_seq` del `fsm` o de la alarma clínica.
2. Buscar el `scene_signals` del mismo `scan_seq`.
3. Comparar `evidence_frame_id` y edades de observacion/depth.
4. Verificar tags ausentes de forma explícita.
5. Revisar `presence`, `zone`, `face_dwell` y `health` del mismo ciclo.
6. Determinar si el problema fue evidencia, política, FSM, sink o despliegue.
7. No cambiar configuración durante la recolección del caso.

### SOP-BP-06 — Cambiar y volver atrás

1. Copiar `mana.toml`, métricas y blueprint activos con una identificación de
   release.
2. Cambiar una sola familia: blueprint, modelo, zona, FSM o política temporal.
3. Ejecutar la compuerta de pruebas y el escenario afectado.
4. Promover con un nuevo directorio o rotación de logs.
5. Si falla, restaurar el `blueprint_file` y las rutas de catálogos anteriores.
6. Confirmar que vuelve a aparecer el mismo patrón de `scene_signals` y que la
   FSM retoma sus decisiones conocidas.
7. Registrar la causa y la evidencia en el ticket de operación.

## 9. Checklist De Aceptacion

- [ ] El blueprint activo y su revision están registrados.
- [ ] Los artefactos de modelos existen y cargan.
- [ ] El pipeline coincide con los requisitos del blueprint.
- [ ] Las zonas y el ROI `face_dwell` están calibrados en coordenadas de frame.
- [ ] El `fsm.toml` compila sin errores acumulados.
- [ ] `scene_signals_events = true` y el nivel permite el diagnóstico requerido.
- [ ] Se observaron `empty`, `single` y `multiple`.
- [ ] Se probó persona sin cara y se verificaron ausencias.
- [ ] Se probó stale/blind y se verificaron edades del `ControlStamp`.
- [ ] Se guardó un JSONL completo de la ventana de promoción.
- [ ] Existe rollback de configuración y artefactos.

## 10. Fuentes

- [Blueprint selection](../guides/blueprint-selection.md)
- [Manual general de blueprints](../manual/inference-blueprints.md)
- [Observabilidad JSONL](../observability.md)
- [Contrato de señales](1-spec.md)
- [Cierre de Etapa D](11-sprint-4-cierre.md)
