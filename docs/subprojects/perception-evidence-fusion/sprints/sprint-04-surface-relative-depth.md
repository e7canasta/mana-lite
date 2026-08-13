# Sprint 4: Profundidad Relativa a Superficies

**Estado:** en implementacion

## Objetivo

Crear una calibracion operativa para una camara fija que permita comparar las
huellas de body parts contra zonas conocidas de cama y piso, sin reconstruir una
postura 3D general y sin activar aun el FSM clinico.

## Entregables

- `SurfaceCalibration` y `SurfaceZone` como contrato serializable.
- Sesion `deep-calib.toml` con poligonos, contexto y estadisticas robustas.
- Binario auxiliar Rust `deep-calib`, aislado del comando principal.
- Reanudacion y escritura atomica de sesiones.
- Envolvente por zona usando mediana, p10, p90, MAD y cobertura.
- Evidencia `bed_residual`/`floor_match` por huella corporal sobre
  `depth-scene`.
- Diagnostico JSONL/Rerun, sin cambios al contrato del FSM.
- Tests con gradiente de perspectiva, perfiles incompatibles y zonas con pocos
  valores validos.

## Flujo

```text
imagen/frame reproducible
        |
        v
deep-calib + depth-scene
        |
        +--> poligono bed/head
        +--> poligono bed/body
        +--> poligono bed/feet
        +--> poligono floor/*
        |
        v
deep-calib.toml
        |
        +--> promocion explicita
        v
runtime SurfaceCalibration
        |
        v
body-part footprints + polygon_stats
        |
        v
evidencia relativa y temporal
```

## Alcance

- La primera entrada sera imagen/replay y vertices de poligono, para mantener
  reproducibilidad y no acoplar el calibrador al ingest RTSP.
- La captura live puede agregarse como adaptador sin cambiar el perfil.
- La referencia de superficie se toma con la escena vacia; la tolerancia para
  un cuerpo apoyado se mantiene separada y configurable.
- Las zonas se expresan en coordenadas globales del frame, no en coordenadas
  locales de un crop.
- La postura se publica como evidencia diagnostica y no como guard FSM.

## Fuera de alcance

- Intrinsecos, extrinsecos, triangulacion y coordenadas 3D.
- Entrenamiento o cuantizacion de modelos.
- UI completa de seleccion por clicks.
- Promocion automatica de umbrales clinicos.
- Soporte inicial para multiples actores sin track inequivoco.

## Criterios de aceptacion

- Una sesion TOML round-trip conserva contexto, poligonos y estadisticas.
- Una zona con gradiente puede distinguirse de otra zona con el mismo frame.
- El perfil se rechaza si modelo, ROI o dimensiones no coinciden.
- `depth-person` no alimenta el residual de cama/piso.
- Poligonos invalidos, degenerados o fuera de frame producen error explicito.
- La escritura interrumpida no deja una sesion parcialmente reemplazada.
- Las estadisticas ignoran valores no finitos y conservan cobertura.
- La salida diagnostica no cambia el FSM ni las reglas de profundidad existentes.
