# Manual operativo del banco de escenarios

> **Para quién es esto.** Para alguien que llega hoy a `mana-lite` y tiene que
> poder correr el banco entero, leer lo que sale y decidir si está bien o mal.
> Es autocontenido: no hace falta leer nada más para operarlo.
>
> *Actualizado: 2026-08-12, con la escalera de seis peldaños corrida y verde.*

---

## Índice

1. [El sistema en cinco minutos](#1-el-sistema-en-cinco-minutos)
2. [Por qué hay un banco y no un solo `mana.toml`](#2-por-qué-hay-un-banco-y-no-un-solo-manatoml)
3. [Preparación del entorno](#3-preparación-del-entorno)
4. [De dónde sale el video](#4-de-dónde-sale-el-video)
5. [Procedimiento: correr el circuito completo](#5-procedimiento-correr-el-circuito-completo)
6. [Los seis peldaños, uno por uno](#6-los-seis-peldaños-uno-por-uno)
7. [Cómo se lee el reporte](#7-cómo-se-lee-el-reporte)
8. [Cómo se lee el JSONL](#8-cómo-se-lee-el-jsonl)
9. [Diagnóstico: síntoma → dónde mirar](#9-diagnóstico-síntoma--dónde-mirar)
10. [Las reglas que costaron caro](#10-las-reglas-que-costaron-caro)
11. [Referencias](#11-referencias)

---

## 1. El sistema en cinco minutos

`mana-lite` es **un PLC cuyo dispositivo de campo es una cámara**. No es una
aplicación de visión con lógica adentro: es un controlador de cadencia fija que
resulta tener un sensor óptico.

Eso no es una metáfora, es la propiedad que gobierna todo el diseño. Corre a
**dos tasas distintas**:

| | El campo | El programa |
|---|---|---|
| Qué es | RTSP → decode → ONNX | tracker → presencia → ocupancia → zonas → FSM → salud |
| A qué velocidad | la que pueda; ~1 keyframe/s | cadencia fija, 5 Hz |
| Puede fallar | sí, es normal | **no**: tiene que emitir salida en cada tick |

Entre los dos hay **un solo objeto**: la imagen de proceso (`ProcessImage`),
congelada y fechada. El programa nunca espera al campo. Si la inferencia tarda
más que un periodo de scan, el lazo sigue ticando sobre la evidencia anterior y
esa evidencia **envejece de forma medible**.

**La pregunta que ubica cualquier cosa en este repositorio:**

> *Si la entrada nunca vuelve a llegar, ¿esto tiene que seguir produciendo
> salida correcta en cada tick?*
>
> Sí → tier de programa (T2, `mana-control`). No → tier de campo (T1).

El caso clínico que corre hoy es prevención de caídas de cama y ciclo de vida de
la cara: `idle → searching → in_bed / detected / edge / other → exiting`.

### El vocabulario mínimo

| Término | Qué es |
|---|---|
| **scan / tick** | una iteración del lazo de control, cada 200 ms |
| **keyframe** | un cuadro I del RTSP; es la única entrada que se decodifica |
| **evidencia** | las observaciones que produjo percepción sobre un keyframe |
| **edad de la evidencia** | cuánto hace que se produjo lo que este scan está usando para decidir. **Es la única magnitud con consecuencia clínica** |
| **atraso (`dline`)** | cuánto después de su vencimiento arrancó un scan |
| **periodo (`cycle`)** | cuánto pasó entre dos scans. Eje distinto del atraso |
| **muestra pisada** | un dato que se sobrescribió antes de que alguien lo tomara. Es la degradación correcta para una muestra |
| **cascada** | un modelo hijo que corre sobre el recorte que produjo un modelo padre |
| **directiva** | lo que control publica para que percepción sepa qué correr y dónde mirar. Es el brazo de vuelta del lazo cerrado |

---

## 2. Por qué hay un banco y no un solo `mana.toml`

Un `mana.toml` de producción tiene ingesta, inferencia, tracking, zonas, FSM y
visualización activos a la vez. Cuando algo se comporta raro, **todas son
sospechosas**, y diagnosticar es deducir hacia atrás sobre seis capas.

La escalera existe para que en cada punto haya como mucho una. Cada peldaño
agrega **una capa y sólo una** respecto del anterior. Si el 04 falla y el 03
estaba verde, el defecto está en lo que el 04 agregó. No hay que buscarlo en
todo el pipeline.

De ahí salen tres reglas de operación:

1. **Se corren en orden.** No se avanza al siguiente con el anterior en rojo.
2. **El criterio de aceptación se escribe antes de correr.** Un escenario sin
   criterio escrito de antemano no es una homologación: es mirar logs y decidir
   después qué contaba como éxito.
3. **Un "dio bien" no sirve.** Cada README tiene una tabla de números medidos
   con la corrida que los produjo.

### Qué es una compuerta

Un criterio **verificable sobre la salida**, no una impresión. Y hay una
distinción que importa: una compuerta es pasa/no pasa, y hay números que no son
compuertas sino observaciones que se registran corrida a corrida. `viz_pisados`
es el ejemplo: varía entre 0 y 130 en corridas idénticas y no significa nada
malo. Confundir los dos tipos hace que la gente aprenda a ignorar la salida.

---

## 3. Preparación del entorno

### 3.1 Variables de entorno

Van en cada shell desde donde se corra. Convienen en el `.zshrc`.

```sh
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
export LD_LIBRARY_PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib"
export CARGO_TARGET_DIR=$HOME/.cache/mana-lite-target
export MANA_MODELS_HOME=/home/visiona/workspace/mana-lite-workspace/mana-lite
```

`MANA_MODELS_HOME` **reancla las rutas relativas** del catálogo de modelos. Las
rutas del catálogo son del estilo `tools/model-tools/artifacts/...`; con la
variable apuntada al checkout, resuelven. Sin ella, resuelven contra el
directorio de trabajo — que suele funcionar si se corre desde la raíz, y suele
romper en cualquier otro lado.

### 3.2 El repositorio no es autocontenido

Depende **por path** de `../inference`, que es otro repositorio
(`e7canasta/mana-inference`). Tienen que estar hermanos:

```
mana-lite-workspace/
├── mana-lite/      ← este repo
└── inference/      ← el runtime ONNX
```

### 3.3 Los pesos

Están gitignoreados. Viven en `tools/model-tools/artifacts/`:

```sh
ls tools/model-tools/artifacts/yolo26-fp16/yolo26s-fp16-320.onnx      # el padre
ls tools/model-tools/artifacts/yoloface-fp16/yolov12s-face-fp16-320.onnx  # el hijo
```

Si falta alguno, el arranque falla con `Config(FileNotFound(...))` y dice cuál.

### 3.4 Verificación previa

Antes de correr nada, la compuerta del código:

```sh
cargo test --workspace --release
```

**Esperado: 437 tests en 22 suites, 0 fallos.**

> ⚠️ **`--workspace` no es opcional.** Sin él se corre sólo el paquete raíz —144
> tests— y los crates de los tiers quedan afuera. El lazo de control vive en
> `mana-control`, con 159 tests que ese comando no toca. En agosto de 2026 un
> defecto que dejó muerta toda la cascada del sistema vivía exactamente ahí.

---

## 4. De dónde sale el video

**La aplicación nunca habla con una cámara.** Habla siempre con **go2rtc**, que
corre en esta misma máquina (`192.168.1.6` **es** este host) escuchando en el
puerto `8554`, y que re-emite tanto cámaras reales como archivos en loop.

Configuración: `/home/visiona/opt/media/go2rtc.yaml`

```yaml
streams:
  clip1:
    - ffmpeg:videos/clip1_gop6.mp4          # archivo, en loop
  home2:
    - rtsp://admin:...@192.168.2.64:554/... # Hikvision real
```

Verificar que está arriba:

```sh
ss -tlnp | grep 8554          # debe listar go2rtc
```

### 4.1 Las dos fuentes, y cuándo usar cada una

| Fuente | URL | Qué tiene | Para qué |
|---|---|---|---|
| `home2` | `rtsp://192.168.1.6:8554/home2` | la habitación real | **control de cadencia**: es la instalación |
| `clip1` | `rtsp://127.0.0.1:8554/clip1` | una persona en cama, en loop | **contenido clínico** |

> ⚠️ **La cámara de la instalación suele estar vacía.** Y un escenario de
> cascada o clínico sin una persona en escena **no prueba lo que dice probar**:
> la compuerta del modelo hijo se cierra por la razón correcta, el FSM se queda
> en `idle`, y todo eso es indistinguible de un sistema roto.
>
> Los peldaños 04, 05 y 06 se corren contra las dos. `clip1` tiene la misma
> cadencia de keyframe (~1/s, el archivo se llama `gop6` por eso) y la misma
> resolución, así que los números de cadencia son comparables.

**Antes de correr un escenario clínico, mirá qué hay en cuadro:**

```sh
ffmpeg -y -rtsp_transport tcp -i "rtsp://127.0.0.1:8554/clip1" \
  -frames:v 1 -update 1 -q:v 3 /tmp/frame.jpg && xdg-open /tmp/frame.jpg
```

Diez segundos, y evita horas de diagnosticar una escena vacía.

### 4.2 El visor

Sólo lo necesita el peldaño 06. Rerun, escuchando donde apunte
`viz.rerun_addr`:

```sh
rerun --port 9876        # en la estación de trabajo, 192.168.1.20 por defecto
```

Para mirar en la misma máquina donde corre el sistema, ver §6.6.

---

## 5. Procedimiento: correr el circuito completo

Siempre **desde la raíz del repositorio**, y siempre a través de `cargo run` —
nunca invocando un binario por ruta fija, porque `CARGO_TARGET_DIR` está
redirigido y `./target/` puede contener un artefacto huérfano de otra época.

```sh
cargo test --workspace --release

timeout 180 cargo run --release -- --config workshop/scenarios/01-ingest-only/mana.toml
./workshop/scenarios/02-ingest-viz/run-variant.sh b-jpeg-native 180
./workshop/scenarios/02-ingest-viz/run-variant.sh a-raw-native 180
timeout 180 cargo run --release -- --config workshop/scenarios/03-ingest-infer/mana.toml
timeout 180 cargo run --release -- --config workshop/scenarios/04-infer-track/mana.toml
timeout 180 cargo run --release -- --config workshop/scenarios/05-clinical/mana.toml
./workshop/scenarios/06-clinical-viz/run-fuente.sh clip1 180
```

### 5.1 Reglas de procedimiento

**Compilá antes de medir.** `cargo run` compila si hace falta, y el compilador
se come la máquina. Una corrida que arranca compilando pierde el primer tercio
de su ventana y ensucia los números. Siempre:

```sh
cargo build --release && timeout 180 cargo run --release -- --config ...
```

**180 segundos como mínimo.** No es capricho:
- el atraso es un fenómeno **por keyframe**, y a ~1 keyframe/s una ventana de
  5 s tiene demasiado pocas muestras para una p95 que signifique algo;
- los temporizadores clínicos son de segundos (`single_confirm_ms = 3000`,
  `empty_confirm_ms = 8000`) y una ventana corta no alcanza para que el FSM
  transicione.

**Una corrida por vez.** Dos procesos compitiendo por CPU invalidan las dos.

**Las salidas van a `workshop/runs/<escenario>/`**, que está fuera de control de
versiones. Un `.jsonl` es evidencia de esa corrida, no un artefacto del
repositorio. Si un número importa, va al README del escenario.

---

## 6. Los seis peldaños, uno por uno

### 6.1 — `01-ingest-only`: RTSP → decode → JSONL

**Qué agrega:** nada, es la base. Sólo ingesta y el lazo ticando en vacío.

```sh
timeout 180 cargo run --release -- --config workshop/scenarios/01-ingest-only/mana.toml
```

**Compuerta:** cadencia de keyframes estable, sin reconexiones, sin frames
corruptos, `dline` en el piso del temporizador.

**Números de referencia** (2026-08-12, `home2`):

```
dline p95   1,6–1,9 ms      missed 0      overruns 0
keyframes   178 / 178 vistos (deriva 0)   reconexiones 0
decode      11 ms promedio  gap 996–1004 ms
```

**Es el control del experimento.** Cualquier peldaño posterior se compara contra
éste, y por eso el transporte no se toca nunca entre escenarios.

---

### 6.2 — `02-ingest-viz`: el bridge de Rerun

**Qué agrega:** el visor, sin ningún modelo.

```sh
./workshop/scenarios/02-ingest-viz/run-variant.sh b-jpeg-native 180
./workshop/scenarios/02-ingest-viz/run-variant.sh a-raw-native 180
```

El script existe para que una corrida sea **una invocación y no la edición
manual de un campo**: dejar un `image_format` flipeado de la corrida anterior
invalida la comparación sin que nada lo avise. Materializa la config efectiva
junto a su salida.

**Compuerta:** una sola línea `viz: connected`, sin churn de reconexión.

**Lo que este peldaño decidió, y que los demás heredan:**

| variante | `viz_pisados` | veredicto |
|---|---|---|
| `b-jpeg-native` | 0 | **es la que se usa** |
| `a-raw-native` | 19 | raw satura el enlace |

Por eso el 06 usa `jpeg`. Esa discusión ya está cerrada acá.

---

### 6.3 — `03-ingest-infer`: un modelo

**Qué agrega:** inferencia, un solo modelo, sin tracking.

```sh
timeout 180 cargo run --release -- --config workshop/scenarios/03-ingest-infer/mana.toml
```

**Este es el peldaño pivote del proyecto.** Midió que la inferencia tarda ~215 ms
y el periodo de scan es 200 ms: **el modelo se come más de un periodo entero**.
Antes del refactor eso bloqueaba el lazo y producía un incumplimiento por
keyframe (`late p95` 101–154 ms). Hoy:

```
dline p95   1,7–6,5 ms   ← la inferencia dejó de cobrárselo al lazo
evid p50    659 ms   max 1235 ms
infer       213 ms promedio (194–251)
```

**Si el atraso diera cero, lo que está mal es la medición, no el sistema.**
Sabemos que el modelo tarda más que un periodo; un `late max` de 0 significaría
que el instrumento no mide lo que dice medir.

---

### 6.4 — `04-infer-track`: tracking y cascada

**Qué agrega:** seguimiento temporal y, con él, el modelo hijo.

Entran juntos porque **no son separables**: la regla del hijo pide una cantidad
exacta de tracks de persona confirmados, así que una cascada con hijo requiere
tracker.

```sh
cargo build --release
timeout 180 cargo run --release -- --config workshop/scenarios/04-infer-track/mana.toml
```

**Es el primer peldaño que ejercita la realimentación.** Hasta el 03 la
directiva existía pero no gobernaba nada: sin tracker no había tracks, y sin
hijo no había quién los consultara.

```
[control]  tracker.current_tracks()  ──► Slot<ControlDirective>
                                              │
[percepción]  la regla del blueprint: ¿exactamente 1 track de persona
              confirmado, de la clase y confianza que pide el hijo?
                 sí → corre face-yolo, recorta sobre ese track
                 no → lo saltea y lo cuenta en skips
```

**Compuertas:**

| Criterio | Qué se espera |
|---|---|
| La regla gobierna | `skips` sube sin exactamente una persona, y cae a 0 cuando la hay |
| El recorte sigue al track | el `roi:` de `face-yolo` **se mueve** entre ventanas |
| Identidad estable | `track_id` no se renumera con una persona quieta |
| Cadencia intacta | `dline` en el piso, `0 missed` |
| Bordes | sin `kf_pisados` |

**Números de referencia** (`clip1`, con persona):

```
evid p50 881 ms / max 1287 ms      dline p95 2,0–4,5 ms
face-yolo skips 8 de 178           kf_pisados 0
4 tracks creados, 172 actualizaciones en 180 s
```

**El costo del segundo modelo: +170 ms de edad de evidencia y nada de cadencia.**

---

### 6.5 — `05-clinical`: la pila completa

**Qué agrega:** zonas, FSM, presencia, ocupancia y reglas de profundidad, sobre
el blueprint que efectivamente se despliega y con **los umbrales de producción**.

```sh
cargo build --release
timeout 180 cargo run --release -- --config workshop/scenarios/05-clinical/mana.toml
```

Un escenario que afloja los tiempos clínicos para pasar no prueba nada.

**Lo que prueba no es que detecte bien** —eso lo prueban los peldaños de abajo—
sino que **la pila completa no le cuesta cadencia al lazo**.

**Números de referencia** (`clip1`):

```
dline p95 1,9–3,0 ms / 5 missed     cycle p95 202 ms / 0 overruns
evid p50 865 ms / max 1272 ms       24 transiciones de FSM
in_bed en 772 de 895 scans          kf_pisados 0, img_pisadas 0
```

**El número que este escenario existe para producir** es `evid: max` contra los
umbrales de `[health]`:

```
evid max        1 272 ms   ← peor caso normal medido
stale_warn_ms   5 000 ms   ← el sistema avisa que la evidencia envejece
data_stale_ms  10 000 ms   ← el sistema se declara ciego
```

Entre "esto ya no es normal" y "dejo de confiar en lo que veo" hay casi nueve
segundos, y en ese intervalo los temporizadores clínicos siguen corriendo sobre
evidencia congelada. Una persona se levanta de la cama y llega al piso en un par
de segundos. **Esa distancia es una decisión clínica que todavía nadie tomó
mirando este número**, porque el número no existía hasta el 2026-08-12.

---

### 6.6 — `06-clinical-viz`: la pila clínica con ojos

**Qué agrega:** ninguna capa de procesamiento. Es el 05 con el visor prendido.

Existe porque los cinco peldaños de abajo prueban que el sistema **sostiene su
contrato temporal**, y ninguno prueba que lo que ve sea razonable. Un `evid: p50`
de 867 ms no dice si la caja está sobre la persona o sobre la mesa de luz.

```sh
rerun --port 9876 &                      # en la estación de trabajo
./workshop/scenarios/06-clinical-viz/run-fuente.sh clip1 180

# para mirar en la misma máquina donde corre:
RERUN_ADDR=127.0.0.1:9876 ./workshop/scenarios/06-clinical-viz/run-fuente.sh clip1 180
```

Toma la fuente como argumento (`clip1` o `home2`) por la razón de §4.1. Guarda
un log y una config por corrida, con marca de tiempo: **una corrida es evidencia
de esa corrida**, y comparar dos es lo primero que se quiere hacer cuando un
número cambia.

**Qué mirar en el visor:**

| Qué | Qué diría que está mal |
|---|---|
| Caja de `detect-fast` | sobre la persona, estable entre keyframes. Si salta o toma muebles, el problema es el detector |
| **Recorte de `face-yolo`** | es el brazo de vuelta del lazo cerrado. **Tiene que seguir a la persona.** Un recorte congelado con la persona moviéndose = la directiva no está llegando |
| ROI estático del padre | `[420,0 1500,1080]`. Si la persona entra y sale de ese rectángulo, la política de recorte está mal calibrada para esa cámara |
| Línea de tiempo de estados | con una persona acostada tiene que dominar `in_bed` |
| Zonas | `config/zones.toml` está calibrado para la instalación. **Contra el clip no coinciden, y es esperable** |

**Este escenario se trae su propio archivo de observabilidad** para que los
eventos de face-dwell lleguen al JSONL y se pueda contrastar lo que se ve contra
lo que el sistema decidió. Declara sólo sus desviaciones respecto del mecanismo.

---

## 7. Cómo se lee el reporte

Cada 5 segundos salen seis líneas por stdout. Esta sección es la más importante
del manual: **saber cuál de estas líneas es causa y cuál es síntoma es la
diferencia entre diagnosticar en diez minutos o en dos días.**

### 7.1 `cycle:` — el periodo

```
cycle:  5.0 Hz — 25 scans in 5s | p95 200ms max 201ms min 199ms | 0 overruns (budget 500ms)
```

Mide **cuánto pasó entre dos scans**. Se autocorrige: un scan que arranca tarde
empuja al siguiente, que arranca de inmediato, y el promedio se mantiene.

> ⚠️ **Por eso `cycle:` se ve sano aunque el lazo esté incumpliendo.** No
> alcanza para decir que el sistema cumple. `overruns` cuenta los scans que
> superaron `health.cycle_budget_ms`.

### 7.2 `dline:` — el atraso

```
dline: 25 deadlines in 5s | late min 0.9ms p50 1.2ms p95 1.8ms max 1.9ms | 0 missed (>5.0ms)
```

Mide **cuánto después de su vencimiento arrancó cada scan**. No se autocorrige.
Es lo que un PLC llama incumplimiento, y es **el eje que importa**.

`missed` cuenta los vencimientos por encima de la tolerancia impresa al lado. La
tolerancia es el piso del temporizador de tokio, no un umbral de política: por
debajo de eso la medición no distingue un incumplimiento del ruido del
instrumento. La distribución va sin recortar, así que el piso queda a la vista.

**Sano hoy: p95 entre 1 y 4 ms.** Antes del refactor eran 101–154 ms.

### 7.3 `evid:` — la edad de la evidencia

```
evid:  25 scans con evidencia in 5s | edad min 312ms p50 712ms p95 1112ms max 1113ms
```

**Es el número clínico.** Las otras líneas dicen si la máquina está sana; ésta
dice si la decisión se tomó sobre algo actual, que es otra pregunta y es la que
le importa a una revisión de incidente.

El piso no es cero y no debería serlo: con keyframes a 1 Hz y un lazo a 5 Hz,
cuatro de cada cinco scans deciden sobre evidencia que ya tenían. Un `p50`
cercano a medio intervalo de keyframe es lo sano. **Lo que hay que mirar es el
`max` contra `health.data_stale_ms`.**

Sin evidencia dice explícitamente `sin evidencia en 5s`, no cuatro ceros — un
cero de edad y una ausencia de dato son lo contrario y se veían igual.

### 7.4 `ingest:` — el campo

```
ingest: 1.0 Hz — 5 keyframes processed (5 seen) in 5s | decode 11ms avg | gap min 996ms ... | cycles 25 | pframes:25, timeouts:89
```

`processed` vs `seen` **se conserva a lo largo de la corrida, no por ventana**:
un keyframe visto al final de una ventana se emite en la siguiente. Lo que
delata una pérdida real es la **deriva acumulada**, o una ventana muerta con
tráfico.

Las banderas al final sólo aparecen cuando el contador es distinto de cero:

| Bandera | Qué significa | ¿Preocupa? |
|---|---|---|
| `pframes`, `timeouts` | tráfico normal con `keyframes_only` | no |
| `dup` | keyframes duplicados suprimidos | no |
| `kf_dropped` | descartados en ingesta | uno aislado, no |
| **`kf_pisados`** | **percepción no dio abasto con la cámara** | **sí** |
| **`img_pisadas`** | **percepción produjo dos evidencias entre dos scans** | **sí** |
| `viz_pisados` | el bridge no llegó a tomar una muestra | no, ver §7.6 |
| `reconnect`, `ssrc`, `rtp` | problemas de red | sí si son sostenidos |

### 7.5 `infer:` y la línea por modelo

```
infer:  2.0 Hz — 10 calls in 5s | 159 (113-209ms) | 10 dets | skips:4, apagados:4
        detect-fast: 1.0 Hz | 5 calls | 200 (196-209ms) | 5/5fr | roi:[420,0 1500,1080]
          face-yolo: 1.0 Hz | 5 calls | 118 (113-122ms) | 5/5fr | roi:[685,161 1005,481]
```

Dos contadores que **no significan lo mismo**, y la distinción cuesta caro:

- **`skips:N`** — el modelo hijo miró la escena y **su regla no aplicó**: no
  había exactamente una persona, o no llegaba a la confianza mínima.
- **`apagados:N`** — el estado del FSM **no pidió ese modelo**. Nunca llegó a
  mirar la escena.

> ⚠️ Sin esa distinción, mover una política al catálogo hace que un modelo deje
> de correr **en silencio**. El silencio es exactamente cómo un defecto que dejó
> muerta toda la cascada se escondió durante meses.

El `roi:` de un hijo **tiene que moverse** si la persona se mueve. Es el
indicador más directo de que el lazo cerrado está gobernando.

### 7.6 `viz_pisados` no es una compuerta

El visor recibe **muestras**, no una cola. Que se pise una significa que el
bridge no llegó a tomarla antes de la siguiente, y eso es la degradación que el
diseño elige: el lazo no espera al visor.

Cuatro corridas idénticas del 06 dieron **0, 37, 97 y 127**. Es dispersión del
lado del consumidor. Lo que sí es constante y es lo que importa: `kf_pisados` e
`img_pisadas` en cero en las cuatro. **La variabilidad no cruza la frontera hacia
el sistema.**

---

## 8. Cómo se lee el JSONL

Sale a `workshop/runs/<escenario>/mana-<fecha>T<hora>.jsonl`, y **rota por
hora**: dos corridas de la misma hora comparten archivo. Para analizar sólo la
última, cortar desde el último `startup`.

### 8.1 Los tipos de evento

| Tipo | Cadencia | ¿Encendido por defecto? | Para qué |
|---|---|---|---|
| `meta` | eventos | sí | arranque, modelos cargados, ciclo de vida de tracks |
| `presence` | por scan | sí | conteo crudo vs confirmado, estado de ocupancia, timers |
| `scene_signals` | por scan | sí | la foto de señales del scan |
| `entity` | por scan | sí | tracks con su bbox |
| `detection` | por keyframe | sí | una por modelo, con `infer_ms` y el recorte |
| `consolidated_detection` | por keyframe | sí | la observación consolidada |
| **`fsm`** | **transiciones** | **sí** | **qué decidió el sistema y por qué** |
| **`zone`** | **transiciones** | **sí** | entradas y salidas de zona |
| `health` | ventana | sí | atraso, ceguera, envejecimiento |
| `face_dwell` | por scan | **no** | diagnóstico: la foto completa por scan |
| `frame` | por frame | no | ruido |

`fsm` y `zone` cuestan 2,1 y 4,6 MB/día contra los ~600 MB/día que ya escriben
los eventos por scan. `face_dwell` cuesta 175 MB/día y por eso queda apagado: lo
enciende el escenario que lo necesita.

### 8.2 Consultas que se usan seguido

```sh
cd workshop/runs/05-clinical
F=$(ls -t *.jsonl | head -1)

# ¿qué decidió el FSM?
grep '"type":"fsm"' $F | python3 -c 'import sys,json
for l in sys.stdin: d=json.loads(l); print(d["from"],"→",d["to"],f"({d[\"dwell_ms\"]}ms)")'

# ¿el tracker está confirmando? (raw vs confirmed)
grep '"type":"presence"' $F | python3 -c 'import sys,json,collections
c=collections.Counter((json.loads(l)["raw_count"], json.loads(l)["confirmed_count"]) for l in sys.stdin)
print(dict(c))'

# ¿el atraso salió al disco?
grep '"event":"scan_deadline"' $F | tail -3

# ¿cuánto tardó cada modelo, frame a frame?
grep '"type":"detection"' $F | python3 -c 'import sys,json
for l in sys.stdin: d=json.loads(l); print(d["model"], d["infer_ms"], d.get("crop"))'
```

> **La consulta más importante cuando un modelo hijo no corre** es la segunda.
> Si `confirmed_count` es 0 mientras `raw_count` es 1, el detector ve a la
> persona y el tracker no la confirma — y ahí está el problema, no en la
> cascada.

---

## 9. Diagnóstico: síntoma → dónde mirar

| Síntoma | Primero mirá | Por qué |
|---|---|---|
| El 04 o el 05 "salen raros" | **`evid:` y `kf_pisados`**, no el atraso | el atraso es síntoma; esos dos son causa |
| Un modelo hijo no corre nunca | **`confirmed_count` en el JSONL de `presence`** | `skips` sólo dice que no corrió, no por qué |
| Un modelo hijo no corre y `skips` es 0 | **`apagados:`** | el estado del FSM no lo pidió; no es la regla |
| El FSM no transiciona | **¿está `fsm_events` encendido?** | ya pasó: el FSM andaba y el instrumento estaba apagado |
| No hay detecciones | **sacá un frame del stream** (§4.1) | la escena vacía y el detector roto se ven igual |
| El recorte del hijo no se mueve | la directiva no está llegando a percepción | es el brazo de vuelta del lazo |
| `cycle:` sano pero algo va mal | **`dline:`** | `cycle` se autocorrige y esconde el incumplimiento |
| `viz_pisados` alto | nada, es dispersión | mientras `kf_pisados` e `img_pisadas` sigan en cero |
| El atraso p95 saltó de golpe | la **fase** del keyframe en la grilla de vencimientos | no es degradación, la fija el GOP de la cámara |
| Números peores de lo esperado | **¿había algo compilando?** | el compilador se come la máquina y arruina la ventana |
| Un track se renumera seguido | ¿es el clip reiniciando su loop? | la persona salta de posición y es esperable |

### 9.1 Modos de falla ya diagnosticados

No los rediagnostiquen:

- **`late p95` con escalones.** No es degradación: es un cambio de **fase**. El
  bloqueo dura ~221 ms y la grilla mide 200 ms; cuánto atraso produce depende de
  dónde cae el keyframe en la grilla, y eso lo fija el GOP de la cámara. Citar
  un `late p95` suelto sin la fase que lo produjo es citar una coincidencia.
- **Zonas que no coinciden con el clip.** `config/zones.toml` está calibrado
  para la instalación. Es esperable.
- **`viz_pisados` disperso.** Ver §7.6.

---

## 10. Las reglas que costaron caro

Cada una salió de un día perdido.

**1. Verificá que la compuerta falle cuando debe fallar.** Dos pasaron en vacío
durante sprints enteros. Una compuerta que nunca falló no es una compuerta
verde: es una compuerta sin probar.

**2. La config no debe declarar lo que el código no honra.** Un knob que se
acepta y se ignora **miente en la revisión**. Pasó con un parámetro de
validación que no validaba, un `min_confidence` inevaluable, un feature que no
apagaba nada, y un `dwell = "-5s"` aceptado como 0.

**3. Y el código no debe decidir lo que ningún catálogo declara.** Es la regla 2
al revés, y costó igual: una compuerta hardcodeada en percepción duplicaba una
regla del blueprint contra otra fuente de datos. Para saber por qué un modelo no
corría había que leer código.

**4. Medí la condición, no un síntoma.**

**5. Antes de diseñar una partición, mirá cuánto es test.** Dos archivos
parecían god files; eran archivos normales con una montaña de tests adentro.

**6. El instrumento no es la medición.** Una línea de reporte puede existir,
verse correcta y no medir lo que dice. Pasó cinco veces:

- `seen` enmascarado con `processed`, que sumaba deriva permanente;
- la tolerancia de incumplimiento puesta por debajo del piso del temporizador;
- cuatro ceros al lado de `0 scans con evidencia`, que se leían como "la
  evidencia tiene 0 ms de edad" — lo contrario de lo que pasaba;
- la edad de la evidencia viajando dentro de un evento apagado por defecto;
- las transiciones del FSM apagadas en el archivo que el escenario incluía, con
  el escenario declarando "transiciones en el JSONL" como criterio.

Ninguna la encontró una revisión de código. Todas las encontró una corrida.
**Antes de creerle a un número, verificá qué lo alimenta, contra qué umbral se
compara, y por dónde sale.**

**7. Un default tiene que ser alcanzable.** `tentative_max_age_ms` valía 600 ms,
más corto que un intervalo de keyframe: con la configuración por defecto ningún
track llegaba a su segunda medición. Todos los escenarios lo pisaban, así que
sólo podía morder a quien escribiera un `mana.toml` mínimo — y le habría dejado
la cascada muerta sin un solo error.

**8. Un contador correcto bajo una cadencia deja de serlo bajo otra.** El
tracker contaba fallos de detección por **tick del lazo** en vez de por
**medición**. Era correcto cuando todo corría en un solo lazo. Al separar las
dos tasas, cuatro de cada cinco ticks pasaron a ser fallos inventados y ningún
track volvió a confirmarse. Ante cualquier contador del tier de control:
**¿cuenta scans o cuenta mediciones?**

---

## 11. Referencias

Este manual alcanza para operar el banco. Para lo demás:

| Documento | Para qué |
|---|---|
| [`HANDOFF.md`](../HANDOFF.md) | estado del proyecto, deuda registrada, qué sigue |
| [`ARCHITECTURE.md`](../ARCHITECTURE.md) | cómo está construido el lazo aislado |
| [`BIGPICTURE.md`](../BIGPICTURE.md) | el encuadre del producto |
| [`docs/adrs/`](../docs/adrs/) | las decisiones y su porqué |
| [`workshop/README.md`](README.md) | índice de la escalera |
| El README de cada escenario | hipótesis, criterios y números medidos de ese peldaño |

### ADRs que conviene leer, en este orden

| ADR | Qué decide |
|---|---|
| [029](../docs/adrs/029-injected-clock.md) | el reloj inyectado: control nunca lee el reloj de pared |
| [032](../docs/adrs/032-scene-signals-as-contract.md) | la tabla de señales como contrato — por qué cambiar cuándo suena una alerta es editar un TOML |
| [033](../docs/adrs/033-isolated-control-loop.md) | el lazo de control aislado |
| [034](../docs/adrs/034-slots-and-queues.md) | **muestras en slots, eventos en colas** — explica qué es un "pisado" y por qué está bien |
| [035](../docs/adrs/035-observability-port.md) | el puerto de observabilidad |

### Estructura del código

| Tier | Qué | Dónde |
|---|---|---|
| T0 · álgebra | sin tasa, sin estado, sin reloj | `mana-id`, `mana-geometry` |
| T1 · campo | sensado y E/S; fallar es normal | `mana-media`, `mana-perception` |
| T2 · programa | cadencia fija, determinista, reloj inyectado | `mana-control` |
| T3 · reporte | JSONL, métricas, Rerun; nunca bloquea el tick | el binario (`src/`) |

**T2 no depende de T1** porque tiene que seguir corriendo cuando T1 murió. Lo
hace cumplir `core/mana-control/Cargo.toml`: tres dependencias. Agregar
`mana-perception` ahí no rompe una convención, **rompe la compilación**.

### Documentacion activa

Los contratos y decisiones vigentes estan indexados en `docs/README.md`. Las
notas historicas viven bajo `docs/archive/` y no deben usarse como instrucciones
operativas.
