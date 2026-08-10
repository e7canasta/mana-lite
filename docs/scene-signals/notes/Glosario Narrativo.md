Glosario Narrativo: Arquitectura de Control y la Tabla de Señales

Como mentor en esta travesía de ingeniería, mi objetivo hoy es que dejen de ver a mana-lite como una simple aplicación de software y comiencen a respetarlo como lo que realmente es: un PLC (Programmable Logic Controller) de alto rendimiento. En nuestro mundo, la cámara no es un periférico multimedia; es un sensor de campo que alimenta un lazo de control crítico para la seguridad del paciente.

Para dominar este sistema, debemos entender la transición de una arquitectura rígida basada en código a una basada en una Imagen de Proceso, un contrato público que llamamos la Tabla de Señales.

1. Introducción: El Sistema como un PLC

En la automatización industrial, la estabilidad no es negociable. Un PLC opera bajo reglas de determinismo que hemos destilado en tres pilares fundamentales para nuestro modelo mental:

* Evidencia a tasa variable: La cámara y los modelos de IA entregan datos (rostros, personas, distancias) a ritmos distintos según la carga computacional.
* Ciclo de control a cadencia fija: Nuestro "cerebro" procesa la lógica en ticks constantes. El sistema no espera a la cámara; decide con lo que tiene en el instante preciso del reloj.
* Decisiones basadas en un snapshot: En cada ciclo, el sistema captura una "fotografía" de toda la evidencia disponible. Esa imagen congelada es la única verdad durante ese tick.

El desafío: Durante mucho tiempo, intentamos gestionar esta fluidez industrial usando estructuras de datos (structs) de Rust extremadamente rígidas. Fue una defensa necesaria, pero se convirtió en un ancla para nuestra agilidad.

2. Concepto 1: El Struct de Campos Privados (El Pasado Rígido)

Originalmente, el corazón de nuestra escena era el FsmSceneContext. Cada vez que un servicio clínico necesitaba una nueva regla —por ejemplo, validar si alguien está en el borde de la cama—, nos enfrentábamos a un "impuesto de desarrollo" inaceptable.

Como arquitecto, observen el Costo de Evolución de este modelo anterior:

Paso	Archivo	Impacto Técnico
1	fsm/engine.rs	Añadir un campo privado en FsmSceneContext.
2	scan.rs	Modificar update_context para poblar el campo.
3	fsm/guard.rs	Crear una variante en FsmGuard (para el TOML).
4	fsm/program.rs	Crear una variante en ProgramGuard (compilada).
5	program.rs	Mapear manualmente el guard en la función compile().
6	guard.rs	Implementar la lógica de evaluación final en eval().

Insight del Mentor: Podrían pensar que los pasos 3 y 4 son duplicación de código. No lo son. Es la separación vital entre compilar en boot y ejecutar con determinismo. Sin embargo, realizar estas 6 ediciones mecánicas para cada una de las 80 reglas futuras es un pasivo técnico que asfixia la innovación. Necesitábamos desacoplar la evidencia de la lógica.

3. Concepto 2: La Tabla de Señales Etiquetadas (El Contrato Público)

La solución es la SignalTable. Hemos pasado de un struct privado a una Imagen de Proceso: un mapa público de etiquetas (tags) y valores.

Este cambio nos permite una eficiencia asombrosa: hemos reducido las variantes de guards de 18 a solo 8 (7 guards especializados que mantienen lógica compleja y 1 guard genérico que cubre todo lo demás).

Así luce nuestra "Imagen de Proceso" en un tick de control:

# Catálogo v1: La escena como contrato
"persona.presente"     = true
"persona.cantidad"     = 1
"cara.confianza"       = 0.85
"cara.en_dwell"        = false
"cara.estuvo_dentro"   = true    # El 9no tag: latch de historial
"ocupacion.cardinalidad" = "single"


La Semántica del Silencio: Ausencia vs. Falso

Aquí reside el corazón de la seguridad clínica. En un PLC, si un sensor no está conectado, no puedes asumir que su valor es "0".

* Falso (false): El área de interés (ROI) está configurada, la estamos vigilando, y la cara está fuera de ella.
* Ausencia: El área de interés ni siquiera ha sido configurada para esta habitación.

