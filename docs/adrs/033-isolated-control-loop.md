# ADR-033: El lazo de control es un hilo aislado que no puede bloquearse

**Status:** Accepted — implementado 2026-08-11 (Fases 3 y 4)
**Date:** 2026-08-11

## Qué cambia para el producto

Hoy **los tiempos clínicos configurados no son los tiempos clínicos entregados**.

`single_confirm_ms = 3000` promete que el sistema confirma una persona tras 3
segundos de evidencia sostenida. Pero el lazo que evalúa esa regla comparte hilo
con la decodificación, con la inferencia y con la visualización. Cuando la
inferencia tarda 216 ms, el lazo no corre. Cuando alguien abre un visor de
depuración y la red se satura, el lazo puede quedarse **8,2 segundos** sin correr
—- medido, no hipotético.

Dicho de otro modo: **abrir un visor de depuración puede retrasar una decisión
clínica.** Esa frase debería ser imposible de escribir sobre este sistema.

Después de este cambio:

**1. La cadencia es una garantía, no una aspiración.** El lazo de control hace
únicamente trabajo acotado —- leer la última observación, avanzar el reloj,
evaluar— y nada con latencia variable puede vivir en él.

**2. Un lazo tarde es visible.** Si el lazo pierde su deadline, eso pasa a ser una
observación de salud del sistema, no un contador que nadie mira. Un sistema
clínico que corre lento en silencio es peor que uno que declara que está
degradado.

**3. `data_stale` recupera su significado.** Hoy puede dispararse porque el hilo
estuvo ocupado. Después, sólo se dispara si percepción realmente dejó de
producir —- que es lo único que debería significar.

## Contexto

Todo el trabajo del sistema ocurre en una sola task de tokio (`src/app/mod.rs:65`):

```rust
tokio::select! {
    kf = self.ingest.poll_freshest_keyframe() => { /* decode + inferencia */ }
    _  = scan_interval.tick()                 => { /* control + observabilidad */ }
}
```

`process_keyframe` (`src/app/mod.rs:145`) **no es `async`**: decodificar y correr
la cascada bloquean la task completa. Mientras corre, `scan_interval.tick()` no
puede dispararse.

Tres consecuencias observadas en producción:

| Síntoma | Causa |
|---|---|
| `min 1ms` en todos los reportes de ciclo | ticks recuperados en ráfaga tras un bloqueo |
| scan de 8,2 s, 8 keyframes descartados | `rec.log()` bloqueado por contrapresión de rerun |
| keyframes contados como vistos y perdidos | `select!` cancelando el drenaje de ingesta |

El tercero ya se corrigió con un invariante de cancelación-seguridad. Pero un
invariante es una regla que alguien tiene que recordar; la estructura correcta
haría innecesaria la regla.

ADR-027 estableció la separación por tiers y ADR-028 la hizo cumplir con
fronteras de crate. Esas fronteras son **de compilación**: garantizan que control
no dependa de ONNX. No dicen nada sobre **de quién es el hilo**, y es ahí donde
la separación se rompe en ejecución.

`App` (`src/app/mod.rs:34`) lo muestra sin ambigüedad: 19 campos que ya están
agrupados por subsistema —- ingesta, percepción, control, observabilidad— pero
comparten una sola estructura y un solo hilo. **El diseño agrupa; la ejecución no
separa.**

## Decisión

**Cada preocupación con latencia variable corre en su propia etapa, con su propio
hilo. El lazo de control es una de ellas y no puede bloquearse por ninguna otra.**

```
[tokio task]        RTSP → demux → dedupe          retina es async; se queda ahí
      │  Slot<RawKeyframe>
      ▼
[hilo percepción]   decode → cascada → SceneSample
      │  Slot<ProcessImage>          Slot<frame+detecciones> ──► [hilo viz]
      ▼
[hilo control]      scan() @ 200 ms — lee el slot, jamás bloquea
      │  Cola<SceneEvent>  ──► [hilo logger]
```

Reglas que se derivan y que son verificables:

1. **Ningún borde que toque el lazo de control puede bloquear.** La política de
   saturación de cada borde se declara explícitamente (ADR-034).
