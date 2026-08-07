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
- [Arquitectura](docs/ARCHITECTURE.md): modulos, ownership y ciclo principal.
- [Observabilidad](docs/observability.md): metricas, toggles y blueprint.
- [ROI y crops](docs/roi.md): crops estaticos y dinamicos.
- [Roadmap](docs/ROADMAP.md): estado y siguientes etapas.
- [ADRs](docs/adrs/): decisiones de diseno.

## Salidas

| Salida | Significado |
|---|---|
| stderr | Resumen operativo de ingest, inferencia y salud |
| JSONL | `detection` y `consolidated_detection`; `entity` cuando tracking esta activo |
| Rerun | Frame, ROI y observaciones; entidades si tracking esta activo |

## Configuracion

La entrada normal es un unico archivo:

```bash
cargo run -- --config config/mana.toml
```

`config/mana.toml` referencia el catalogo de modelos, cascada, zonas, FSM,
metricas, visualizacion y blueprint. No guardar credenciales reales en archivos
versionados.

## Verificacion

```bash
cargo test
git diff --check
```

El tracker actual es un prototipo de prediccion lineal y matching greedy por
IoU. Kalman/Hungarian y TTL de evidencias quedan para una etapa posterior,
despues de validar la consolidacion con video real.
