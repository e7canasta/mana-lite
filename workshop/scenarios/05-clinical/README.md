# 05 — La pila clínica completa

## Hipótesis

**La pila completa no le cuesta cadencia al lazo.**

Zonas, FSM y presencia encendidos sobre el blueprint que efectivamente se
despliega. Si el atraso se mantiene en el piso del temporizador con todo
prendido, el aislamiento del lazo se sostiene de punta a punta y no sólo con
inferencia.

Lo que este escenario **no** prueba es que el sistema detecte o clasifique bien.
Eso lo prueban los peldaños de abajo. Acá se prueba que la máquina sostiene su
contrato temporal mientras toma decisiones clínicas.

## Qué está activo

Todo. No queda ninguna capa por agregar.

|Capa|Estado|
|---|---|
|Ingesta, inferencia, tracking|activos|
|Zonas|**activas** — `config/zones.toml`|
|FSM|**activo** — `config/blueprints/detect-room-face/fsm.toml`|
|Presencia y ocupancia|**activas**|
|Reglas de profundidad|activas — `config/depth-rules.toml`|
|Rerun|apagado — se homologa en el 02|

**Los umbrales son los de producción a propósito.** Un escenario que afloja los
tiempos clínicos para pasar no prueba nada.

## Cómo correr

```sh
timeout 180 cargo run --release -- --config workshop/scenarios/05-clinical/mana.toml
```

180 s como mínimo: los temporizadores clínicos son de segundos
(`single_confirm_ms = 3000`, `empty_confirm_ms = 8000`) y una ventana corta no
alcanza para que el FSM transicione.

## Criterios de aceptación

|Criterio|Qué se espera|
|---|---|
|**Cadencia bajo carga completa**|`dline:` en el piso del temporizador y `0 missed`. Es el criterio principal|
|Presupuesto|`0 overruns`, `cycle` p95 en ~200 ms|
|El FSM gobierna|transiciones en el JSONL (`"type":"fsm"`) coherentes con lo que pasa en la escena|
|Los catálogos compilan|arranque sin error: si `zones.toml` o el FSM tuvieran una referencia rota, el bootstrap falla|
|Bordes|`kf_pisados` e `img_pisadas` en cero|

## El número que este escenario existe para producir

```
evid:  N scans con evidencia | edad min ..ms p50 ..ms p95 ..ms max ..ms
```

**Es el primer escenario donde esa línea significa algo**, porque es el primero
donde hay un FSM decidiendo sobre esa evidencia con temporizadores corriendo.

Lo que hay que mirar es el `max` contra los umbrales de `[health]`:

```
data_stale_ms  = 10 000    cuándo el sistema se declara ciego
stale_warn_ms  =  5 000    cuándo avisa que la evidencia envejece
```

Entre el peor caso normal y la declaración de ceguera hay casi nueve segundos, y
en ese intervalo los temporizadores clínicos siguen corriendo sobre evidencia
congelada. **Esa distancia es una decisión clínica que todavía nadie tomó
mirando este número**, porque el número no existía hasta el 2026-08-12. Ver
`HANDOFF.md` §9.

### Lo que sería un hallazgo

**`evid: max` acercándose a `data_stale_ms`** con la cámara sana. Significaría
que la evidencia no se está refrescando por una razón que no es la red, y que el
umbral de ceguera está midiendo otra cosa que lo que se cree.

## Números medidos

<!-- Completar con la corrida. -->

|Corrida|`evid` p50/max|`dline` p95 / missed|`cycle` p95 / overruns|transiciones FSM|
|---|---|---|---|---|
| | | | | |