2. **El trabajo del lazo de control está acotado por construcción.** Si algo con
   latencia variable necesita entrar, entra como etapa, no como llamada.
3. **Hilos para cómputo, async sólo para I/O de red.** Decode e inferencia son
   CPU-bound; una task async que hace 216 ms de trabajo síncrono es un hilo con
   pasos de más y con un peligro de cancelación que un hilo no tiene.
4. **Cada borde es observable.** Contador de descartes por slot, publicado en
   métricas. Un descarte silencioso es la misma clase de mentira que un knob que
   se ignora.

## Consecuencias

**El `select!` desaparece del camino caliente**, y con él toda la clase de bug de
cancelación. El invariante *"todo future dentro del select debe ser
cancelación-seguro"* deja de ser necesario: no hace falta una regla si la
estructura hace imposible la situación.

**El presupuesto de ciclo pasa a medir algo accionable.** Si un lazo aislado que
sólo hace trabajo acotado se pasa de 200 ms, es un problema real del lazo y no
ruido de otra etapa.

**El reloj virtual deja de ser correcto por accidente.** Hoy `ScanTimeline` cuenta
ticks y se mantiene alineado sólo porque `MissedTickBehavior::Burst` recupera los
perdidos. Con el lazo aislado se puede derivar el tiempo del reloj monotónico y
que un deadline perdido sea visible en vez de absorbido (ADR-029 se enmienda en
su mecanismo, no en su principio: el reloj se sigue inyectando).

**Costos que se aceptan.** Más sincronización que razonar, mitigada por pocos
bordes y primitivas simples. Hasta un periodo de latencia por borde —- a un
keyframe por segundo, irrelevante. Depuración de concurrencia más cara, mitigada
haciendo cada borde observable.

**Lo que no cambia.** El modelo de dominio —- `ProcessImage`, `SceneEvent`, el
catálogo de señales, el layering de configuración, la validación al arrancar.
Esta decisión cambia la plomería, no el dominio, y no debería alterar ninguna
regla clínica.

## Resultado medido

Escenario 03 (`workshop/scenarios/03-ingest-infer/`), antes y después de separar
las etapas, con la misma cámara y el mismo modelo:

| | antes | después |
|---|---|---|
| `cycle` p95 | 305–348 ms | **200–201 ms** |
| `cycle` min | 49–92 ms | **197–199 ms** |
| atraso p95 | 101–154 ms | **1,4–3,3 ms** |
| atraso max | 200–260 ms | **1,6–4,8 ms** |
| vencimientos incumplidos | 5 por ventana | **0** |
| latencia de inferencia | 194–217 ms | 194–217 ms |

La inferencia tarda exactamente lo mismo: no se optimizó nada, dejó de cobrárselo
al lazo. El `cycle min` de 199 ms es la señal de que la causa se fue y no se
disimuló — un mínimo de 85 ms era la recuperación en ráfaga, y ya no hay nada que
recuperar.

Ningún slot descartó muestras en la corrida (`kf_pisados` y `img_pisadas` en
cero): la cadencia no se compró tirando evidencia.

## Lo que la decisión no previó

**El lazo es cerrado, no un pipeline.** El FSM decide qué modelos corren y el
tracker dónde recortar: percepción es el actuador de un lazo, no una fuente. Por
eso hay dos slots en direcciones opuestas y no uno. El diagrama de esta ADR
muestra sólo el sentido de ida.

**El decoder no se puede mover de hilo.** `FrameDecoder` contiene un
`ffmpeg_next::software::scaling::Context` con un `*mut SwsContext` y sin
`unsafe impl Send`, así que la etapa se manda como semilla y el decoder se
construye del otro lado. El hilo reporta hacia atrás si falla, para que un ffmpeg
roto siga siendo falla de arranque.

## Referencias

- [ADR-027](027-tier-architecture.md) — separación por tiers
- [ADR-028](028-crate-boundaries.md) — fronteras de crate (de compilación)
- [ADR-029](029-injected-clock.md) — reloj inyectado
- [ADR-034](034-slots-and-queues.md) — muestras en slots, eventos en colas
- [ADR-035](035-observability-port.md) — el puerto de observabilidad
- `ARCHITECTURE.md` §1, §2, §6.1