Si confundiéramos Ausencia con Falso, el sistema podría decirle a una enfermera que un paciente está a salvo, cuando en realidad ni siquiera estamos mirando la cama. La SignalTable protege esta distinción: si una señal está ausente, ningún guard (ni siquiera el de desigualdad !=) coincidirá.

4. Concepto 3: Tipos con Semántica (Más allá de los Primitivos)

Para que este contrato sea infalible, los valores deben tener límites estrictos. No usamos "floats" o "integers" genéricos; usamos tipos con conciencia industrial.

Tipo	Semántica	Operadores Válidos
Bool	Presencia o estado binario.	==, !=
Count	Cantidad entera ≥ 0.	==, !=, >=, <=, >, <
Ratio	Proporción estricta de 0.0 a 1.0.	>=, <=, >, <
Label	Valor de un conjunto cerrado (Enum).	==, !=

Lección Crítica: Notarán que el tipo Ratio tiene prohibida la igualdad (==). Comparar precisiones decimales en un lazo de control es invitar a los bugs de redondeo a destruir tu lógica clínica. Además, un Ratio de 1.5 no es un "valor extraño"; es un error de programa. Si alguien intenta inyectar unidades incorrectas, el sistema prefiere fallar en el arranque antes que operar con datos semánticamente corruptos.

5. Concepto 4: Validación en Arranque vs. Runtime (Determinismo)

Al abandonar el chequeo estático del compilador de Rust para las reglas dinámicas en TOML, muchos temen perder seguridad. Nuestra respuesta es el Determinismo del PLC: el sistema "compila" su lógica al encenderse. No aceptamos reglas nuevas "en caliente" porque la flexibilidad es la enemiga del comportamiento predecible.

Durante el boot, el sistema debe detectar y rechazar estos 5 fallos antes de iniciar el lazo:

1. Tags Inexistentes: Intentar usar una señal que ningún productor declara.
2. Operadores Incompatibles: Intentar usar un "mayor que" (>=) sobre un valor Booleano.
3. Igualdad en Ratios: Usar == en valores de confianza o proporciones.
4. Valores fuera de rango: Un Ratio de 1.5 o un Count negativo.
5. Labels Inválidos: Comparar contra una categoría que el productor no puede emitir.

"Un programa de ladder logic no se type-checkea en C, se valida al cargarlo en el controlador."

Esta filosofía garantiza que si el dispositivo arranca, el programa es lógicamente válido.

6. Concepto 5: El Gemelo Digital y la Observabilidad

Este cambio alcanza su máxima expresión en la Etapa D: El Gemelo Visible. Históricamente, nuestros logs eran fragmentados; guardábamos solo lo que el programador consideraba relevante.

Con la Tabla de Señales, generamos un snapshot completo por cada tick. Esto transforma el log en una herramienta forense:

* Antes: "¿Por qué no sonó la alerta? El log no dice nada sobre la cara".
* Ahora: El equipo de Operaciones puede ver que en el tick #4502, la señal cara.confianza cayó a 0.31, por debajo del umbral clínico.

La escena deja de ser una caja negra privada para convertirse en un flujo de datos observable que explica, ciclo a ciclo, por qué se tomó cada decisión.

7. Conclusión: El Futuro del Blueprint

La Tabla de Señales no es solo un refactor; es un cambio de paradigma que transforma un "release de software" en una actualización de configuración. Hemos trazado una frontera clara: los guards de Zonas, Salud y Profundidad mantienen su complejidad especializada, mientras que la evidencia de la escena se vuelve democrática y accesible.

3 Transformaciones Mágicas para recordar:

1. Del Binario al TOML: Cambiar una condición clínica ya no requiere recompilar Rust, solo editar un blueprint.
2. De la Caja Negra a la Inspección Total: El estado interno ahora es visible y auditable mediante el Gemelo Digital.
3. Del Acoplamiento al Contrato: El sistema ya no depende de campos privados, sino de un vocabulario público y versionado.

Ahí reside la elegancia de nuestro oficio: aplicar el rigor de la ingeniería industrial para domar la complejidad del software moderno.
