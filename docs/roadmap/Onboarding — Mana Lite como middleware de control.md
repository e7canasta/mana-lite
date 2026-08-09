Onboarding — Mana Lite como middleware de control

1. El modelo mental, en un párrafo

Esto no es una app de visión que además tiene lógica. Es un PLC cuyo dispositivo de campo resulta ser una cámara. Corre a dos tasas: el campo (RTSP → decode → ONNX) va a la tasa que puede, con latencia variable y fallando seguido; el programa (tracker → presencia → ocupación → zonas → FSM → health) corre a cadencia fija y tiene que emitir salida en cada tick aunque el campo esté muerto. Entre ambos hay un solo objeto: la imagen de proceso (ProcessImage), que es el gemelo digital — congelado, fechado, con edad explícita.

Todo lo demás del diseño se deriva de ahí.

2. La única pregunta que ubica cualquier cosa

▎ Si la entrada nunca vuelve a llegar, ¿esto tiene que seguir produciendo salida correcta en cada tick?

Sí → T2 programa. No → T1 campo.

No hace falta más criterio. Los cuatro tiers:

- T0 · álgebra — sin tasa, sin estado, sin reloj. mana-id, mana-geometry.
- T1 · campo — sensado y E/S. Fallar es normal. mana-media, mana-perception.
- ProcessImage — la frontera. Pertenece a T2 (el PLC es dueño de su imagen de proceso; los dispositivos de campo no saben que existe).
- T2 · programa — cadencia fija, determinista, reloj inyectado. mana-control.
- T3 · reporte — JSONL, métricas, Rerun. Nunca bloquea el tick. En el binario.

T2 no depende de T1 no por elegancia: porque T2 debe seguir corriendo cuando T1 murió. El Cargo.toml de mana-control (mana-geometry + serde) es lo que lo hace cumplir.

3. Dónde estás hoy

Lo bueno: scan(state, image, now) -> Vec<SceneEvent> ya es el contrato PLC exacto. Y FsmProgram::compile(catalog, zones) -> Result<_, Vec<String>> ya implementa la separación compilar en boot / ejecutar determinista — que es literalmente cómo funciona un PLC con ladder logic. Eso es infraestructura seria, ya construida.

Lo roto: el árbol no compila (53 errores), y esos errores son la lista de violaciones de tier.

---
4. El stress-test: ¿se nos ahoga el rey?

Esta es tu pregunta real. La corrí contra cinco crecimientos plausibles.

✅ Nuevo modelo o tarea de percepción (pose, action recognition)

Toca mana-perception + el adaptador en el binario. T2 no se entera. El puerto SceneObservation absorbe el cambio. Sano.

✅ Nuevo sink (MQTT, webhook, HL7)

Toca solo T3. Porque scan() devuelve Vec<SceneEvent> en vez de escribir, agregar un sink es agregar un consumidor. Sano — una vez que saques logger de scan.rs, que hoy está acoplado.

✅ Nueva fuente de evidencia (térmica, audio, segunda cámara como sensor)

Nuevo crate T1 + un campo en ProcessImage + adaptador. El gemelo digital crece, que es su trabajo. Sano.

⚠️ Nueva regla de escena — acá está el ahogo

Es lo que más vas a hacer, y hoy cuesta 6 ediciones en 4 archivos:

┌─────┬────────────────────────┬─────────────────────────────────────────┐
│  #  │         Dónde          │                   Qué                   │
├─────┼────────────────────────┼─────────────────────────────────────────┤
│ 1   │ fsm/engine.rs:17       │ +1 campo en FsmSceneContext             │
├─────┼────────────────────────┼─────────────────────────────────────────┤
│ 2   │ scan.rs update_context │ +1 línea que lo puebla                  │
├─────┼────────────────────────┼─────────────────────────────────────────┤
│ 3   │ fsm/guard.rs:21        │ +1 variante en FsmGuard (cara a TOML)   │
├─────┼────────────────────────┼─────────────────────────────────────────┤
│ 4   │ fsm/program.rs:97      │ +1 variante en ProgramGuard (compilada) │
├─────┼────────────────────────┼─────────────────────────────────────────┤
│ 5   │ program.rs compile()   │ +1 brazo de mapeo                       │
├─────┼────────────────────────┼─────────────────────────────────────────┤
│ 6   │ guard.rs eval          │ +1 brazo de evaluación                  │
└─────┴────────────────────────┴─────────────────────────────────────────┘

