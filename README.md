# Mana Lite

Pipeline de percepcion clinica en un unico binario Rust. Recibe RTSP, decodifica
H.264, ejecuta modelos ONNX, filtra y consolida detecciones, y publica JSONL y
Rerun.

## Estado Actual

El modo operativo por defecto de este workspace es la consolidacion stateless:

```toml
[pipeline]
infer = true
track = false
```

Esto publica observaciones del frame actual sin asignar `track_id`. El tracking
temporal existe como etapa opcional y se probara por separado.

El baseline actual ejecuta 5 ramas: `detect-fast` (raiz), `pose-standard`,
`face-yolo` y `seg-standard` como hijos `same_frame`, y `depth-standard` como
raiz independiente con mapa depth local a su ROI. La matriz FP16 de modelos
(4 tareas × 4 tamanos × 2 resoluciones) esta registrada deshabilitada para
benchmark en `tools/model-tools/`.

## Inicio Rapido

```bash
cargo test
cargo run -- --config config/mana.toml
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
- [Arquitectura](docs/ARCHITECTURE.md): modulos, ownership y ciclo principal.
- [Observabilidad](docs/observability.md): metricas, toggles y blueprint.
- [ROI y crops](docs/roi.md): crops estaticos y dinamicos.
- [Roadmap](docs/ROADMAP.md): estado y siguientes etapas.
- [ADRs](docs/adrs/): decisiones de diseno (001-024).

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

El tracker actual es un prototipo de prediccion lineal y matching greedy por
IoU. Kalman/Hungarian y TTL de evidencias quedan para una etapa posterior,
despues de validar la consolidacion con video real.
