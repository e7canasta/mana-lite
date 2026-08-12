# Workshop

Banco de escenarios para homologación funcional y operativa de `mana-lite`.

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

Los seis están corridos y verdes al 2026-08-12, con los números medidos en el
README de cada uno. El 06 es el único que no agrega una capa: existe para
**mirar**, porque los cinco de abajo prueban que el sistema sostiene su contrato
temporal y ninguno prueba que lo que ve sea razonable.

Sin cobertura todavía: los blueprints `detect-face-pose-seg` y
`detect-room-raw`.

### La cámara de la instalación suele estar vacía

Los escenarios 04, 05 y 06 no prueban lo que dicen probar sin una persona en
escena: la cascada no corre, el FSM se queda en `idle`, y una compuerta cerrada
por la razón correcta no se distingue de una rota. Los tres se corren también
contra un RTSP local con una persona en cama, a la misma cadencia de keyframe
que la cámara. El 06 lo tiene como argumento (`run-fuente.sh clip1`).

## Cómo correr un escenario

Siempre desde la raíz del repositorio, y siempre a través de `cargo run` — nunca
invocando un binario por ruta fija, porque `target-dir` está redirigido y
`./target/` puede contener un artefacto huérfano (ver
`docs/wiki/1.1-getting-started.md` → Build and Run):

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