Todo dentro de mana-control — eso es bueno, la frontera aguanta. Pero crece lineal para siempre y hay dos enums de 18 variantes cada uno que deben mantenerse sincronizados a mano. A 40 guards duele; a 80 es un pasivo.

Y FsmSceneContext son 7 booleanos planos. Cada predicado nuevo de escena es un campo más, permanentemente.

El error que no debes cometer: fusionar FsmGuard y ProgramGuard para ahorrar tipeo. Esa duplicación aparente es el split compilar/ejecutar del PLC, y es correcta. Uno es el texto del programa, el otro es el programa compilado y resuelto contra los catálogos. Preservalo.

La salida real — y es la misma idea que ya usás, aplicada un nivel más adentro:

▎ La imagen de proceso de un PLC no es un struct de booleanos con nombre. Es una tabla de señales etiquetadas.

Convertir FsmSceneContext en una tabla de señales tipadas convierte "agregar una regla" en "registrar un productor de señal", y una sola variante genérica Signal { tag, op, value } cubre una clase grande de reglas con cero variantes nuevas. Los pasos 3-6 desaparecen; quedan 1-2.

El costo: perdés el match exhaustivo del compilador. La mitigación ya existe — FsmProgram::compile() valida contra catálogos y devuelve Result<_, Vec<String>>. La verificación se muda del type system al boot, que es exactamente el modelo PLC y es aceptable porque el programa es fijo tras el arranque. Un tag mal escrito en TOML falla al compilar el programa, no en runtime.

Esto no es para ahora — es P4, después de que compile. Pero es la salida, y saber que existe cambia una decisión inmediata: cada booleano plano que agregues hasta entonces es deuda que vas a migrar.

🔴 Multi-cámara — el que sí rompe

ControlState es instancia única y el catálogo FSM es global. N cámaras = N ControlState + N ProcessImage, con un solo programa compilado compartido y N ciclos de scan. El diseño de tiers lo soporta conceptualmente (el PLC escala a N lazos), pero ControlState y ScanTimeline no están parametrizados por lazo. Si multi-cámara está en el roadmap a 12 meses, decidilo ahora: es barato hoy y caro después de 20 reglas más.

---
5. Las tres reglas que te llevás

6. La pregunta de pertenencia. "¿Tiene que tickear con el campo muerto?" Ubica cualquier archivo, tipo o función sin discutir.
7. El Cargo.toml es el lint. No escribas reglas de arquitectura en un doc que nadie lee: hacé que la dependencia prohibida no compile. La excepción son los relojes — Instant::now() dentro de T2 no lo detecta ningún crate boundary; eso se prohíbe con un tipo (que ScanInstant solo nazca de ScanTimeline).
8. Compilar en boot, ejecutar determinista. Ya lo tenés en el FSM. Es el patrón que hay que replicar cuando crezca la lógica de escena, no abandonar.

9. Lo que rompe el onboarding hoy

core/mana-control/src/scan.rs está minificado — scan() es una línea de ~4.000 caracteres, ControlState otra. El archivo que implementa el lazo de control es ilegible para quien no lo escribió. Para Linux Foundation o Eclipse eso es descalificante antes de cualquier debate de crates: ningún revisor externo puede auditar el lazo. Es rustfmt + partir scan() en los ocho pasos que ya están implícitos y que son el orden del ciclo, no una descomposición cosmética.




Listo. Cinco ADRs en el repo y el roadmap publicado.

ADRs nuevos en docs/adrs/, formato de la casa, numerados desde 027:

┌─────┬──────────────────────────────────────────────┬──────────┐
│     │                    Título                    │  Status  │
├─────┼──────────────────────────────────────────────┼──────────┤
│ 027 │ Tier Architecture by Determinism Class       │ Accepted │
├─────┼──────────────────────────────────────────────┼──────────┤
│ 028 │ Crate Boundaries as Compile-Time Enforcement │ Accepted │
├─────┼──────────────────────────────────────────────┼──────────┤
│ 029 │ Injected Clock in the Program Layer          │ Accepted │
├─────┼──────────────────────────────────────────────┼──────────┤
│ 030 │ Shared Mechanism, Owned Vocabulary           │ Accepted │
├─────┼──────────────────────────────────────────────┼──────────┤
│ 031 │ Scene Signal Table                           │ Proposed │
└─────┴──────────────────────────────────────────────┴──────────┘

