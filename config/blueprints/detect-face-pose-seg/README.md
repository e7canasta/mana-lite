# `detect-face-pose-seg`

Perfil estable para ejecutar enriquecimiento solo cuando hay exactamente una
persona confirmada y visible.

```text
detect-fast
    ├── face-yolo
    ├── pose-standard
    └── seg-standard
```

Este perfil requiere:

- `[pipeline] track = true` en `config/mana.toml`.
- Dos detecciones consecutivas por defecto para confirmar un track.
- Un track visible (`misses = 0`) y de clase `person`.
- Exactamente un track elegible en la escena configurada.

El tracking evita activar los modelos hijos por un falso positivo aislado.
`max_age` conserva la identidad para tracking, pero no permite inferencia hija
con una observacion ausente: los hijos se omiten si el track no esta visible.
