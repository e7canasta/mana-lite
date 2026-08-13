# `detect-seg`

Perfil ligero para despliegues con coste bajo y una sola persona estable,
con máscara de instancia como enriquecimiento.

```text
detect-fast
    └── seg-standard (exactamente una persona, track confirmado)
```

El gate usa la misma regla que `face-yolo` en `detect-face`: exactamente un
track de persona confirmado y visible, y el recorte dinámico se resuelve sobre
ese track, de modo que un dropout corto del detector no apaga al hijo.

La máscara viaja en `Detection.mask` (espacio del crop, con `origin` al frame)
y se serializa al JSONL por detección (Spec-003) y a Rerun como overlay.

Para activarlo, `config/mana.toml` debe apuntar a:

```toml
[inference]
blueprint_file = "config/blueprints/detect-seg/blueprint.toml"
```
