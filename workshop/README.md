# Workshop

Banco de escenarios para homologación funcional y operativa de `mana-lite`.

> **Si llegás hoy al proyecto, leé [`ONBOARDING.md`](ONBOARDING.md).** Es el
> primer día y el protocolo de prueba de versión: entorno, la escalera con sus
> compuertas y números de referencia, cómo decidir pasa/no pasa y qué dejar
> como evidencia. Después, [`MANUAL.md`](MANUAL.md): el manual operativo del
> banco — fuentes de video, cómo se lee cada línea del reporte y una tabla de
> síntoma → dónde mirar. Los dos son autocontenidos; este índice y el README de
> cada escenario son el detalle, no el punto de entrada.

`config/` es la configuración de producción. `workshop/` es donde se prueba una
cosa por vez, con la configuración completa a la vista y un criterio de
aceptación escrito **antes** de correr.

## Principio

Cada escenario aísla una capa y sólo una. Si un escenario falla, el defecto está
en la capa que ese escenario agregó respecto del anterior — no hay que buscarlo
en todo el pipeline. Por eso los escenarios se corren en orden y cada uno tiene
una compuerta: no se avanza al siguiente con el anterior en rojo.

Esto es lo contrario de arrancar con el pipeline completo y deducir hacia atrás.
Un `mana.toml` de producción tiene ingesta, inferencia, tracking, zonas, FSM y
visualización activos a la vez; cuando algo se comporta raro, todas son
sospechosas. La escalera existe para que en cada punto haya como mucho una.

## Escenarios

|#|Escenario|Capa que agrega|Compuerta|
|---|---|---|---|
|01|`01-ingest-only`|RTSP → decode → JSONL|Cadencia de keyframes estable, sin reconexiones, sin frames corruptos|
|02|`02-ingest-viz`|Bridge de Rerun|Una sola línea `viz: connected`, sin churn de reconexión|
|03|`03-ingest-infer`|Inferencia, un modelo|Atraso del lazo visible y acotado por la latencia del modelo|
|04|`04-infer-track`|Tracking y cascada con hijo|La compuerta del hijo gobierna y el recorte sigue al track|
|05|`05-clinical`|Zonas, FSM, presencia, ocupancia|Cadencia en el piso del temporizador con todo encendido|
|06|`06-clinical-viz`|*(ninguna)* — el 05 con visor|El visor no cuesta evidencia: `kf_pisados` e `img_pisadas` en cero|
|07|`07-detect-pose`|Rama de pose (`detect-pose`)|`pose-standard` carga, su compuerta gobierna y el recorte sigue al track|
|08|`08-detect-seg`|Rama de segmentación (`detect-seg`)|`seg-standard` carga y emite `mask` al JSONL con la compuerta gobernando|
|09|`09-detect-face-pose`|Segunda rama hermana (`detect-face-pose`)|Dos compuertas separadas, dos recortes, costo aditivo|
|10|`10-detect-face-pose-seg`|Tercera rama hermana (`detect-face-pose-seg`)|Tres compuertas separadas, máscara + pose + face en la misma corrida|
|11|`11-inference-capacity`|Instrumento de capacidad del scheduler|Gap y atraso por modelo en ventana larga para perfiles `s/m` en `192` y `320`|

Los seis primeros están corridos y verdes al 2026-08-12, con los números medidos
en el README de cada uno. El 06 es el único que no agrega una capa: existe para
**mirar**, porque los cinco de abajo prueban que el sistema sostiene su contrato
temporal y ninguno prueba que lo que ve sea razonable.

Los escenarios 07–10 extienden la escalera por la familia de perfiles ligeros:
uno por rama (pose, seg) y los dos abanicos (face+pose, face+pose+seg). Los
cuatro ya tienen corridas formales de 180 s contra `clip1` y `home2` usando el
baseline `YOLO26s FP16 320` para detección, pose y segmentación, y `YOLO12s FP16
320` para face. Las cuatro ramas cargan y gobiernan sus salidas sin
`kf_pisados`; segmentación bajó de aproximadamente 1.1 s por inferencia con
`YOLO26x 640` a aproximadamente 47–55 ms.

Sin cobertura todavía: el blueprint `detect-room-raw`.

### La cámara de la instalación suele estar vacía

Los escenarios 04, 05, 06 y 07–10 no prueban lo que dicen probar sin una
persona en escena: la cascada no corre, el FSM se queda en `idle`, y una
compuerta cerrada por la razón correcta no se distingue de una rota. Los que
tienen cascada se corren también contra un RTSP local con una persona en cama,
a la misma cadencia de keyframe que la cámara. El 06 lo tiene como argumento
(`run-fuente.sh clip1`).

## Cómo correr un escenario

Siempre desde la raíz del repositorio, y siempre a través de `cargo run` — nunca
invocando un binario por ruta fija, porque `target-dir` está redirigido y
`./target/` puede contener un artefacto huérfano (ver
`workshop/ONBOARDING.md` y `workshop/MANUAL.md` para Build and Run):

```sh
cargo run --release -- --config workshop/scenarios/01-ingest-only/mana.toml
```

Las salidas van a `workshop/runs/<escenario>/`, que está fuera de control de
versiones. Los `.jsonl` de una corrida son evidencia de esa corrida, no
artefactos del repositorio.

## Qué es una compuerta

Un criterio verificable sobre la salida, no una impresión. Cada escenario
declara en su `README.md`:

- **Hipótesis** — qué se afirma que funciona.
- **Criterios de aceptación** — condiciones observables, con el comando que las
  mide sobre el JSONL o el log.
- **Modos de falla conocidos** — qué síntoma corresponde a qué defecto, para no
  rediagnosticar lo mismo dos veces.

Un escenario sin criterio de aceptación escrito antes de correrlo no es una
homologación: es mirar logs y decidir después qué contaba como éxito.
