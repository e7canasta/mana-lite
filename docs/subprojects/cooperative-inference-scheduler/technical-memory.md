# Memoria tecnica: Scheduler Cooperativo de Inferencia

**Estado:** Sprint 2 instrumentado; corrida de hardware, urgencias y validacion cruzada pendientes
**Ultima actualizacion:** 2026-08-12

## 1. Problema

Los blueprints actuales seleccionan modelos y declaran gates de cascada, pero
no expresan cuanto puede costar cada rama en el tiempo. El runtime ejecuta los
modelos elegibles en el hilo de percepcion y en orden secuencial. Esto funciona
con modelos pequenos, pero un modelo de segmentacion, pose o face de mayor
tamano puede consumir mas de un intervalo de keyframe.

El problema no es que el lazo de control se bloquee: ADR-033 ya lo aislo. El
problema restante es de capacidad de percepcion:

```text
fuente de keyframes > capacidad de percepcion
        |
        +-- el slot pisa muestras viejas
        +-- la evidencia nueva llega con mayor edad
        +-- otros modelos esperan al modelo pesado
        +-- la frecuencia real queda por debajo de la esperada
```

Procesar un backlog seria peor que descartarlo. En video, una muestra vieja
deja de representar el estado actual de la escena.

## 2. Estado actual que no se debe asumir

La arquitectura actual tiene tres dueños de ejecucion:

- ingesta: task async de RTSP y deduplicacion de keyframes;
- percepcion: un hilo que decodifica y ejecuta la cascada;
- control: un hilo con scan fijo, `5 Hz` por defecto.

Dentro de percepcion, `InferEngine` es una instancia mutable y las llamadas a
`predict_image` son sincronas. `run_inference` ejecuta roots y children de forma
secuencial. Los slots existentes son bordes entre etapas:

- `Slot<RawKeyframe>`: ingesta hacia percepcion;
- `Slot<PerceptionOutput>`: percepcion hacia control;
- `Slot<ControlDirective>`: realimentacion de control hacia percepcion.

No existe un slot de salida por modelo ni un worker por modelo.

## 3. Decision de primera version

Implementar un scheduler cooperativo dentro de `CascadeScheduler` y del ciclo
de percepcion. Cada regla puede declarar un intervalo minimo:

```toml
[[rules]]
model = "seg-standard"
requires = "detect-fast"
requires_class = "person"
interval_min_ms = 2000
```

`interval_min_ms = 2000` expresa como maximo una ejecucion cada dos segundos,
es decir, `0.5 Hz`. No promete que la ejecucion ocurra exactamente cada dos
segundos: la fuente, los gates y la capacidad real pueden reducirla.

El intervalo se mide entre inicios de ejecucion. Una ejecucion pesada no se
solapa con otra porque no hay workers; al terminar, la siguiente muestra fresca
se evalua contra el estado del scheduler.

## 4. Politica de atraso

La politica es latest-wins y sin catch-up:

1. percepcion toma el keyframe mas fresco disponible;
2. cada modelo se evalua una sola vez en ese ciclo;
3. un modelo no debido se omite;
4. un modelo debido sin target valido se omite por gate;
5. una ejecucion larga no genera llamadas retroactivas;
6. el siguiente ciclo vuelve a tomar la muestra mas fresca.

Si una inferencia tarda mas que el intervalo de la fuente, pueden aumentar
`keyframes_dropped` o `kf_pisados`. Eso no es una falla silenciosa: es la señal
operativa de que el perfil elegido excede la capacidad del dispositivo.

## 5. Cadencia configurada versus cadencia observada

El scheduler decide elegibilidad. La cadencia observada es emergente:

```text
Hz real <= min(Hz de fuente, Hz de capacidad, Hz de gates, Hz de intervalo)
```

Una frecuencia menor no implica automaticamente atraso. Un modelo puede correr
a `0.5 Hz` porque su intervalo lo pide. Hay atraso cuando estaba debido y no
puede ejecutarse por capacidad, o cuando la evidencia y los slots muestran que
percepcion no mantiene el ritmo de entrada.

Sprint 2 conserva esa distincion en las metricas:

- `not_due`: el intervalo aun no vencio;
- `due_but_gated`: estaba debido, pero el estado no lo solicito;
- `due_but_no_target`: estaba debido, pero la regla no encontro target;
- `gap_*`: tiempo entre inicios reales;
- `due_late_*`: atraso del inicio respecto de `next_due`.

Los gaps se agregan por ventana y se miden antes del backend, por lo que un
fallo de inferencia no desaparece de la medicion de capacidad.

## 6. Urgencias futuras

Una urgencia no sera una interrupcion del sistema operativo. Sera una prioridad
cooperativa que salta el intervalo normal una vez:

```text
face ambiguo -> request pose now -> pose elegible en el siguiente punto seguro
```

La peticion debe tener modelo, razon, prioridad y expiracion. El scheduler no
debe preemptar una llamada ONNX en curso. Para una peticion transitoria que no
puede perderse, el canal adecuado sera una cola; no debe depender de un slot que
puede sobrescribirla. Si la urgencia es una propiedad persistente del estado
FSM, puede derivarse en cada `ControlDirective` y permanecer en el slot.

## 7. Validacion cruzada

La salida de un modelo no debe invocar directamente a otro modelo ni introducir
tipos de percepcion en `mana-control`. La forma prevista es:

```text
face -> senal de incertidumbre
     -> request urgente de pose
pose -> keypoints
     -> adaptador de percepcion
     -> face_pose_confirmed / pose_quality
     -> FSM
```

La validacion puede ocurrir en el siguiente keyframe, que es la version simple,
o en el mismo frame mediante una cola dinamica de ejecucion, que sera posterior.
En ambos casos el resultado debe llevar `frame_number`, timestamp y edad.

## 8. Por que no workers ahora

Workers separados agregarian ownership de sesiones ONNX, copia o sharing del
RGB, sincronizacion de resultados y competencia por el mismo dispositivo. No
son necesarios para probar la politica de cadencia. El hilo de percepcion ya
esta aislado del control y el slot ya resuelve el desacople de muestras.

Un worker para un modelo pesado sera una optimizacion posterior, no una
precondicion del scheduler. Se justificara solo si las metricas muestran que un
modelo largo retrasa sistematicamente la publicacion de modelos livianos.

## 9. Riesgos conocidos

- Un intervalo mal elegido puede hacer que un modelo nunca alcance la cadencia
  deseada.
- Un window de metricas corto puede ocultar el comportamiento de un modelo a
  `0.5 Hz`.
- Reutilizar un resultado viejo sin edad puede crear evidencia falsa.
- Una urgencia sin limite puede matar la politica normal por starvation.
- Una dependencia circular de cascade puede producir una ejecucion imposible.

## 10. Decisiones pendientes

- Si el campo de intervalo se llama `interval_min_ms` o `period_ms` en el
  contrato publico. Esta memoria usa `interval_min_ms` porque expresa mejor un
  limite maximo de frecuencia.
- El formato exacto de `InferenceRequest` para urgencias.
- La politica de TTL para masks, keypoints y resultados reutilizados en
  visualizacion.
