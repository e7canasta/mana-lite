# Roadmap: Fusion de Evidencias y Estimador de Partes Corporales

## Vision

Pasar de una cascada que solo publica detecciones independientes a una
percepcion cooperativa que conserva la evidencia, mide su acuerdo y puede
estimar geometria corporal sin confundir una derivacion con una deteccion cruda.

```text
model outputs
    -> association por actor
    -> validation de evidencias
    -> body parts estimator
    -> ventana temporal
    -> resumen semantico / diagnostico
```

## Dependencias existentes

- ADR-017: consolidacion cross-model.
- Scheduler cooperativo: cadencia, urgencias y frescura.
- Sprint 4 del scheduler: contrato especializado face/pose y
  `FacePoseValidation` ya presente en el working tree.
- Tracking: identidad temporal parcial; la evidencia por fuente y el historial de
  partes aun no existen.

## Sprints

### Sprint 0: contrato y limites

**Estado:** documentado.

- separar validacion cruzada de estimacion de partes;
- definir calidad continua y frescura;
- fijar que los payloads crudos permanecen en percepcion;
- documentar dependencias y no objetivos.

**Puerta de salida:** los dos productos tienen nombres, entradas y consumidores
distintos.

### Sprint 1: cross-model validation

**Estado:** implementacion inicial y corrida fisica completadas; calibracion
pendiente.

- ensamblar desde `PendingModelOutput`, no desde `ConsolidatedObservation`;
- extraer o formalizar un validador puro para face/pose;
- añadir relaciones pose/segment y face/segment sin convertirlas en gates duros;
- calcular calidad por fuente, acuerdo y frescura;
- probar actores distintos, crops, joints insuficientes y contradicciones;
- separar validacion semantica de requests urgentes del scheduler.

**Puerta de salida:** una validacion produce calidad reproducible y razones sin
mover keypoints ni mascaras a control.

El MVP debe limitarse a un actor confirmado y a la evidencia reunida desde
`PendingModelOutput`. Multi-actor general y asociacion temporal sin track quedan
fuera de esta puerta.

La primera implementacion ya produce el evento debug `cross_model_validation`
con `quality`, `agreement`, `freshness`, fuentes y razones. `freshness` vale
`1.0` mientras la validacion se limita al keyframe actual; la ventana temporal
pertenece al Sprint 3.

La corrida `m/192` con `detect + face + pose + seg` produjo 62 eventos de
validacion sobre 61 keyframes en 60 s. El JSONL fue valido, Rerun conecto y el
control registro 0 overruns y 0 deadlines perdidos.

### Sprint 2: body parts estimator

**Estado:** implementado inicialmente.

- definir geometria local para cabeza, tronco, brazos y piernas;
- crear soporte de segmentos ensanchados desde pose;
- usar face como ancla de cabeza;
- recortar/refinar con mascara sin modificar la mascara original;
- devolver calidad y frescura por parte;
- publicar primero en diagnostico/Rerun, no en politica clinica.

La primera salida implementada es `type=body_parts` en JSONL. Incluye actor,
geometría local, soporte, modelos de origen, calidad, cobertura de máscara,
frames de origen y `stale`; el estimador es stateless y publica `stale=false`.

**Puerta de salida:** se puede inspeccionar un conjunto de partes parciales y
explicar de que evidencias proviene cada una.

La salida inicial es diagnóstico/JSONL. No modifica `SceneSample`, el FSM ni
`DetectionConsolidator`.

### Sprint 3: fusion temporal y evidencia depth

**Estado:** temporalidad planificada; depth por body part implementado como
diagnostico opt-in.

- mantener ventana acotada `t-n ... t` por actor;
- aplicar decaimiento de frescura;
- suavizar puntos y geometria por parte;
- amortiguar contradicciones aisladas;
- degradar o expirar contradicciones persistentes;
- medir edad, stale y calidad por parte.

**Puerta de salida:** una fuente intermitente no produce saltos innecesarios ni
presenta evidencia vieja como fresca.

El track paralelo de depth ya entrega `PolygonStats`, huellas por parte,
`depth-scene`/`depth-person`, JSONL y Rerun. No convierte aun la profundidad en
postura ni en politica clinica.

### Sprint 4: profundidad relativa a superficies

**Estado:** en curso.

- definir `SurfaceCalibration` y `SurfaceZone` por poligono;
- construir `deep-calib` como binario auxiliar aislado;
- guardar y reanudar `deep-calib.toml` de forma atomica;
- generar envolventes `p10..p90` y MAD para cama y piso;
- calcular residual y cobertura por huella corporal sobre `depth-scene`;
- publicar evidencia diagnostica sin tocar el FSM;
- invalidar perfiles cuando cambien camara, ROI, modelo o resolucion.

**Puerta de salida:** una escena fija puede producir y auditar perfiles de cama
y piso, y el runtime puede distinguir evidencia valida, fuera de envolvente y
desconocida sin comparar crops incompatibles.

### Sprint 5: postura relativa y extremidades fuera de cama

**Estado:** futuro.

- combinar pose, mascara, superficies y persistencia;
- clasificar acostado, sentado sobre cama, sentado al borde y parado;
- detectar manos/pies fuera de cama;
- validar con escenas etiquetadas;
- decidir si algun resumen semantico estrecho llega al FSM.

## Puertas de decision

- No hacer ejecucion same-frame dinamica durante una inferencia salvo que una
  corrida pruebe que la latencia lo exige. El soporte estatico existente de
  `same_frame = true` no se elimina.
- No permitir que la confianza de un solo modelo borre las otras evidencias.
- No usar el convex hull global como unica representacion de cuerpo.
- No exportar body parts al control antes de fijar un contrato semantico estrecho.
- No agregar una metrica que no cambie una decision de tuning o seguridad.
