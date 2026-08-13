# Memoria tecnica: Scheduler Cooperativo de Inferencia

**Estado:** scheduler cooperativo y validacion face/pose implementados; corrida
de hardware y politica clinica pendientes
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

La instrumentacion de capacidad conserva esa distincion en las metricas:

- `not_due`: el intervalo aun no vencio;
- `due_but_gated`: estaba debido, pero el estado no lo solicito;
- `due_but_no_target`: estaba debido, pero la regla no encontro target;
- `gap_*`: tiempo entre inicios reales;
- `due_late_*`: atraso del inicio respecto de `next_due`.

Los gaps se agregan por ventana y se miden antes del backend, por lo que un
fallo de inferencia no desaparece de la medicion de capacidad.

## 6. Urgencias cooperativas

Una urgencia no es una interrupcion del sistema operativo. Es una prioridad
cooperativa que salta el intervalo normal una vez:

```text
face ambiguo -> request pose now -> pose elegible en el siguiente punto seguro
```

La request implementada tiene modelo, razon, prioridad, `requested_at` y
`expires_at`, con TTL maximo de cinco segundos. Prioridad mayor gana; en empate
gana la request mas antigua. El scheduler congela las requests al comienzo de
cada keyframe, permite como maximo una admision urgente por keyframe y consume
la request al marcar el inicio, antes del backend. Una request persistente se
reemplaza con la directiva y conserva su identity key mientras esa directiva la
publique, para no revivirla en cada scan. Una transitoria vive en `VecDeque` y no
puede perderse por una directiva nueva.

La urgencia no saltea parent, clase, cantidad, confianza, region, tracking ni
crop. Tampoco preempta una llamada ONNX en curso. Si una request cruza dos
keyframes sin iniciar se cuenta como starvation una sola vez; si expira se
elimina y se cuenta una sola vez.

## 7. Validacion cruzada

La salida de un modelo no debe invocar directamente a otro modelo ni introducir
tipos de percepcion en `mana-control`. El contrato implementado es:

```text
face -> senal de incertidumbre
     -> request urgente de pose
pose -> keypoints
      -> adaptador de percepcion
      -> FacePoseValidation { valid, quality, frame_number }
      -> cara.pose_validada / cara.pose_calidad
      -> FSM
```

Los keypoints se quedan en percepcion. `SceneSample` recibe solo el resultado
semantico; `AgedEvidence<SceneSample>` ya aporta timestamp y
`ProcessImage::observations_age_ms()` aporta edad. La primera version produce la
request transitoria desde percepcion, conserva el `CascadeTarget` y valida en el
siguiente keyframe. El mapa de joints usado es COCO `[nose, eyes, ears]`,
verificado contra la forma `[1, 300, 57]` del ONNX real. El same-frame dinamico
queda fuera y solo se reconsidera si una corrida demuestra que la latencia
adicional es inaceptable.

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

## 10. Estado destilado y limites

La politica vigente queda resumida asi:

- `interval_min_ms` pertenece al blueprint y se mide entre inicios reales;
- latest-wins descarta muestras viejas y no hace catch-up;
- una urgencia cooperativa salta solo el intervalo, no los gates ni el orden
  topologico;
- las requests tienen prioridad, TTL, consumo one-shot y limites anti-starvation;
- face/pose cruza a control solo como `FacePoseValidation { valid, quality,
  frame_number }`, nunca con keypoints o masks;
- el control mantiene su cadencia y no espera a inferencia.

Quedan como validacion operativa, no como backlog de sprint, la corrida sobre
hardware representativo, el tuning de intervalos y la politica clinica concreta
que consumira pose validada.
