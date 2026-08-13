# Onboarding — primer día y homologación de versión

> **Para quién es esto.** Para un o una ingeniera que llega hoy a `mana-lite` y
> cuya primera tarea es **probar la versión**: correr el banco de escenarios del
> workshop con el protocolo de `workshop/README.md`, decidir si cada escenario
> pasa o no, y dejar evidencia que otra persona pueda leer.
>
> *Actualizado: 2026-08-12, con la escalera corrida y verde en `dev-refactor`.*

Leé esto primero, [`MANUAL.md`](MANUAL.md) después. Este documento es el día uno
y el protocolo; el manual es la referencia operativa completa del banco.

---

## Índice

1. [El trato](#1-el-trato-lo-que-sé-al-cerrar-la-primera-semana)
2. [El sistema en cinco minutos](#2-el-sistema-en-cinco-minutos)
3. [Por qué hay una escalera y no un solo `mana.toml`](#3-por-qué-hay-una-escalera-y-no-un-solo-manatoml)
4. [Preparación del entorno — día uno](#4-preparación-del-entorno--día-uno)
5. [De dónde sale el video](#5-de-dónde-sale-el-video)
6. [El protocolo de prueba de versión](#6-el-protocolo-de-prueba-de-versión)
7. [Los seis peldaños, con sus compuertas y números](#7-los-seis-peldaños-con-sus-compuertas-y-números)
8. [Cómo se lee el reporte](#8-cómo-se-lee-el-reporte)
9. [Cómo se lee el JSONL](#9-cómo-se-lee-el-jsonl)
10. [Versión en rojo: triage](#10-versión-en-rojo-triage)
11. [Cerrar la homologación](#11-cerrar-la-homologación)
12. [La primera semana, día por día](#12-la-primera-semana-día-por-día)

---

## 1. El trato: lo que sé al cerrar la primera semana

Al terminar este onboarding podés:

1. Correr los seis escenarios en orden, sin romper ninguna regla de
   procedimiento.
2. Leer las seis líneas del reporte sabiendo **cuál es causa y cuál es síntoma**.
3. Decidir compuerta por compuerta si la versión pasa o no, comparando contra
   los números de referencia del §7.
4. Cuando algo está mal, ubicar la capa sin rediagnosticar modos de falla ya
   conocidos (§10).
5. Dejar la evidencia que el proyecto considera una homologación: números
   medidos, veredicto y el `.jsonl` de cada corrida (§11).

**La regla que gobierna todo:** no se avanza al peldaño siguiente con el
anterior en rojo. La escalera existe para que una falla aísle una sola capa;
romper el orden destruye esa propiedad.

---

## 2. El sistema en cinco minutos

`mana-lite` es **un PLC cuyo dispositivo de campo es una cámara**. No es una
aplicación de visión con lógica adentro: es un controlador de cadencia fija que
resulta tener un sensor óptico. Corre a **dos tasas distintas**:

| | El campo (T1) | El programa (T2) |
|---|---|---|
| Qué es | RTSP → decode → ONNX | tracker → presencia → ocupancia → zonas → FSM → salud |
| A qué velocidad | la que pueda; ~1 keyframe/s | cadencia fija, 5 Hz |
| Puede fallar | sí, es normal | **no**: tiene que emitir salida en cada tick |

Entre los dos hay **un solo objeto**: la imagen de proceso (`ProcessImage`),
congelada y fechada. El programa nunca espera al campo. Si la inferencia tarda
más que un periodo de scan, el lazo sigue ticando sobre la evidencia anterior y
esa evidencia envejece **de forma medible**. Ese envejecimiento es la edad de
la evidencia (`evid:` en el reporte), y es el único número con consecuencia
clínica: una decisión tomada sobre una imagen vieja es una decisión sobre algo
que ya no está pasando.

**La pregunta que ubica cualquier cosa en este repositorio:**

> *Si la entrada nunca vuelve a llegar, ¿esto tiene que seguir produciendo
> salida correcta en cada tick?*
>
> Sí → tier de programa. No → tier de campo.

El vocabulario mínimo, en una tabla:

| Término | Qué es |
|---|---|
| **scan / tick** | una iteración del lazo de control, cada 200 ms |
| **keyframe** | un cuadro I del RTSP; la única entrada que se decodifica |
| **evidencia** | lo que percepción produjo sobre un keyframe |
| **edad de la evidencia** | cuánto hace que se produjo lo que este scan usa para decidir |
| **atraso (`dline`)** | cuánto después de su vencimiento arrancó un scan |
| **periodo (`cycle`)** | cuánto pasó entre dos scans, eje distinto del atraso |
| **muestra pisada** | un dato sobrescrito antes de que alguien lo tomara; la degradación correcta para una muestra |
| **cascada** | un modelo hijo que corre sobre el recorte producido por un modelo padre |
| **directiva** | lo que control publica para que percepción sepa qué correr y dónde mirar; es el brazo de vuelta del lazo cerrado |

El caso clínico que corre hoy es prevención de caídas de cama:
`idle → searching → in_bed / detected / edge / other → exiting`.

---

## 3. Por qué hay una escalera y no un solo `mana.toml`

Un `mana.toml` de producción tiene ingesta, inferencia, tracking, zonas, FSM y
visualización activos a la vez. Cuando algo se comporta raro, **todas son
sospechosas**. La escalera del workshop existe para que en cada punto haya como
mucho una: cada escenario agrega **una capa y sólo una** respecto del anterior.

> Si el 04 falla y el 03 estaba verde, el defecto está en lo que el 04 agregó.
> No hay que buscarlo en todo el pipeline.

De ahí salen tres reglas de operación:

1. **Se corren en orden.** No se avanza con el anterior en rojo.
2. **El criterio de aceptación se escribe antes de correr.** Un escenario sin
   criterio previo no es una homologación: es mirar logs y decidir después qué
   contaba como éxito. Por eso cada escenario declara su compuerta en su
   `README.md` — y por eso vos no las escribís: las verificás.
3. **Un "dio bien" no sirve.** Cada README tiene una tabla de números medidos
   con la corrida que los produjo. Tu corrida tiene que producir números.

Qué es una compuerta (y qué no): un criterio **verificable sobre la salida**,
pasa/no pasa. No toda observación es compuerta. `viz_pisados` es el ejemplo que
más confunde: varía entre 0 y 130 en corridas idénticas y no significa nada
malo; es una observación que se registra, no una compuerta (ver §7.6 del
manual). Confundir los dos tipos hace que la gente aprenda a ignorar la salida.

---

## 4. Preparación del entorno — día uno

### 4.1 Variables de entorno

Van en cada shell desde donde se corra. Convienen en el `.zshrc`:

```sh
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
export LD_LIBRARY_PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib"
export CARGO_TARGET_DIR=$HOME/.cache/mana-lite-target
export MANA_MODELS_HOME=/home/visiona/workspace/mana-lite-workspace/mana-lite
```

`MANA_MODELS_HOME` **reancla las rutas relativas** del catálogo de modelos
(`tools/model-tools/artifacts/...`) al checkout. Sin ella resuelven contra el
directorio de trabajo: suele funcionar desde la raíz y suele romper en
cualquier otro lado. Si tu checkout vive en otra ruta, apuntala a tu checkout.

### 4.2 El repositorio no es autocontenido

Depende **por path** de `../inference`, otro repositorio
(`e7canasta/mana-inference`). Tienen que estar hermanos:

```
mana-lite-workspace/
├── mana-lite/      ← este repo
└── inference/      ← el runtime ONNX
```

### 4.3 Los pesos

Están gitignoreados. Viven en `tools/model-tools/artifacts/`:

```sh
ls tools/model-tools/artifacts/yolo26-fp16/yolo26s-fp16-320.onnx        # el padre
ls tools/model-tools/artifacts/yoloface-fp16/yolov12s-face-fp16-320.onnx # el hijo
```

Si falta alguno, el arranque falla con `Config(FileNotFound(...))` y dice cuál.

### 4.4 El video local

La aplicación nunca habla con una cámara: habla con **go2rtc** en esta misma
máquina (192.168.1.6 **es** este host), puerto `8554`, configurado en
`/home/visiona/opt/media/go2rtc.yaml`. Verificá que esté arriba antes de
correr nada:

```sh
ss -tlnp | grep 8554        # debe listar go2rtc
```

### 4.5 La compuerta del código

Antes de correr un solo escenario:

```sh
cargo test --workspace --release
```

**Esperado: 437 tests en 22 suites, 0 fallos.**

> ⚠️ **`--workspace` no es opcional.** Sin él se corre sólo el paquete raíz
> —144 tests— y quedan afuera los crates de los tiers. El lazo de control vive
> en `mana-control`, con 159 tests que ese comando no toca. En agosto de 2026
> un defecto que dejó muerta toda la cascada vivía exactamente ahí, años-luz de
> lo que una revisión de código alcanza a ver.

---

## 5. De dónde sale el video

Hay **dos fuentes**, y la elección entre ellas es parte del protocolo:

| Fuente | URL | Qué tiene | Para qué |
|---|---|---|---|
| `home2` | `rtsp://192.168.1.6:8554/home2` | la habitación real | **control de cadencia**: es la instalación |
| `clip1` | `rtsp://127.0.0.1:8554/clip1` | una persona en cama, en loop | **contenido clínico** |

> ⚠️ **La cámara de la instalación suele estar vacía.** Y un escenario de
> cascada o clínico sin una persona en escena **no prueba lo que dice probar**:
> la cascada no corre, el FSM se queda en `idle`, y una compuerta cerrada por la
> razón correcta no se distingue de una rota.
>
> Los peldaños 04, 05 y 06 se corren contra **las dos** fuentes. `clip1` tiene
> la misma cadencia de keyframe (~1/s, el archivo se llama `gop6` por eso) y la
> misma resolución que la cámara, así que los números de cadencia son
> comparables entre fuentes.

**Antes de correr un escenario clínico, mirá qué hay en cuadro:**

```sh
ffmpeg -y -rtsp_transport tcp -i "rtsp://127.0.0.1:8554/clip1" \
  -frames:v 1 -update 1 -q:v 3 /tmp/frame.jpg && xdg-open /tmp/frame.jpg
```

Diez segundos, y evita horas de diagnosticar una escena vacía.

**El visor** (sólo lo necesita el 06). Rerun, escuchando donde apunte
`viz.rerun_addr`:

```sh
rerun --port 9876        # en la estación de trabajo, 192.168.1.20 por defecto
```

Para mirar en la misma máquina donde corre el sistema, `RERUN_ADDR=127.0.0.1:9876`
(ver §6.6 del manual).

---

## 6. El protocolo de prueba de versión

Siempre **desde la raíz del repositorio**, y siempre a través de `cargo run` —
nunca invocando un binario por ruta fija, porque `CARGO_TARGET_DIR` está
redirigido y `./target/` puede contener un artefacto huérfano de otra época.

### 6.1 Las tres reglas de procedimiento

1. **Compilá antes de medir.** `cargo run` compila si hace falta, y el
   compilador se come la máquina: una corrida que arranca compilando pierde el
   primer tercio de su ventana y ensucia los números. Si hubo cambios de
   código, primero `cargo build --release`, y recién después la primera corrida.
2. **180 segundos como mínimo.** El atraso es un fenómeno **por keyframe** (a
   ~1 keyframe/s, una ventana de 5 s es poco para una p95 que signifique algo),
   y los temporizadores clínicos son de segundos (`single_confirm_ms = 3000`,
   `empty_confirm_ms = 8000`): una ventana corta no alcanza para que el FSM
   transicione.
3. **Una corrida por vez.** Dos procesos compitiendo por CPU invalidan las dos
   corridas. Nada de "aprovecho y corro dos".

### 6.2 El circuito completo

```sh
# 0 — la compuerta del código
cargo test --workspace --release

# 1 — ingesta pura (control del experimento)
timeout 180 cargo run --release -- --config workshop/scenarios/01-ingest-only/mana.toml

# 2 — el bridge de Rerun, dos variantes (cada invocación materializa su config)
./workshop/scenarios/02-ingest-viz/run-variant.sh b-jpeg-native 180
./workshop/scenarios/02-ingest-viz/run-variant.sh a-raw-native 180

# 3 — inferencia, un modelo (el peldaño pivote)
timeout 180 cargo run --release -- --config workshop/scenarios/03-ingest-infer/mana.toml

# 4 — tracking y cascada: contra la cámara (control) y contra el clip (contenido)
timeout 180 cargo run --release -- --config workshop/scenarios/04-infer-track/mana.toml
#   y contra el clip, con la config efectiva materializada (ver 6.3)

# 5 — la pila clínica completa: contra las dos fuentes
timeout 180 cargo run --release -- --config workshop/scenarios/05-clinical/mana.toml
#   y contra el clip, igual que el 04

# 6 — el 05 con visor
rerun --port 9876 &
./workshop/scenarios/06-clinical-viz/run-fuente.sh clip1 180
#   para mirar en esta máquina:
RERUN_ADDR=127.0.0.1:9876 ./workshop/scenarios/06-clinical-viz/run-fuente.sh clip1 180
```

### 6.3 Correr el 04 y el 05 contra el clip

Los configs del 04 y del 05 apuntan a `home2` por defecto. Para la corrida
clínica hay que materializar la config efectiva con la URL del clip — el mismo
principio que `run-fuente.sh` usa con el 06, para que cada corrida deje a su
lado **exactamente** lo que se corrió, sin ediciones manuales que después hay
que reconstruir:

```sh
R=workshop/runs/04-infer-track; mkdir -p "$R"
sed -e 's#rtsp://192.168.1.6:8554/home2#rtsp://127.0.0.1:8554/clip1#' \
    -e '/^username = "admin"$/d' -e '/^password = ""$/d' \
    workshop/scenarios/04-infer-track/mana.toml > "$R/mana-clip1.toml"
timeout 180 cargo run --release -- --config "$R/mana-clip1.toml"
```

(El clip local en 127.0.0.1 no pide credenciales; dejarlas mentiría sobre la
fuente. Hacelo igual para el 05 con su directorio y su config.)

### 6.4 Dónde quedan las salidas

Todo sale a `workshop/runs/<escenario>/`, **fuera de control de versiones**. Un
`.jsonl` es evidencia de esa corrida, no un artefacto del repositorio. Si un
número importa, al final del protocolo va al README del escenario (§11).

---

## 7. Los seis peldaños, con sus compuertas y números

Para el detalle (qué config está activa, modos de falla, criterios completos):
el `README.md` de cada escenario. Acá están el comando, la compuerta y los
números de referencia de la corrida verde del 2026-08-12.

### 7.1 — `01-ingest-only`: RTSP → decode → JSONL

**Qué agrega:** nada, es la base. Ingesta y el lazo ticando en vacío.

**Compuerta:** cadencia de keyframes estable (`processed == seen`), sin
reconexiones, sin frames corruptos, `dline` en el piso del temporizador, sin
`stale` ni `blind` en el JSONL.

**Números de referencia** (`home2`):

```
dline p95   1,6–1,9 ms      missed 0      overruns 0
keyframes   178 / 178 vistos (deriva 0)   reconexiones 0
decode      11 ms promedio  gap 996–1004 ms
```

**Es el control del experimento.** Todo peldaño posterior se compara contra
éste; por eso el transporte no se toca nunca entre escenarios.

> Habilitá la búsqueda del JSONL de este peldaño: un `stale` que aparece y se
> queda es la firma del drenaje de keyframes, que ya se corrigió una vez
> (README del 01) y es exactamente el tipo de cosa que reaparece.

### 7.2 — `02-ingest-viz`: el bridge de Rerun

**Qué agrega:** el visor, sin ningún modelo.

**Compuerta:** una sola línea `viz: connected`, sin churn de reconexión; layout
del viewer estable.

**Lo que este peldaño decidió, y que los demás heredan:**

| variante | `viz_pisados` | veredicto |
|---|---|---|
| `b-jpeg-native` | 0 | **es la que se usa** |
| `a-raw-native` | 19 | raw satura el enlace |

Por eso el 06 usa jpeg. **Si esta relación se invierte en tu corrida, es un
hallazgo de versión**, no un capricho: algo cambió en el costo del bridge.

### 7.3 — `03-ingest-infer`: un modelo

**Qué agrega:** inferencia, un solo modelo, sin tracking.

**Compuerta:** el atraso es **visible** — `late max` del orden de la latencia
de inferencia (~215 ms), no 0 — y la cadencia media sigue en ~200 ms.

**Números de referencia:**

```
dline p95   1,7–6,5 ms   ← la inferencia dejó de cobrárselo al lazo
evid p50    659 ms   max 1235 ms
infer       213 ms promedio (194–251)
```

> **Si el atraso diera cero, lo que está mal es la medición, no el sistema.**
> El modelo tarda más que un periodo; un `late max` de 0 significaría que el
> instrumento no mide lo que dice medir. Nunca "arregles" este peldaño para
> que dé más lindo — la compuerta del 03 pide atraso visible.

### 7.4 — `04-infer-track`: tracking y cascada

**Qué agrega:** seguimiento temporal y, con él, el modelo hijo. Entran juntos
porque no son separables: la regla del hijo pide una cantidad exacta de tracks
de persona confirmados, así que cascada con hijo requiere tracker.

**Compuertas:**

| Criterio | Qué se espera |
|---|---|
| La regla gobierna | `skips` de `face-yolo` **sube** sin exactamente una persona (corrida `home2` vacía) y baja cerca de 0 con una persona confirmada (`clip1`) |
| El recorte sigue al track | el `roi:` de `face-yolo` **se mueve** entre ventanas |
| Identidad estable | `track_id` no se renumera con una persona quieta |
| Cadencia intacta | `dline` en el piso, `0 missed` |
| Bordes | sin `kf_pisados` |

**La comparación que importa es 04 contra 03:** el único cambio de config es
`track` y el blueprint; todo lo demás es idéntico. Cualquier diferencia
pertenece a esas dos cosas.

**Números de referencia** (2026-08-12):

| Corrida | `evid` p50/max | `dline` p95 | `face-yolo` skips | `kf_pisados` |
|---|---|---|---|---|
| `home2`, escena vacía | 674 / 1227 ms | 2,0–7,6 ms | 178 de 178 | 0 |
| `clip1`, con persona | 881 / 1287 ms | 2,0–4,5 ms | **8 de 178** | 0 |

**El costo del segundo modelo: +170 ms de edad de evidencia y nada de
cadencia.** Si `evid: p50` cae respecto del 03, sospechá del instrumento.

**Lo que sería un hallazgo:** `kf_pisados` subiendo — dos modelos que ya no
entran en el intervalo de keyframe. Es degradación correcta, pero es el primer
síntoma de que la cascada no escala en este hardware, y hay que verlo acá y no
en producción.

### 7.5 — `05-clinical`: la pila completa

**Qué agrega:** zonas, FSM, presencia, ocupancia y reglas de profundidad, sobre
el blueprint que efectivamente se despliega y con **los umbrales de
producción**. Un escenario que afloja los tiempos clínicos para pasar no prueba
nada.

**Compuertas:**

| Criterio | Qué se espera |
|---|---|
| **Cadencia bajo carga completa** | `dline:` en el piso del temporizador y `0 missed` — es el criterio principal |
| Presupuesto | `0 overruns`, `cycle` p95 en ~200 ms |
| El FSM gobierna | transiciones en el JSONL (`"type":"fsm"`) coherentes con la escena — con persona en cama, dominar `in_bed` |

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

### 7.6 — `06-clinical-viz`: la pila clínica con ojos

**Qué agrega:** ninguna capa de procesamiento. Es el 05 con el visor prendido.
Existe porque los cinco de abajo prueban que el sistema **sostiene su contrato
temporal** y ninguno prueba que lo que ve sea **razonable**.

**Compuertas:** una sola conexión al visor; `dline` en el piso y `0 missed`;
`0 overruns`; **`kf_pisados` e `img_pisadas` en cero** — el visor no le cuesta
evidencia al sistema.

**Lo que NO es compuerta (y por qué):** `viz_pisados`. El visor recibe
muestras, no una cola; que se pise una es la degradación correcta — el lazo no
espera al visor. Cuatro corridas idénticas dieron 0, 37, 97 y 127. **La
variabilidad no cruza la frontera hacia el sistema**: lo que importa es que
`kf_pisados` e `img_pisadas` sigan en cero.

**Qué mirar en el visor** (el 06 corre con `face_dwell` encendido, su propio
archivo de observabilidad):

| Qué | Qué diría que está mal |
|---|---|
| Caja de `detect-fast` | sobre la persona, estable entre keyframes. Si salta o toma muebles, es el detector |
| **Recorte de `face-yolo`** | es el brazo de vuelta del lazo cerrado. **Tiene que seguir a la persona.** Un recorte congelado con la persona moviéndose = la directiva no está llegando |
| Línea de tiempo de estados | con una persona acostada tiene que dominar `in_bed` |
| Zonas | `config/zones.toml` está calibrado para la instalación; **contra el clip no coinciden, y es esperable** — no lo rediagnostiques |

---

## 8. Cómo se lee el reporte

Cada 5 segundos salen seis líneas por stdout. **Saber cuál es causa y cuál es
síntoma es la diferencia entre diagnosticar en diez minutos o en dos días.**

| Línea | Mide | Rol | Sano |
|---|---|---|---|
| `cycle:` | cuánto pasó entre dos scans | **síntoma que se autocorrige** — se ve sano aunque el lazo incumpla | p95 ~200 ms, `0 overruns` (budget 500 ms) |
| `dline:` | cuánto después de su vencimiento arrancó cada scan | **el eje que importa — es el incumplimiento del PLC** | p95 entre 1 y 4 ms, `0 missed` |
| `evid:` | edad de la evidencia, por scan | **el número clínico** | `max` lejos de `data_stale_ms` |
| `ingest:` | el campo: cadencia, decode, deriva | causa cuando el campo es el problema | `processed` ≈ `seen` sostenido |
| `infer:` (+ línea por modelo) | llamadas, latencia, detecciones, roi | la foto de percepción | roi del hijo moviéndose |
| banderas | `kf_pisados`, `img_pisadas`, `viz_pisados`… | causa vs dispersión | los dos primeros **en 0 siempre**; el tercero no es compuerta |

Dos distinciones que se pagan caras:

- **`cycle:` sano no prueba cumplimiento.** Un scan que arranca tarde empuja al
  siguiente y el promedio se mantiene; sólo `overruns` y `dline` cuentan el
  incumplimiento.
- **`skips` ≠ `apagados`.** `skips:N` dice que el hijo miró la escena y su regla
  no aplicó (no había exactamente una persona o no llegaba a la confianza
  mínima). `apagados:N` dice que el FSM ni siquiera pidió ese modelo. Sin esa
  distinción, un modelo que deja de correr por una política lo hace **en
  silencio** — que es exactamente como un defecto que mató toda la cascada se
  escondió durante meses.

El piso de `evid` no es cero y no debería serlo: con keyframes a ~1 Hz y un
lazo a 5 Hz, cuatro de cada cinco scans deciden sobre evidencia que ya tenían.
Un `p50` cercano a medio intervalo de keyframe es lo sano; lo que hay que
mirar es el `max` contra `health.data_stale_ms`. Sin evidencia la línea dice
explicitamente `sin evidencia en 5s`, no cuatro ceros.

---

## 9. Cómo se lee el JSONL

Sale a `workshop/runs/<escenario>/mana-<fecha>T<hora>.jsonl`, y **rota por
hora**: dos corridas de la misma hora comparten archivo. Para analizar sólo la
última, cortar desde el último `startup`.

Los tipos de evento, con los que vas a vivir:

| Tipo | Cadencia | Para qué |
|---|---|---|
| `meta` | eventos | arranque, modelos cargados, ciclo de vida de tracks |
| `presence` | por scan | conteo crudo vs confirmado, ocupancia, timers |
| `entity` | por scan | tracks con su bbox y `track_id` |
| `detection` | por keyframe | una por modelo, con `infer_ms` y el recorte |
| **`fsm`** | transiciones | **qué decidió el sistema y por qué** |
| **`zone`** | transiciones | entradas y salidas de zona |
| `health` | ventana | atraso, ceguera, envejecimiento |
| `face_dwell` | por scan | encendido sólo por el 06 |

`fsm` y `zone` cuestan 2,1 y 4,6 MB/día contra los ~600 MB/día de los eventos
por scan; `face_dwell` cuesta 175 MB/día y por eso queda apagado de fábrica.

Consultas que se usan seguido:

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
> persona y el tracker no la confirma — el problema está ahí, no en la cascada.
> `skips` sólo dice que el hijo no corrió; no dice por qué.

---

## 10. Versión en rojo: triage

Cuando un peldaño falla, la escalera te dijo **dónde** está el defecto: en la
capa que ese peldaño agregó respecto del anterior. La tabla de diagnóstico:

| Síntoma | Primero mirá | Por qué |
|---|---|---|
| El 04 o el 05 "salen raros" | **`evid:` y `kf_pisados`**, no el atraso | el atraso es síntoma; esos dos son causa |
| Un modelo hijo no corre nunca | **`confirmed_count`** en el JSONL de `presence` | `skips` sólo dice que no corrió, no por qué |
| Un modelo hijo no corre y `skips` es 0 | **`apagados:`** | el FSM no lo pidió; no es la regla |
| El FSM no transiciona | ¿está `fsm_events` encendido? | ya pasó: el FSM andaba y el instrumento estaba apagado |
| No hay detecciones | **sacá un frame del stream** (§5) | escena vacía y detector roto se ven igual |
| El recorte del hijo no se mueve | la directiva no está llegando a percepción | es el brazo de vuelta del lazo |
| `cycle:` sano pero algo va mal | **`dline:`** | `cycle` se autocorrige y esconde el incumplimiento |
| `viz_pisados` alto | nada, es dispersión | mientras `kf_pisados` e `img_pisadas` sigan en cero |
| El atraso p95 saltó de golpe | la **fase** del keyframe en la grilla | no es degradación; la fija el GOP de la cámara |
| Números peores de lo esperado | **¿había algo compilando?** | el compilador arruina la ventana de medición |
| Un track se renumera seguido | ¿es el clip reiniciando su loop? | la persona salta de posición; es esperable |

### Modos de falla ya diagnosticados — no se rediagnostican

- **`late p95` con escalones.** No es degradación: es un cambio de **fase**. El
  bloqueo dura ~221 ms y la grilla mide 200 ms; cuánto atraso produce depende
  de dónde cae el keyframe en la grilla, y eso lo fija el GOP de la cámara.
  Citar un `late p95` suelto sin la fase que lo produjo es citar una
  coincidencia.
- **Zonas que no coinciden con el clip.** `config/zones.toml` está calibrado
  para la instalación. Es esperable.
- **`viz_pisados` disperso.** Ver §7.6.
- **El drenaje de keyframes** (`0 processed (N seen)`, `stale`, después
  `blind`, todo el resto sano). Ya se corrigió una vez en el 01; si reaparece,
  la causa y la prueba están documentadas en el README del 01.

**Regla de oro:** antes de creerle a un número, verificá qué lo alimenta,
contra qué umbral se compara y por dónde sale. Cinco veces en la historia del
proyecto una línea de reporte existía, se veía correcta y no medía lo que
decía (ver `MANUAL.md` §10, regla 6). Ninguna la encontró una revisión de
código; todas las encontró una corrida.

---

## 11. Cerrar la homologación

Una corrida green no cierra nada por sí sola. Cierra el **registro**:

1. **Por escenario:** compuertas con su veredicto (pasa/no pasa) y los números
   medidos de **tu** corrida — no los de referencia del README.
2. **Los números que importan van al README del escenario**, con la fecha y la
   fuente (`home2` o `clip1`). Si un número cambió respecto de la referencia,
   eso es lo que se discute, no el número en sí.
3. **La evidencia queda en `workshop/runs/<escenario>/`**: el `.jsonl` y, donde
   el escenario la materializa, la config efectiva y el log con marca de
   tiempo. No va a git — es evidencia de esa corrida.
4. **La versión se declara homologada o no** con la lista de compuertas
   pasa/no pasa, en el orden de la escalera. Si hay algo en rojo, la lista
   dice en qué peldaño y con qué síntoma (usar la tabla del §10).

Si además hubo cambios de código durante la homologación (corregir para pasar
no es homologar: las compuertas se corren sobre la versión tal cual), el
registro de la entrega sigue el estilo de `HANDOFF.md` §3: el diff completo, la
salida literal de cada comando de compuerta, y las decisiones que se desviaron
del plan con su razón.

---

## 12. La primera semana, día por día

| Día | Qué | Cómo saber que está bien |
|---|---|---|
| 1 | Entorno (§4): env vars, hermanos, pesos, go2rtc; **compuerta del código** | `cargo test --workspace --release` → 437 tests, 0 fallos |
| 2 | `MANUAL.md` entero + escenario 01 | 01 verde contra `home2`; números en el rango del §7.1 |
| 3 | Escenarios 02 y 03 | 02: 1 conexión, jpeg vs raw como en la tabla; 03: atraso visible |
| 4 | Escenario 04 contra las dos fuentes | `skips` sube vacío / cae con persona; roi que se mueve |
| 5 | Escenarios 05 y 06 | cadencia en el piso con todo encendido; `kf_pisados`/`img_pisadas` en 0; `in_bed` dominando en el visor |
| 6 | Triage de lo que haya quedado rojo + cierre (§11) | registro completo; decís pasa/no pasa por compuerta |

Después de cerrar, la lectura ordenada para entender *por qué* está hecho así:

1. `workshop/MANUAL.md` §6 y §10 (los peldaños y las reglas que costaron caro).
2. `docs/adrs/033` (lazo aislado), `034` (slots y colas — qué es un "pisado"),
   `035` (puerto de observabilidad), `029` (reloj inyectado), `032` (la tabla
   de señales como contrato).
3. `ARCHITECTURE.md` y `HANDOFF.md` (donde está la deuda abierta: §11 tiene las
   únicas cosas con consecuencia clínica pendientes).

La documentacion historica ya no forma parte del arbol activo. Para contratos y
decisiones usa `docs/README.md`, `docs/adrs/`, `docs/specs/` y
`docs/subprojects/`. No mezclar notas archivadas con instrucciones operativas.