ADR-028 revisa parcialmente ADR-019. Ninguno contradice ADR-001 ni ADR-003 — al contrario, este trabajo es lo que los hace cumplir.

Roadmap: ⧉ https://claude.ai/code/artifact/dc544de1-1380-4de4-a31b-52988cc480d9

Big picture con el stack de tiers, la matriz de dependencias, estado real medido, los seis sprints con sus compuertas, las cinco fases y el contrato de revisión.

Dos cambios respecto de lo que te dije antes

Ambos por evidencia que apareció al escribirlo, así que los señalo explícitamente:

Metí un Sprint 1 que no existía. Al mirar la red de seguridad encontré 5 tests en total, y golden_synthetic_cycle.rs todavía importa mana_lite::scan, ::presence, ::occupancy — módulos borrados. La red también está rota. No se puede refactorizar un lazo clínico contra 5 tests, así que "red de seguridad" es un sprint propio entre compilar y tocar diseño. Eso hace que el Sprint 0 tenga una regla incómoda pero necesaria: arreglar imports y nada más, dejando las violaciones de frontera en su lugar con un FIXME. Es antinatural y es lo correcto — cualquier mejora que se cuele ahí viaja sin red.

El Sprint 3 arranca por scan.rs, no por viz/mod.rs. Ya lo habíamos hablado, pero ahora está fijado en el plan: el archivo con más líneas de producción no es el más urgente; el ilegible sí.

Nota sobre las compuertas

Todas son comandos con salida esperada, no criterios. grep -rn 'logger' core/mana-control/src → 0 es verificable por vos, por mí, o por un revisor de la fundación sin conocer el código. Es el mismo principio que el Cargo.toml: la regla vive donde se ejecuta, no donde se documenta.

El único invariante que no se puede expresar así es el reloj, y por eso ADR-029 lo cierra con un tipo (ScanInstant sin constructor público) en vez de con una regla.

---
Nada está commiteado — los cinco ADRs están en el working tree junto a la migración en vuelo. Cuando quieras arranco: puedo hacerte el Sprint 0 completo, o revisártelo cuando lo tengas.




---


Sí, y me hiciste corregir el registro. DDD parte por sustantivos de negocio (Person, Session, Zone como agregados) — aquí eso no explica nada. Un middleware de control se parte por clase de determinismo y tasa. Y encontré la evidencia de que esa es la lectura correcta.

El criterio de corte, en vocabulario de control

Una pregunta mecánica reemplaza todo el juicio arquitectónico:

▎ Si la entrada nunca vuelve a llegar, ¿este componente tiene que seguir produciendo salida correcta en cada tick?
▎
▎ Sí → capa de programa. No → capa de campo.

No es teórica: tus últimos commits son precisamente eso — fsm: blind state recovers via data_fresh guard, health: injectable clock and recovery heartbeat, health: a failed decode is not a fresh signal. El sistema ya está construido sobre ese invariante. Solo que no está escrito como regla de fronteras.

Los tiers

