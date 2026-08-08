# Manual Operativo: Blueprints De Inferencia

## 1. Seleccionar el perfil activo

Editar solo la referencia en `config/mana.toml`:

```toml
[inference]
model_catalog = "config/models.toml"
blueprint_file = "config/blueprints/detect-face/blueprint.toml"
```

El proceso carga el catalogo y activa solamente los modelos listados por el
blueprint. No es necesario editar `models.toml` para cambiar de perfil.

## 2. Perfil ligero

Usar `detect-face` cuando se necesita una deteccion primaria y face con coste
reducido:

```text
detect-fast -> face-yolo
```

Configuracion recomendada para una habitacion con una persona:

```toml
[pipeline]
infer = true
track = true
zones = false
fsm = false
```

El child face usa el track confirmado. El filtro de presencia mantiene la
ultima observacion durante cuatro ticks vacios por defecto. Puede omitirse si
hay mas de una persona.

## 3. Perfil estable multi-modelo

Usar `detect-face-pose-seg` para activar face, pose y segmentacion:

```toml
[inference]
blueprint_file = "config/blueprints/detect-face-pose-seg/blueprint.toml"

[pipeline]
infer = true
track = true
zones = false
fsm = false
```

Este perfil no debe arrancar con `track = false`: el bootstrap lo rechaza
porque los children necesitan tracks confirmados.

## 4. Que ocurre con una persona

Con el perfil multi-modelo:

```text
frame 1: detect encuentra person -> track tentativo -> no children
frame 2: track confirmado -> si hay exactamente una persona, corren children
frame N: track visible -> children pueden seguir corriendo
frame sin deteccion: misses > 0 -> children se omiten
dos personas: exact_count != 1 -> children se omiten
```

Esto evita que un candidato aislado, una deteccion vacia o una segunda persona
active ramas costosas.

## 5. Cambio 24/7

Procedimiento:

1. Ejecutar primero `detect-face` con video conocido y observar el filtro de
   presencia.
2. Verificar `detection` y `consolidated_detection` en JSONL.
3. Verificar `/world/camera/observations` en Rerun.
4. Cambiar a `detect-face-pose-seg` y activar `track = true`.
5. Confirmar que los children muestran `skip` con cero o dos personas.
6. Confirmar que los children corren solo con un track confirmado.
7. Reiniciar el servicio y observar una ventana completa de salud.

No cambiar simultaneamente el blueprint, los thresholds del modelo y las
zonas clinicas. Cada cambio debe poder atribuirse a una sola causa.

## 6. Diagnostico

| Sintoma | Interpretacion | Revisar |
|---|---|---|
| `face` nunca corre | No hay una persona elegible | clase, confianza, conteo y ROI |
| pose/seg siempre hacen `skip` | Falta track confirmado | `pipeline.track`, `min_hits`, `misses` |
| children corren con dos personas | Gate incorrecto | `requires_exact_count = 1` |
| muchas activaciones breves | Estabilidad insuficiente | postprocess y tracking |
| modelo no carga | path o artefacto invalido | `models.toml` |
| blueprint rechaza bootstrap | referencia invalida | modelo primario, lista `models` y rules |

## 7. Regla de propiedad

- ML mantiene `models.toml` y los artefactos.
- Integracion mantiene los blueprints.
- Operaciones selecciona `blueprint_file` y los toggles de `mana.toml`.
- Clinical mantiene `fsm.toml` y `zones.toml`.
