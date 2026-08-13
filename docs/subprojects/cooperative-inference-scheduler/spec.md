# Especificacion: Scheduler Cooperativo de Inferencia

**Identificador:** SUBSPEC-001
**Estado:** implementacion incremental; Sprint 3 cerrado a nivel runtime
**Version:** 0.2

## 1. Proposito

Definir un contrato temporal por modelo para la cascada de `mana-lite`, sin
alterar la cadencia fija del control ni convertir el runtime en un launcher de
workers.

## 2. Alcance

La especificacion cubre:

- intervalos minimos por modelo;
- elegibilidad por keyframe;
- gates de cascade existentes;
- politica latest-wins y sin catch-up;
- medicion de cadencia real y atraso;
- reserva para peticiones urgentes;
- contrato de frescura para resultados derivados.

No cubre batching, P-frames, preempcion, distribucion entre GPUs ni reinicio de
etapas.

## 3. Configuracion

La cadencia pertenece a la regla del blueprint porque es una decision del perfil
operativo. El catalogo y su overlay siguen describiendo el artefacto y sus
parametros tecnicos.

```toml
[[rules]]
model = "detect-fast"

[[rules]]
model = "face-yolo"
requires = "detect-fast"
requires_class = "person"
requires_exact_count = 1
interval_min_ms = 1000

[[rules]]
model = "seg-standard"
requires = "detect-fast"
requires_class = "person"
requires_exact_count = 1
interval_min_ms = 2000
```

Reglas:

- el campo es opcional para conservar compatibilidad;
- valor omitido equivale a `0`;
- `0` significa cada keyframe que supere los gates;
- un valor positivo es el minimo entre inicios de ejecucion;
- el valor no es una garantia de Hz exacta;
- el valor debe ser finito en la representacion de runtime y no puede
  desbordar al calcular el vencimiento.

## 4. Orden de decision

Para cada keyframe recibido, el scheduler debe aplicar estas capas:

1. modelos seleccionados por el blueprint y habilitados por el catalogo;
2. modelos pedidos por la directiva actual del FSM;
3. orden topologico: roots antes que children;
4. gate urgente, si existe una peticion valida;
5. intervalo vencido, si no hay urgencia;
6. gate de parent, clase, confianza, area, region y tracking;
7. resolucion del crop;
8. ejecucion secuencial y marca del inicio de ejecucion.

Una ejecucion no debe aparecer en `pending` hasta que `InferEngine::run` haya
producido un resultado valido. La marca temporal de scheduling debe representar
que el modelo fue intentado, para evitar reintentos ilimitados de una carga que
falla inmediatamente.

## 5. Reglas de frescura

- Cada salida debe conservar el `frame_number` del keyframe que la produjo.
- Una salida ausente por intervalo no se considera una salida fresca vacia.
- El resultado anterior solo puede reutilizarse si el consumidor acepta
  explicitamente evidencia envejecida.
- El control no recibe masks ni keypoints crudos por este mecanismo.
- Una senal derivada de validacion cruzada debe declarar su propio timestamp y
  edad.
- Un decode fallido no actualiza la edad de evidencia ni cuenta como ejecucion
  valida del modelo.

## 6. Politica latest-wins

El sistema no debe drenar una cola de keyframes antiguos para cumplir una tasa.
Si percepcion esta ocupada:

```text
put(I2) -> put(I3) -> take() == I3
```

El slot debe contar el overwrite. La ingesta puede ademas contar keyframes
suprimidos durante su propio drenaje. Ambos contadores deben conservar su
significado diferente.

## 7. Urgencias, version futura

Una peticion urgente debe tener como minimo:

```text
model_key
reason
priority
requested_at
expires_at
```

Semantica:

- salta `interval_min_ms` una vez;
- no interrumpe una inferencia en curso;
- `expires_at` debe ser posterior a `requested_at` y el TTL no puede superar
  cinco segundos;
- prioridad mayor gana; en empate gana `requested_at` mas antiguo y luego el
  orden lexicografico de modelo y razon;
- la vista de requests se congela al comenzar el keyframe; una request producida
  durante la inferencia queda para el siguiente keyframe;
