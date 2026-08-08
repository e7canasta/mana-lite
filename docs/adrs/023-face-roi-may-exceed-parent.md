# ADR-023 — El ROI hijo (face) puede exceder deliberadamente el ROI padre

**Estado:** Accepted
**Fecha:** 2026-08-07

**Contexto:** en la prueba RTSP con ROI fijo `detect-fast = [560,140 1240,820]`
se observó que el crop de `face-yolo` se construye como un cuadrado de 320x320
centrado en el centro de la mitad superior de la persona, y ese cuadrado solo
se clampea contra los bordes del frame completo, no contra el ROI fijo del
padre. Consecuencia: cuando la persona toca el borde del ROI, `face-yolo` mira
píxeles fuera de la región que `detect-fast` analiza.

Esto fue interpretado primero como un error de diseño (el ROI verde no coincide
con el bbox azul). El análisis mostró que es comportamiento deliberado y útil:

- `detect-fast` (padre) sí está limitado a su ROI fijo.
- `face-yolo` (hijo) recibe un crop físico real, pero ese crop es un segundo
  ROI expandido alrededor de la persona, independiente del ROI del padre.
- El log demostró el caso concreto: persona `[1189,247 → 1239,651]`, face ROI
  `[1055,189 → 1375,509]`, face detectada `[1291,189 → 1338,228]` fuera del
  ROI fijo del padre.

**Decisión:** documentar como comportamiento oficial — no como workaround —
la regla:

1. **ROI padre estático:** `detect-fast` corre solo dentro de su región fija;
   nada fuera de ella existe para el padre.
2. **ROI hijo dinámico:** el crop de `face-yolo` es un cuadrado centrado en la
   mitad superior de la persona (`square_size = 320`, `upper_fraction = 0.50`),
   clampeado contra el frame completo. Puede sobresalir del ROI del padre.
3. **Espacios distintos:** el bbox azul (persona) vive en el espacio del ROI
   del padre; el ROI verde (face) vive en espacio de frame completo. No son el
   mismo espacio y no deben compararse directamente.
4. **Evidencia de face:** la face se consolida como componente de `person`
   solo si su bbox queda contenido en el bbox de la persona (política de
   consolidación), independientemente del ROI del padre.

**Alternativas descartadas:**
- Clampear el crop hijo contra el ROI del padre (`ROI face estricto =
  cuadrado ∩ ROI detect-fast`): degradaría la detección de caras en el borde y
  no resuelve un problema real de privacidad (el crop es temporal y no se
  publica como región analizada).

**Consecuencias:**
- Mantener ejemplos y demos de configuración en las guías (funciona mejor que
  lo planeado).
- La visualización debe mostrar ambos espacios por separado (boxes del padre
  en su ROI, crop del hijo como vista independiente).
- Cualquier regla futura que limite la visión de `face-yolo` debe definirse en
  su propia config `[models.face-yolo.crop]`, no heredando el ROI del padre.

## Aplicacion en `detect-room-face`

El blueprint conserva el crop dinamico de `face-yolo`: un cuadrado de 320 px
centrado en la mitad superior del track de `person`. El ROI fijo de
`detect-fast`, `face_dwell` y la zona semantica `zones.bed` no se reutilizan
como crop facial. El objeto observado sigue siendo la misma cara, pero cada
region tiene un contrato distinto: deteccion fisica, dwell facial fijo,
contexto de profundidad o permanencia de persona.
