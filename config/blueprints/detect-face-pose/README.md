# `detect-face-pose`

Perfil ligero con dos enriquecimientos hermanos sobre el mismo detector base.

```text
detect-fast
    ├── face-yolo     (exactamente una persona, track confirmado)
    └── pose-standard (exactamente una persona, track confirmado)
```

Cada hijo declara la misma regla que `face-yolo` en `detect-face` y la resuelve
por separado: la compuerta de uno no depende de la del otro, y cada uno recorta
sobre el track confirmado con su propio crop. No hay orden entre hermanos.

Para activarlo, `config/mana.toml` debe apuntar a:

```toml
[inference]
blueprint_file = "config/blueprints/detect-face-pose/blueprint.toml"
```