┌──────────────┬─────────────────────────────────────────────────────────────────────────────────────────────────────┬─────────────┬─────────────────────────────────────────┬────────────────────────────┐
│     Tier     │                                               Qué es                                                │    Tasa     │           Semántica de fallo            │           Crates           │
├──────────────┼─────────────────────────────────────────────────────────────────────────────────────────────────────┼─────────────┼─────────────────────────────────────────┼────────────────────────────┤
│ T0           │ Álgebra pura. Sin tasa, sin estado, sin reloj.                                                      │ —           │ no falla                                │ mana-id, mana-geometry     │
├──────────────┼─────────────────────────────────────────────────────────────────────────────────────────────────────┼─────────────┼─────────────────────────────────────────┼────────────────────────────┤
│ T1 · campo   │ Sensado y E/S. Latencia no acotada.                                                                 │ variable    │ fallar es normal                        │ mana-media,                │
│              │                                                                                                     │             │                                         │ mana-perception            │
├──────────────┼─────────────────────────────────────────────────────────────────────────────────────────────────────┼─────────────┼─────────────────────────────────────────┼────────────────────────────┤
│ ⎯⎯           │ ProcessImage — la imagen de proceso. Congelada, fechada, con edad.                                  │ —           │ —                                       │ (la frontera)              │
├──────────────┼─────────────────────────────────────────────────────────────────────────────────────────────────────┼─────────────┼─────────────────────────────────────────┼────────────────────────────┤
│ T2 ·         │ Tracker, FSM, presencia, ocupación, zonas, health. Sin E/S, sin asignación sorpresa, reloj          │ fija        │ no puede fallar: tickea siempre         │ mana-control               │
│ programa     │ inyectado.                                                                                          │             │                                         │                            │
├──────────────┼─────────────────────────────────────────────────────────────────────────────────────────────────────┼─────────────┼─────────────────────────────────────────┼────────────────────────────┤
│ T3 · reporte │ JSONL, métricas, Rerun. Fuera del camino crítico.                                                   │ best-effort │ falla en silencio, nunca bloquea el     │ en el bin                  │
│              │                                                                                                     │             │ tick                                    │                            │
└──────────────┴─────────────────────────────────────────────────────────────────────────────────────────────────────┴─────────────┴─────────────────────────────────────────┴────────────────────────────┘

La regla de dependencia deja de ser estética y pasa a ser una propiedad del sistema: T2 no puede depender de T1 porque T2 debe seguir corriendo cuando T1 está muerto. Eso no es un principio de diseño, es el requisito clínico.

Y el corolario que ordena la duda de "dónde va X": la imagen de proceso pertenece a la capa de programa, no a la de campo. Un PLC es dueño de su process image; los dispositivos de campo no saben que existe. Por eso ProcessImage va en mana-control y percepción nunca lo nombra — el adaptador del runtime lo construye.

Esto ya está bien hecho — corrijo algo que dije

ProcessImage existe (core/mana-control/src/lib.rs:92). Yo venía asumiendo que el gemelo no tenía tipo; es falso. Y la firma de scan es exactamente el contrato PLC:

pub fn scan(state: &mut ControlState, image: &ProcessImage, now: ScanInstant) -> Vec<SceneEvent>

Imagen de proceso + instante de ciclo → eventos. Sin logger, sin E/S, sin Instant::now()  diseñado del repo y valida el encuadre entero: no hay que inventar la arquitectura de
tiers, hay que hacerla explícita y que el compilador la sostenga.

Las violaciones, renombradas

Con el vocabulario correcto dejan de ser "acoplamientos" y pasan a tener significado físi

┌──────────────────────────────────────────────────────────────────────────┬────────────────────────────────────────────────────────────────────────────────────────────────────┐
│                                Violación                                 │                        Qué significa en un PLC                                                     │
├──────────────────────────────────────────────────────────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ cascade.rs → kalman, track                                               │ La capa de c. Es realimentar la salida al sensor sin pasar por la imagen de proceso. Rompe la      │
│                                                                          │ determinabilidad del ciclo.                                                                                                    │
├──────────────────────────────────────────────────────────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ scan.rs → logger                                                         │ La capa de pAcopla el tick determinista a E/S de disco.                                            │
├──────────────────────────────────────────────────────────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ ScanInstant::now() (scan.rs:14) y health.rs new()/touch()/evaluate() sin │ Puertas al rapa determinista.                                                                      │
│  sufijo _at                                                              │                                                                                                    │
└──────────────────────────────────────────────────────────────────────────┴────────────────────────────────────────────────────────────────────────────────────────────────────┘

La tercera es la peor y no la detecta ningún crate boundary: las variantes _at inyectan econviven con wrappers de conveniencia que llaman Instant::now(). Un Cargo.toml no puede
prohibir eso. Se prohíbe con un tipo — que ScanInstant solo sea construible desde ScanTims:31) y borrar ScanInstant::now() y los wrappers no-_at.

