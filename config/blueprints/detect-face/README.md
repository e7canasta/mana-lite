# `detect-face`

Perfil ligero para calibracion y despliegues con coste bajo.

```text
detect-fast
    └── face-yolo (exactamente una persona, mismo frame)
```

No requiere tracking. El gate usa las detecciones aceptadas por
`detect-fast`; la confianza y el area base se controlan en
`config/models.toml`.

Para activarlo, `config/mana.toml` debe apuntar a:

```toml
[inference]
blueprint_file = "config/blueprints/detect-face/blueprint.toml"
```
