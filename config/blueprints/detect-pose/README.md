# `detect-pose`

Perfil ligero para despliegues con coste bajo y una sola persona estable,
con postura como enriquecimiento.

```text
detect-fast
    └── pose-standard (exactamente una persona, track confirmado)
```

El gate usa la misma regla que `face-yolo` en `detect-face`: exactamente un
track de persona confirmado y visible, y el recorte dinámico se resuelve sobre
ese track, de modo que un dropout corto del detector no apaga al hijo.

Los keypoints viajan en `Detection.keypoints` y se dibujan en Rerun como
esqueleto; el JSONL los registra por detección como campo `keypoints`
(`[[x, y, conf], ...]` en coordenadas de frame).

Para activarlo, `config/mana.toml` debe apuntar a:

```toml
[inference]
blueprint_file = "config/blueprints/detect-pose/blueprint.toml"
```