Un hallazgo que cambia mi P2
core/mana-control/src/scan.rs está minificado. La función scan() es una sola línea de ~4. es otra. update_context, intersects, near, ScanTimeline — todas en una línea cada una.Eso invalida la métrica de líneas en ambas direcciones: "528 líneas de producción" es unarchivo más crítico del sistema — el que implementa el ciclo PLC — es ilegible paracualquiera que no lo haya escrito. Para un aterrizaje en Linux Foundation o Eclipse eso eualquier discusión de crates: ningún revisor externo puede auditar el lazo de control.

Corrijo la prioridad que te di: P2 empieza por scan.rs y mana-control/src/domain.rs (el m, no por viz/mod.rs. Es rustfmt + partir scan() en los pasos que ya están implícitos(predict → age → presence → track → occupancy → zones → fsm → health), que es literalmente el scan -> age_input() -> update_presence() -> ... que proponía la revisión 2. Ahí sí tenían razón, y ahora se ve por
qué: esos pasos son el orden del ciclo de scan, no una descomposición estética.

---
El plan queda: P0 cerrar compilación con las fronteras T1/T2 · P1 sellar la frontera (borr Event de scan, cortar cascade → kalman) · P2 legibilidad del lazo de control (scan.rsprimero) · P3 consolidar std/ a los 5 crates.



 Un dato más que decide todo: core/mana-control/src/domain.rs son 6 líneas — el macro domain_id! copiado y minificado en una sola línea, sin DomStr (que no está definido ahí; de ahí dos de los errores de compilación). El vocabulario de newtypes se forkeó mal durante la migración. Eso es exactamente el problema que un diseño de crates tiene que resolver.

El criterio

En Rust, pub(crate) ya te da privacidad de módulo. Un crate separado compra una sola cosa que los módulos no dan: un Cargo.toml que hace que una dependencia prohibida no compile.

Por lo tanto la pregunta para cada crate candidato no es "¿es un tema distinto?" sino:

▎ ¿Qué dependencia quiero volver imposible?

Si la respuesta es "ninguna", no es un crate: es un módulo. Aplicado a lo que existe hoy:

┌─────────────────┬──────────────────────────────────────────────────────┬─────────────┐
│      Crate      │                ¿Qué vuelve imposible?                │  Veredicto  │
├─────────────────┼──────────────────────────────────────────────────────┼─────────────┤
│ mana-control    │ ONNX, Rerun, FFmpeg, JSONL, percepción               │ ✅ crate    │
├─────────────────┼──────────────────────────────────────────────────────┼─────────────┤
│ mana-perception │ tracker, FSM, Kalman, zonas                          │ ✅ crate    │
├─────────────────┼──────────────────────────────────────────────────────┼─────────────┤
│ mana-geometry   │ todo (hoja pura, 7 consumidores reales)              │ ✅ crate    │
├─────────────────┼──────────────────────────────────────────────────────┼─────────────┤
│ mana-media      │ control y percepción; aísla el feature ffmpeg        │ ✅ crate    │
├─────────────────┼──────────────────────────────────────────────────────┼─────────────┤
│ mana-viz        │ nada. Ya es Rerun-específico, lo llama solo src/viz/ │ ❌ módulo   │
├─────────────────┼──────────────────────────────────────────────────────┼─────────────┤
│ mana-types      │ nada. Es el vector que mantiene vivo código muerto   │ ❌ borrar   │
├─────────────────┼──────────────────────────────────────────────────────┼─────────────┤
│ mana-rtsp       │ nada. 64 líneas de helper H.264                      │ ❌ absorber │
└─────────────────┴──────────────────────────────────────────────────────┴─────────────┘

La evidencia de mana-types: sus 8 fns públicas de mana-viz tienen 2 consumidores reales (boxes2d_from_xyxy, FrameSize). log_detections_2d, log_zones_2d, log_roi_2d, log_frame_rgb24 están muertas — y son las únicas que usan DetectionBatchV1, SceneMsgV1, RoiCommandV1. Borrado el código muerto, los seis tipos wire quedan en cero usos. DetectionV1, SceneEntityV1 y ZoneV1 ya están en cero hoy.

Diseño recomendado: 5 libs + 1 bin

mana-id/          ← tier 0: DomStr + macro domain_id!  (~100 líneas)
mana-geometry/    ← tier 0: bbox, iou, polygon, compact_mask, transform

mana-media/       ← tier 1: PixelFormat, RawFrame, decoder, buffer_pool, h264
mana-perception/  ← tier 1: detection, cascade, depth_map, backend ONNX
mana-control/     ← tier 1: track, fsm, presence, occupancy, zones, health, kalman

mana-lite/ (bin)  ← tier 2: app, config, logger, metrics, viz, ingest, ADAPTADORES

La matriz de dependencias es la arquitectura, y se escribe en Cargo.toml:

┌─────────────────┬─────────┬──────────┬───────┬────────────┬─────────┐
│                 │ mana-id │ geometry │ media │ perception │ control │
├─────────────────┼─────────┼──────────┼───────┼────────────┼─────────┤
│ mana-geometry   │    —    │    —     │   —   │     —      │    —    │
├─────────────────┼─────────┼──────────┼───────┼────────────┼─────────┤
│ mana-media      │    —    │    —     │   —   │     —      │    —    │
├─────────────────┼─────────┼──────────┼───────┼────────────┼─────────┤
│ mana-perception │    ✔    │    ✔     │   ✔   │     —      │   ⛔    │
├─────────────────┼─────────┼──────────┼───────┼────────────┼─────────┤
│ mana-control    │    ✔    │    ✔     │  ⛔   │     ⛔     │    —    │
├─────────────────┼─────────┼──────────┼───────┼────────────┼─────────┤
│ mana-lite       │    ✔    │    ✔     │   ✔   │     ✔      │    ✔    │
└─────────────────┴─────────┴──────────┴───────┴────────────┴─────────┘

Las dos celdas ⛔ que importan: control no ve percepción, percepción no ve control. Hoy eciones (cascade.rs → kalman/track; scan.rs → detection). Y control ⛔ media: control nunca
toca un frame, solo observaciones ya adaptadas.

Nótese que mana-control no depende ni de image, ni de ndarray, ni de chrono, ni de log. Sgeometry + serde. Eso es el lint: no hay que escribir una regla de arquitectura ni unscript de CI, el linker la aplica.

Los dos puntos difíciles

1. Dónde viven los IDs. Aquí es donde discrepo de la propuesta de mana-core. La regla que

▎ Se comparte el mecanismo, no el vocabulario.

DomStr + domain_id! van a mana-id (tier 0). Pero las instancias las declara cada dueño: Món, StateId/ZoneId en control. El adaptador del runtime convierte en el puerto (.as_str(),
un hop).
El argumento: un macro no puede ser vector de acoplamiento — no arrastra tipos. Un ClassNeses tiene un campo que solo le sirve a percepción y control ya no puede evolucionarlo. Laalternativa "compartir el tipo" es cómoda hoy y es precisamente cómo mana-core se convierte en el vertedero que describí antes. El costo real de mi regla es una conversión trivial en el adaptador; el costo del mana-core compartido es que en un año el puerto es decorativo.

Y contra el estado actual no hay debate: hoy el macro está forkeado en 6 líneas minificad

2. El contrato wire. No haría mana-wire todavía. El contrato JSONL vive en src/logger/, t segundo binario. Que sea un módulo logger/schema/ con structs versionadas y tests decaracterización congelados — eso te da lo que importa (separar evento de dominio de registro wire) sin pagar una frontera. Se promueve a crate el día que aparezca un segundo consumidor, no antes.

Los *V1 de iceoryx2 son contrato de Full Mana OS, no de Mana Lite. Su sitio es el repo de Full Mana OS. Aquí son cero usos sostenidos por código muerto. Los borraría; git los recuerda.                       Qué desaparecemana-types (456 líneas → PixelFormat y RawFrameV1 migran a mana-media, el resto se borra)ervando 2 funciones de 8), mana-rtsp (64 líneas → mana-media). Neto: de 7 crates a 5, concero pérdida de capacidad y tres fronteras falsas menos que mantener.

---
Esto es ortogonal al P0 que planteé antes: primero cerrar la compilación con las fronteransolidar std/. ¿Lo redacto como documento de arquitectura con la matriz, los veredictos y
el criterio del "¿qué vuelve imposible?", o prefieres discutir antes alguno de los dos pu