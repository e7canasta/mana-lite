# Guia: Elegir Un Blueprint

## Decision rapida

```text
Calibracion o coste minimo       -> detect-face
24/7 con enriquecimiento estable -> detect-face-pose-seg
```

## `detect-face`

Elegir este perfil cuando:

- tracking aun esta en calibracion.
- se necesita observar detecciones y face.
- se quiere reducir carga de CPU/GPU.
- una decision del mismo frame es aceptable.

Su gate es directo: `detect-fast` debe producir exactamente una persona
aceptada en el frame actual.

## `detect-face-pose-seg`

Elegir este perfil cuando:

- pose y segmentacion deben ejecutarse solo sobre una persona estable.
- una deteccion aislada no debe activar inferencia cara.
- se necesita estabilidad entre frames.
- el coste de tracking es aceptable.

Su gate usa el tracker, no solo la deteccion del frame. Por defecto necesita
dos hits para confirmar la identidad y no utiliza tracks con misses.

## Elegir `same_frame` o tracking

`same_frame` es una politica de latencia y simplicidad. No es una politica de
estabilidad.

Tracking es una politica de estabilidad y continuidad. Debe preferirse para
despliegues 24/7, especialmente con `requires_exact_count` y modelos caros.

## Agregar un nuevo blueprint

1. Crear `config/blueprints/<nombre>/blueprint.toml`.
2. Declarar `primary_model`.
3. Declarar todos los modelos activos en `models`.
4. Añadir una regla root para el primario.
5. Añadir una regla por child.
6. Elegir `same_frame` o tracking de forma explícita.
7. Declarar `requires_tracking = true` si alguna regla depende de tracks.
8. Ejecutar `cargo test`.
9. Seleccionarlo desde `mana.toml`.

Nunca copiar entradas completas de `models.toml` dentro del blueprint. El
blueprint referencia claves estables del catalogo.
