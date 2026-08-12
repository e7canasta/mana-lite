# Mana Lite

Middleware de control clínico en un único binario Rust. Recibe RTSP, decodifica
H.264, ejecuta modelos ONNX, consolida detecciones y corre una capa de control a
cadencia fija que publica JSONL y Rerun.

La frase que ordena el diseño está en [HANDOFF.md](HANDOFF.md):

> **mana-lite es un PLC cuyo dispositivo de campo es una cámara.** El programa
> corre a cadencia fija y tiene que emitir salida en cada tick aunque el campo
> esté muerto.

## Estado actual

El sistema corre en **tres etapas con dueños de ejecución distintos**, unidas por
bordes que no bloquean:

```
[task tokio]      RTSP → demux → dedupe
      │  Slot<RawKeyframe>
      ▼
[hilo percepción] decode → cascada → ProcessImage        ~221 ms
      │  Slot<PerceptionOutput>   │  Slot<VizBatch> ──► [hilo viz] ── Rerun
      ▼                           
[task tokio]      scan() @ 200 ms — el lazo de control
      ▼
      JSONL
```

El lazo de control mantiene su cadencia aunque la inferencia tarde más de un
periodo, aunque el visor sature el enlace o aunque percepción entre en pánico.
Medido: atraso p95 de **1,4–3,3 ms** sobre un periodo de 200 ms, con la
inferencia corriendo a 194–217 ms. Ver [ARCHITECTURE.md](ARCHITECTURE.md) §1.

El blueprint por defecto es `detect-room-face`: cardinalidad de sala más recorte
dinámico de cara, con FSM de ciclo de vida. La matriz FP16 de modelos (4 tareas ×
4 tamaños × 2 resoluciones) está registrada deshabilitada para benchmark en
`tools/model-tools/`.

## Inicio Rapido

```bash
cargo test
cargo run --release -- --config config/mana.toml
```

Para probar una capa por vez, el banco de escenarios de `workshop/` la enciende
de a una, con criterios escritos antes de correr:

```bash
cargo run --release -- --config workshop/scenarios/01-ingest-only/mana.toml
```

## Documentacion

- [Guia de operaciones](docs/operations.md): administracion, configuracion,
  logs de terminal, JSONL, Rerun y troubleshooting.
- [Onboarding de ingenieria](docs/onboarding.md): estructura del codigo y
  flujo del pipeline.
- [SPEC](docs/SPEC.md): contratos funcionales y formato de eventos.
- [Especificaciones](docs/specs/): blueprints de ramas — depth
  ([depth-standard](docs/specs/depth-standard.md)), segmentacion
  ([seg-standard](docs/specs/seg-standard.md)) y wire de mascaras
  ([mask-jsonl](docs/specs/mask-jsonl.md)).
- [Arquitectura de ejecución](ARCHITECTURE.md): hilos, relojes, puertos e
  invariantes. Es la que hay que leer antes de tocar el lazo.
- [Arquitectura del workspace](docs/ARCHITECTURE.md): partición en crates por
  tier (ADR-027, ADR-028).
- [Handoff](HANDOFF.md): dónde estamos, qué falta y cómo retomar.
- [Observabilidad](docs/observability.md): metricas, toggles y blueprint.
- [ROI y crops](docs/roi.md): crops estaticos y dinamicos.
- [ADRs](docs/adrs/): decisiones de diseño (001-035).

## Salidas

| Salida | Significado |
|---|---|
| stderr | Resumen operativo de ingest, inferencia y salud |
| JSONL | `detection`, `consolidated_detection` y `depth`; `entity` cuando tracking esta activo |
| Rerun | Frame, ROI, detecciones, mascaras, pose y depth; entidades si tracking esta activo |

## Configuracion

La entrada normal es un unico archivo:

```bash
cargo run -- --config config/mana.toml
```

`config/mana.toml` referencia el catalogo de modelos, cascada, zonas, FSM,
metricas, visualizacion y blueprint. No guardar credenciales reales en archivos
versionados.

### Donde viven los pesos ONNX

Los catalogos traen rutas relativas a la raiz del repo
(`tools/model-tools/artifacts/...`), y esos archivos estan gitignoreados: un
clone nuevo, un worktree o un job de CI no los tiene.

`MANA_MODELS_HOME` reancla las rutas **relativas** del catalogo; las absolutas
las fija el despliegue y no se tocan. Sin la variable, el comportamiento por
defecto no cambia.

```bash
export MANA_MODELS_HOME=/ruta/al/checkout-con-artifacts
```

## Verificacion

```bash
cargo test --workspace
git diff --check
```

La suite completa necesita los pesos ONNX: un test de arranque
(`bootstrap_with_reader_wires_real_catalogs`) construye `App` contra los
catalogos reales. Si no los tenes en la raiz del checkout, apuntalos con
`MANA_MODELS_HOME`.

El tracker usa filtro de Kalman con asociación por distancia de Mahalanobis
(`tracking.mahalanobis_threshold`) e IoU como respaldo. Hungarian queda pendiente:
la asignación sigue siendo greedy.