- como maximo una request valida se admite de forma urgente por keyframe;
- se consume cuando el modelo inicia;
- expira si no puede atenderse dentro de su ventana;
- una request persistente se deduplica por `(model_key, reason)` mientras la
  directiva la mantenga; una transitoria usa una cola durable;
- el bypass no salta parent, clase, cantidad, confianza, region, tracking, crop
  ni orden topologico;
- no puede generar catch-up ni ejecuciones duplicadas del mismo modelo sobre
  el mismo keyframe sin una regla explicita de cross-validation.

Una urgencia persistente derivada del FSM puede reconstruirse en cada tick y
viajar en `ControlDirective`, pero no revive una request one-shot ya consumida
mientras conserve el mismo identity key. Una urgencia transitoria que no puede
perderse usa la cola durable de `CascadeScheduler`, no un slot latest-wins.

## 8. Cross-validation, Sprint 4

La validacion cruzada debe ser una senal semantica en el adaptador de
percepcion. Ejemplo:

```text
face uncertain
    -> urgent pose request
pose keypoints
    -> validate face geometry
    -> FacePoseValidation { valid, quality, frame_number }
    -> FSM signal
```

La version inicial debe aceptar la latencia de un keyframe adicional. La
ejecucion same-frame dinamica se implementara solo si una corrida demuestra que
esa latencia no es aceptable.

El resultado que cruza a control debe ser estrecho:

```text
FacePoseValidation { valid, quality, frame_number }
```

`quality` es un ratio finito en `[0,1]`. El timestamp lo aporta
`AgedEvidence<SceneSample>.observed_at` y la edad se calcula con
`ProcessImage::observations_age_ms()`. `None` significa que no hubo resultado;
`Some(valid=false)` significa que la validacion se ejecuto y rechazo la
evidencia. Keypoints, masks e indices de joints no cruzan el puerto de control.

## 9. Observabilidad requerida

Las metricas existentes ya informan inferences, Hz observado, latencia,
`skip`, `gated`, `empty`, `kf_pisados`, edad de evidencia y deadlines del
control. El scheduler debe agregar gradualmente:

- intervalo configurado por modelo;
- cantidad `not_due`;
- cantidad `due_but_gated`;
- cantidad `due_but_no_target`;
- gap entre inicios p50/p95/max;
- muestras y minimo del gap entre inicios;
- atraso contra `next_due` p50/p95/max;
- ejecuciones urgentes;
- urgencias expiradas;

Un informe de cinco segundos sirve para modelos a 1 Hz o mas. Para modelos a
0.5 Hz se deben usar ventanas de 30 a 60 segundos o corridas largas del
workshop.

En el reporte de runtime, las distribuciones por modelo se publican como
`gap_samples`, `gap_min_ms`, `gap_p50_ms`, `gap_p95_ms`, `gap_max_ms` y sus
equivalentes `due_late_*`. `urgent`, `urgent_expired`, `urgent_requests`,
`urgent_wait_*` y `urgent_starvation` forman parte del esquema desde Sprint 3.
Sprint 4 puede agregar metricas semanticas solo si tienen una decision
operativa asociada; no reciclar las metricas de urgencia.

## 10. Compatibilidad

Con todos los intervalos omitidos, la ejecucion debe ser equivalente al runtime
actual: mismos modelos elegibles, mismo orden, mismos slots y mismos eventos.

El scheduler no puede cambiar por si solo las reglas clinicas del FSM, la
semantica de `ProcessImage` ni los limites de los crates T2.

## 11. Criterios de aceptacion

- Un modelo con intervalo `0` corre en todos los keyframes elegibles.
- Un modelo con intervalo `2000 ms` no inicia dos ejecuciones con menos de dos
  segundos entre ellas.
- Un atraso no produce ejecuciones de recuperacion.
- Un keyframe viejo se pisa y el siguiente ciclo toma el mas fresco.
- Los gates siguen teniendo prioridad sobre la ejecucion normal.
- La configuracion omitida conserva el comportamiento actual.
- Las metricas distinguen no debido, gate y ejecucion real.
- `cargo test --workspace --release` permanece verde.
