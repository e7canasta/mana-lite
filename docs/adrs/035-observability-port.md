# ADR-035: Observabilidad necesita un puerto, como los otros subsistemas

**Status:** Superseded en parte por ADR-033 — ver *Qué pasó realmente*
**Date:** 2026-08-11

## Qué cambia para el producto

Hoy **la visualización de depuración puede degradar el sistema en producción**.
No en teoría: un scan de 8,2 segundos, medido, con 8 keyframes descartados,
porque el enlace hacia el visor se saturó.

Un ingeniero que abre Rerun para mirar qué está pasando en un cuarto puede,
sin saberlo, retrasar la decisión clínica sobre ese cuarto.

Después de esto la visualización es un consumidor más: si no da abasto, pierde
frames y lo informa, pero no puede tocar el lazo de control. Y como efecto
secundario del mismo cambio, la observabilidad pasa a ser **sustituible en
tests**: se puede verificar qué se emite sin levantar un visor.

## Contexto

`docs/wiki/1-overview.md` declara cuatro subsistemas: Ingesta, Inferencia,
Control y Observabilidad. Los tres primeros hablan por puertos declarados
—- `SceneSample`, `ProcessImage`, `SceneEvent`. **Observabilidad es el único sin
puerto**, y ni siquiera aparece en el diagrama de flujo de la wiki.

Existe la intención: `PipelineObserver` (`src/app/observer.rs:10`) es la
abstracción correcta —- define `on_occupancy`, `emit`, `flush`, y hay un
`NullObserver` para tests. La wiki la documenta como el mecanismo de fan-out
(`docs/wiki/6.2`).

Pero el código la honra en **un solo método**. Los otros trece sitios de llamada
entran por `FanoutObserver.viz`, que es un campo `pub`. Y `viz_mut()` devuelve
`&mut VizBridge` —- el **tipo concreto**— así que aunque se la usara, no
permitiría sustituir la implementación.

La abstracción está declarada, documentada, y evadida.

> Un subsistema sin puerto declarado termina cableado inline. Esa no es una
> observación estética: es la causa mecánica de que una preocupación de
> depuración pueda frenar el lazo de control.

## Decisión

**Se completa el seam que ya está declarado**, en cuatro movimientos:

**1. Un trait `VizSink`** con la superficie que hoy expone `VizBridge` (20
métodos, todos object-safe). Ese trait *es* el puerto faltante.

**2. `VizBridge` lo implementa directo.** Comportamiento síncrono actual, útil
cuando bloquear no importa: tests, grabación a `.rrd` local.

**3. `VizRelay` lo implementa reenviando a un hilo propio**, por un `Slot`
(ADR-034) que descarta en vez de bloquear. Es la implementación de producción.

**4. `FanoutObserver.viz` pasa a `Box<dyn VizSink>`.** Los trece sitios de
llamada existentes **siguen compilando sin tocarse**, por deref. El campo deja de
ser `pub` y `viz_mut()` devuelve `&mut dyn VizSink`.

El punto de diseño que justifica esta forma: **el costo de convertir
prestado→propio se paga una vez, dentro del relay**, y no repartido por el
pipeline. Los sitios de llamada no se enteran de que hay un hilo del otro lado.

### Por qué un trait y no mover `VizBridge` a un hilo directamente

Porque la sustituibilidad es la mitad del valor. Con el trait aparecen dos
implementaciones más que hoy no existen y hacen falta: un `NullViz` real para
correr sin visualización sin `#[cfg]` desparramados, y un sink de captura para
tests que verifique *qué* se emitió sin levantar un viewer.

## Consecuencias

**El pipeline no puede bloquearse por visualización**, por construcción y no por
cuidado.

**La visualización se vuelve testeable.** Hoy, verificar que una transición de
estado se emite correctamente exige un `MemorySink` de rerun y hurgar chunks. Con
el puerto, un sink de captura responde la pregunta directamente.

**Un `Box<dyn>` en el camino de llamada.** Despacho dinámico donde antes había
llamada estática. A las cadencias del sistema —- decenas de llamadas por segundo—
es irrelevante, y se paga a cambio de que el subsistema sea sustituible.

**El campo `pub` que se cierra puede romper código externo.** Dentro de este
repositorio son trece sitios y ninguno necesita cambiar. Si algo afuera dependía
de `observer.viz` como `VizBridge` concreto, dependía de un detalle que nunca
debió estar expuesto.

## Qué pasó realmente

**El problema se resolvió por estructura, no por abstracción**, y esta ADR queda
como registro de un diagnóstico correcto con una solución equivocada.

El diagnóstico era exacto: la visualización corría en el hilo del control y podía
frenarlo, y la causa era que no tenía puerto. La solución propuesta —un trait
`VizSink` con la superficie de los ~20 métodos, más un `VizRelay` que lo
implementara— no se construyó, y no hace falta.

Lo que pasó en la Fase 3 (ADR-033) fue que al partir las etapas, el `VizBridge`
quedó con **dueño único**: el hilo de percepción. El lazo de control ya no puede
alcanzarlo, así que no puede ser frenado por él. El problema que esta ADR quería
resolver dejó de existir por dónde quedó la frontera de ejecución.

Y el trait que ya existía —`PipelineObserver`— se borró, porque al separar las
etapas quedó con **un solo implementador y ningún doble de test**. Un trait que
no vuelve imposible ningún error es un módulo con pasos de más; es el mismo
criterio que ADR-028 aplica a los crates, y aplica igual acá.

### Lo que sigue abierto

Un enlace saturado **sigue bloqueando a percepción**: `re_chunk` no ofrece
política de descarte. Bajó de categoría —de *"un visor de depuración retrasa una
decisión clínica"* a *"un visor de depuración deja al sistema sin evidencia
fresca, y el control lo declara"*— pero no es cero.

El cierre es un `Slot` hacia un hilo propio del visor, que descarte en vez de
bloquear: la primitiva de ADR-034 aplicada al borde que le faltaba. **No** hace
falta reintroducir el trait de 20 métodos — con un solo dueño alcanza con que el
relay sea ese dueño.

Registrado en `ARCHITECTURE.md` §6.1, sin medir todavía: la corrida que lo
cuantifica es la variante `a-raw-native` del escenario 02.

## Referencias

- [ADR-033](033-isolated-control-loop.md) — el lazo de control aislado
- [ADR-034](034-slots-and-queues.md) — muestras en slots
- `ARCHITECTURE.md` §3.1 — la costura
- `docs/wiki/6.2-pipeline-state-visualization.md` — el fan-out documentado
