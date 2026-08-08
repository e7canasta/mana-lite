# ADR-011: Inference Engine Design

**Status:** Accepted
**Date:** 2026-08-04

## Context

La capa de inferencia debe cargar sesiones ONNX Runtime para cada modelo del catálogo, ejecutarlas bajo demanda, y devolver tensores crudos para postprocesamiento. El pipeline clínico tiene requisitos específicos:

1. **Multiple models per cycle:** un estado FSM puede pedir 2-4 modelos simultáneos (detect + pose + face).
2. **Session reuse:** cargar una sesión ORT toma 200-2000ms (IO + warmup). Las sesiones viven toda la vida del proceso.
3. **CPU-first:** la mayoría de despliegues son CPU (Jetson, edge x86). GPU es opcional.
4. **Graceful degradation:** si un modelo falla al cargar, los demás deben seguir funcionando.

## Decision

**Pool de sesiones ORT con dispatch secuencial.**

```rust
struct InferEngine {
    sessions: HashMap<String, ModelSlot>,
    allocator: ort::MemoryAllocator,    // reutilizado para todas las sesiones
}

struct ModelSlot {
    session: ort::Session,
    config: ModelEntry,                 // del models.toml
    status: ModelStatus,                // Loaded | Failed { reason }
    last_run_at: Instant,               // para interval scheduling (ADR-016)
    run_count: u64,
}

enum ModelStatus {
    Loaded,
    Failed { reason: String },           // no bloquea otros modelos
}

struct InferOutput {
    model: String,
    task: String,
    infer_ms: u64,
    outputs: Vec<ort::Value>,            // tensores crudos sin postprocesar
}
```

**API pública:**

```rust
impl InferEngine {
    /// Construye todas las sesiones del catálogo. Modelos que fallan → ModelStatus::Failed.
    fn new(catalog: &ModelCatalog, device: &str) -> Result<Self>;

    /// Ejecuta un modelo. Toma el PreprocessedFrame del cache.
    fn run(&mut self, model_key: &str, tensor: &ort::Tensor<f32>) -> Option<InferOutput>;
}
```

**Ciclo de vida de una sesión:**

```
startup:  ort::Session::new()  ←  carga .onnx del disco, warmup opcional
                                    falla → ModelStatus::Failed, continúa
                                    ok → ModelStatus::Loaded

runtime:  session.run(inputs!["images" => tensor])
              → Vec<ort::Value>  (tensores de salida: bboxes, scores, masks...)

shutdown: drop(InferEngine)  →  drop(ort::Session)  automático
```

## Model warmup

Primera inferencia de cada modelo es 2-5× más lenta (JIT compilation, memory allocation). Solución: warmup sintético durante startup.

```rust
fn warmup(&mut self, model_key: &str) {
    let slot = &mut self.sessions[model_key];
    let imgsz = slot.config.imgsz.unwrap_or(640);
    let dummy = ort::Tensor::<f32>::zeros(&[1, 3, imgsz as usize, imgsz as usize]);
    let _ = slot.session.run(inputs!["images" => dummy]);
    slot.run_count = 0;  // no cuenta para métricas
    log::info!("warmup: {model_key} ready");
}
```

El warmup se hace secuencial durante startup. Con 5 modelos × 100ms warmup = 500ms adicional en arranque, aceptable.

## Model dispatch por task

Cada modelo tiene un `task` declarado en `models.toml`. El InferEngine no interpreta la task — solo ejecuta y devuelve tensores crudos. El `Postprocessor` (ADR-012) sabe cómo interpretar cada task.

```
task = "detect"   →  outputs: [bboxes (1×84×N), scores, ...]     → YOLO detect head
task = "pose"     →  outputs: [bboxes + 17 keypoints × 3]         → YOLO pose head
task = "segment"  →  outputs: [bboxes + mask coefficients + proto] → YOLO segment head
task = "classify" →  outputs: [logits (1×N)]                      → classification head
task = "depth"    →  outputs: [depth map (1×H×W)]                  → monocular depth
```

El engine no necesita conocer estos formatos. Solo pasa `ort::Value` al postprocesador correcto.

## Why not parallel execution?

Cinco modelos en paralelo con `rayon` o `tokio::spawn_blocking` reducirían latencia de 200ms (secuencial: 50+30+40+30+50) a 50ms (max individual). Pero:

1. ORT ya usa threads internamente (intra-op parallelism). Ejecutar múltiples sesiones ORT en paralelo causa oversubscription: 4 modelos × 4 threads ORT = 16 threads en 4 cores.
2. El superloop es single-thread por diseño (ADR-003).
3. La latencia absoluta no es crítica para tiempo clínico (segundos, no ms).

**Decisión:** Ejecución secuencial para v0.2. Paralelismo opcional vía feature flag en v0.3 si hay demanda.

## Platform-specific dispatch

```rust
fn create_session(model_path: &Path, device: &str) -> Result<ort::Session> {
    let builder = match device {
        "cpu" => ort::SessionBuilder::new(ort::ExecutionProvider::CPU)?,
        "rocm" => ort::SessionBuilder::new(ort::ExecutionProvider::ROCm)?,
        "cuda" => ort::SessionBuilder::new(ort::ExecutionProvider::CUDA)?,
        "openvino" => ort::SessionBuilder::new(ort::ExecutionProvider::OpenVINO)?,
        "coreml" => ort::SessionBuilder::new(ort::ExecutionProvider::CoreML)?,
        _ => return Err(/* unknown device */),
    };
    builder.commit_from_file(model_path)
}
```

El device es global (definido en `mana.toml` → `ModelEntry.device`). En v0.2, un solo device para todos los modelos. Per-model device es futuro.

## Consequences

- **Positive:** Pool de sesiones evita recarga de modelos. Warmup elimina cold-start latency en producción.
- **Positive:** Graceful degradation — un modelo roto no mata el pipeline.
- **Positive:** Desacoplamiento InferEngine ↔ Postprocessor vía `ort::Value`.
- **Negative:** 5 modelos × 50-200MB RAM cada uno = 250-1000MB RAM total. Edge devices con <4GB pueden necesitar model quantization (INT8/FP16).
- **Negative:** Sin paralelismo, la latencia total es suma de latencias. Para 3 modelos: 8+30+5 = 43ms en CPU. Aceptable a 2fps clínicos.
- **Negative:** `ort::Value` API es volátil entre versiones de ORT. Necesitamos fijar la versión en `Cargo.toml` y testear upgrades.

## References

- ADR-005: Cascaded Inference
- ADR-010: Preprocess Cache
- ADR-012: Postprocess Pipeline
- ADR-016: Cascade Scheduler
- [ORT Rust API docs](https://docs.rs/ort/latest/ort/)
