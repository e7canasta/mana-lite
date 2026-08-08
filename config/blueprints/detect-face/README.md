# `detect-face`

Perfil ligero para despliegues con coste bajo y una sola persona estable.

```text
detect-fast
    └── face-yolo (exactamente una persona, track + presencia estable)
```

El gate usa las detecciones aceptadas por `detect-fast`; la confianza y el
area base se controlan en `config/models.toml`. Requiere tracking y el filtro
de presencia mantiene un dropout corto antes de declarar ausencia.
La maquina de cardinalidad mantiene `single` frente a un candidato aislado y
solo declara `multiple` cuando la politica de ocupacion confirma la segunda
persona.

Para activarlo, `config/mana.toml` debe apuntar a:

```toml
[inference]
blueprint_file = "config/blueprints/detect-face/blueprint.toml"
```
