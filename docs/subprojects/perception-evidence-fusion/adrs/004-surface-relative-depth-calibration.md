# ADR-004: Calibracion Relativa a Superficies Conocidas

**Status:** Accepted
**Date:** 2026-08-13

## Contexto

La escena objetivo es fija: una camara de seguridad con angulo picado observa
una cama, su borde y una parte del piso. El producto no necesita coordenadas 3D
generales. Necesita saber si una parte corporal esta dentro de la envolvente de
la cama, cerca del piso o fuera de la superficie esperada.

La profundidad monocular puede tener escala relativa, gradiente por perspectiva
y variacion entre modelos/crops. Un valor global o un unico rango para toda la
cama produciria falsos positivos entre cabecera, cuerpo y pies.

## Decision

1. Calibrar `bed` y `floor` como colecciones de zonas poligonales globales.
2. Dividir inicialmente la cama en `head`, `body` y `feet`.
3. Guardar mediana, p10, p90, MAD y cobertura por zona; no usar min/max como
   envolvente operacional.
4. Usar exclusivamente `depth-scene` para comparar una parte contra una
   superficie de la escena.
5. Mantener `depth-person` como evidencia local entre partes y no compararlo
   contra perfiles de otro crop.
6. Ejecutar la calibracion mediante un binario Rust separado llamado
   `deep-calib`, sin arrancar `App`, scheduler, FSM ni RTSP del runtime.
7. Persistir la sesion en un TOML separado y requerir una promocion explicita
   para hacerla visible al runtime.
8. Mantener la postura y las extremidades fuera de cama como diagnostico hasta
   validar umbrales y persistencia con escenas reales.

## Alternativas descartadas

- Plano 3D con intrinsecos/extrinsecos: mas general que el problema y no
  necesario para la primera decision operativa.
- Un escalar global: no representa el gradiente de profundidad de la cama.
- Un rango global por superficie: puede funcionar solo como prototipo, pero
  pierde la diferencia cabecera/cuerpo/pies.
- Comparar depth-person absoluto con depth-scene: los mapas pertenecen a crops
  y contextos distintos.
- Python como implementacion primaria: duplicaria carga, preprocesado y
  semantica del modelo Rust existente.
- Sobrescribir `depth-rules.toml`: mezcla calibracion experimental con politica
  clinica y elimina una frontera de auditoria.

## Consecuencias

- El perfil es especifico de camara, ROI, modelo y resolucion.
- Cambiar cualquiera de esos elementos invalida la sesion.
- La perspectiva se absorbe primero con subzonas; si una zona sigue siendo
  demasiado ancha, se subdivide antes de introducir una superficie afín.
- El calibrador es pequeno y testeable, pero puede reutilizar exactamente el
  muestreo y los tipos de depth del pipeline.
- La salida inicial es evidencia, no una orden clinica.
