# Guia: Elegir Un Blueprint

## Decision rapida

```text
Calibracion o coste minimo       -> detect-face
24/7 con enriquecimiento estable -> detect-face-pose-seg
```

## `detect-face`

Elegir este perfil cuando:

- solo se necesita detect y face.
- se necesita observar detecciones y face.
- se quiere reducir carga de CPU/GPU.
- se quiere tolerar un dropout corto del detector.

Usa tracking y presencia temporal: una persona aceptada activa la presencia y
los vacios se toleran durante `off_ticks` antes de declarar ausencia.

## `detect-face-pose-seg`

Elegir este perfil cuando:

- pose y segmentacion deben ejecutarse solo sobre una persona estable.
- una deteccion aislada no debe activar inferencia cara.
- se necesita estabilidad entre frames.
- el coste de tracking es aceptable.

Su gate usa el tracker y la presencia temporal, no solo la deteccion del frame.
Por defecto necesita dos hits para confirmar la identidad.

## Elegir `same_frame` o tracking

`same_frame` es una politica de latencia y simplicidad. No es una politica de
estabilidad.

La presencia temporal es una politica de estabilidad de senal. El tracking es
una politica de continuidad espacial e identidad. Para este caso deben usarse
juntos, aunque solo haya una persona.

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
