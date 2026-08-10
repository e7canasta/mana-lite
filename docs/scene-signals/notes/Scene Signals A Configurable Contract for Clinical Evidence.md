Scene Signals: A Configurable Contract for Clinical Evidence


Esta serie de documentos técnicos describe la transición del sistema de monitoreo de escenas hacia una **tabla de señales tipadas, declaradas y versionadas** para optimizar la toma de decisiones clínicas. El objetivo principal es transformar la evidencia capturada en un **contrato público y configurable**, permitiendo que nuevas condiciones de seguridad se definan mediante archivos de configuración sin necesidad de recompilar el código fuente. La implementación se estructura en **cuatro etapas progresivas** que garantizan la estabilidad del sistema, manteniendo intactos los comportamientos clínicos actuales mientras se mejora la **observabilidad mediante un gemelo digital** en los registros. Al estandarizar el vocabulario técnico en tipos como **Booleanos, Conteos, Ratios y Etiquetas**, se facilita la integración con sistemas externos y se eliminan ambigüedades en la detección de eventos. Esta arquitectura preserva motores especializados para áreas críticas como la profundidad y la salud del equipo, asegurando un **vínculo determinista** entre la evidencia recolectada y las alertas generadas.

Explica la importancia de la semántica de ausencia frente al valor falso.
¿Qué guards especializados conservarán su propia lógica y por qué no migran?
¿Cómo se validan los umbrales en el arranque?
¿Qué sucede si una señal está ausente en el blueprint?
¿Qué sucede si una señal está ausente en un tick?
¿Por qué el operador de igualdad está prohibido para Ratios?
¿Cómo valida el motor los valores de tipo Label?
¿Qué es un SignalFault?
Diferencia entre señal y alerta
¿Cómo se gestiona el historial con la señal cara.estuvo_dentro?
¿Por qué no se migran los guards de salud y profundidad?
¿Qué diferencia hay entre SignalFault y una señal ausente?

---


## ¿Cómo permite el nuevo sistema de señales configurar blueprints sin reprogramar Rust?

El nuevo sistema de señales permite configurar blueprints sin reprogramar Rust al transformar la evidencia clínica de un **struct privado de campos fijos** en una **tabla de señales etiquetadas y tipadas** que actúa como un contrato publicado,,.

A continuación se detalla el mecanismo técnico y operativo que habilita esta capacidad:

### 1. La variante de Guard Genérico
Anteriormente, cada nueva condición clínica requería crear una variante específica en un `enum` de Rust y realizar seis ediciones en cuatro archivos distintos,. El nuevo sistema introduce una variante genérica llamada **`ProgramGuard::Signal { tag, op, value }`**,.
*   Esta variante permite que 11 de los 18 guards actuales (como presencia de persona, confianza facial o cardinalidad) se expresen como una simple comparación de **"etiqueta, operador y valor"**,.
*   Al ser genérica, el sistema ya sabe cómo evaluarla sin necesidad de añadir código nuevo para cada nueva regla que siga este patrón,.

### 2. Configuración vía TOML en el Blueprint
Los blueprints ya incluían archivos de configuración (`fsm.toml`), pero antes solo podían elegir entre predicados pre-compilados en el binario. Con la tabla de señales:
*   Un administrador puede definir una transición nueva directamente en el **TOML** usando el vocabulario de etiquetas declarado (ej. `persona.presente`, `cara.confianza`, `ocupacion.cardinalidad`),.
*   Esto permite que servicios con necesidades distintas (como una UTI frente a una sala general) tengan **umbrales y reglas propias** simplemente editando su archivo de configuración, sin generar un nuevo release del software,,.

### 3. Validación en Arranque (Modelo PLC)
Para mantener la seguridad clínica sin el chequeo estricto del compilador de Rust en cada regla, el sistema adopta un **modelo de PLC (Controlador Lógico Programable)**,.
*   **Compilación en Boot:** El sistema carga el programa del FSM y lo valida contra un catálogo estático de señales durante el arranque,.
*   **Detección de Errores:** La función `FsmProgram::compile()` rechaza el inicio si el TOML contiene un tag inexistente, un operador incompatible con el tipo (como un `>=` sobre un booleano) o valores fuera de rango (como un Ratio mayor a 1.0),,.
*   **Determinismo:** Una vez validado y arrancado, el programa es fijo; no se aceptan reglas nuevas "en caliente", lo que garantiza que el lazo de control sea predecible y seguro,,.

### 4. Vocabulario Declarado y Semántico
Para que la configuración sea robusta, los tags no son strings libres, sino **vocabulario declarado y versionado**,.
*   Cada señal tiene un tipo con semántica estricta (Bool, Count, Ratio, Label) que el motor de validación entiende,.
*   El sistema distingue entre un valor `false` y una **señal ausente** (por ejemplo, si no hay una zona de dwell configurada), evitando comportamientos erróneos por falta de evidencia,,.

En resumen, el sistema mueve la decisión de **"qué condición evaluar"** del código fuente de Rust a la configuración del blueprint, manteniendo la seguridad mediante un proceso de validación exhaustivo durante el encendido del equipo,.


---

## ¿Qué tipos de datos y operadores soporta el motor?

El motor de la tabla de señales soporta cuatro tipos de datos fundamentales, cada uno con una semántica estricta y un conjunto específico de operadores permitidos para garantizar la seguridad clínica y el determinismo del sistema,,.

A continuación se detallan los tipos de datos y sus operadores:

### 1. Tipos de Datos y Operadores Válidos
| Tipo de Dato | Semántica | Operadores Permitidos |
| :--- | :--- | :--- |
| **Bool** | Representa valores verdadero o falso. | `==` (igualdad), `!=` (desigualdad),,. |
| **Count** | Representa una cantidad mediante un entero no negativo (\\(\ge 0\\)),. | `==`, `!=`, `>=`, `<=`, `>`, `<`,,. |
| **Ratio** | Proporción de valores finitos en el rango **0.0 a 1.0**,. | `>=`, `<=`, `>`, `<`,,. |
| **Label** | Valor perteneciente a un conjunto cerrado y conocido (dominio),. | `==`, `!=`,,. |

### 2. Reglas de Validación y Restricciones Técnicas
El sistema impone restricciones críticas para evitar errores comunes en la lógica de control:

*   **Prohibición de igualdad en Ratios:** No es posible comparar un `Ratio` mediante `==` o `!=`,,. Esto evita errores derivados de la comparación exacta de punto flotante en un lazo de control crítico,.
*   **Validación de Rango para Ratio:** Cualquier valor de `Ratio` fuera del intervalo, o que sea infinito o NaN, es rechazado inmediatamente durante la construcción o la validación en el arranque,,,.
*   **Integridad de Labels:** Los valores de tipo `Label` deben coincidir exactamente con las variantes declaradas por el productor en el catálogo; de lo contrario, la validación fallará,,.
*   **Comportamiento ante Señales Ausentes:** Si una señal está ausente en un tick específico, **ningún operador coincidirá**, incluyendo el operador de desigualdad (`!=`),,. Esto garantiza que la falta de evidencia no se confunda accidentalmente con un valor falso o negativo,.

### 3. Verificación en el Arranque (Boot)
A diferencia de un sistema de tipado dinámico, todas estas reglas se validan mediante la función `FsmProgram::compile()` antes de que el sistema comience a operar,,. El motor rechazará el inicio si detecta un operador incompatible con el tipo (por ejemplo, un `>=` sobre un `Bool`) o si un blueprint intenta realizar una comparación prohibida,,.

---

## ¿Qué sucede si una señal está ausente en un tick?

Si una señal está ausente en un tick, el sistema sigue una regla estricta: **ningún guard genérico coincide con esa señal, incluyendo el operador de desigualdad (`!=`)**,,.

Este comportamiento es fundamental para la seguridad clínica y el determinismo del sistema, y se desglosa en los siguientes puntos clave:

### 1. Distinción entre "Falso" y "Ausente"
El contrato de señales establece que la ausencia no es un error de programa, sino un estado normal de la evidencia,. Es crucial distinguir estos dos casos:
*   **Valor `false`:** La capacidad existe y se midió un resultado negativo (ej. hay un área de interés configurada, pero el paciente no está en ella).
*   **Señal ausente:** No hay evidencia disponible para ese tick o la capacidad no está configurada (ej. el servicio no utiliza zonas de dwell),.

### 2. Semántica de Evaluación
Cuando el motor de la FSM evalúa una transición que depende de una señal ausente:
*   **Falla la coincidencia:** El guard simplemente no se activa.
*   **Sin coerciones:** El sistema **no convierte** la ausencia en `false`, en cero, ni en un valor por defecto,.
*   **Protección contra falsos negativos:** Esto evita que un despliegue sin una configuración específica (como un ROI de dwell) se comporte accidentalmente como si hubiera medido una condición negativa,.

### 3. Ejemplo Clínico: `cara.en_dwell`
El impacto de esta lógica se ve claramente en la señal de permanencia facial,:
*   **`true`:** La cara está dentro del área configurada.
*   **`false`:** El área está configurada, pero la cara está fuera.
*   **Ausente:** **No hay evidencia de dwell disponible** (porque no se configuró el área en ese blueprint). En este caso, un guard que busque tanto `== true` como `== false` fallará, protegiendo al sistema de tomar decisiones basadas en datos inexistentes.

### 4. Diferencia con Fallos de Sistema
Es importante no confundir una señal ausente con un **`SignalFault`**. Mientras que la ausencia es un estado válido de la evidencia, un `SignalFault` ocurre si un productor intenta insertar un valor inválido o incompatible con el catálogo, lo que activa inmediatamente un estado seguro y emite un diagnóstico,.

En resumen, la ausencia garantiza que el sistema solo actúe cuando existe **evidencia positiva o negativa explícita**, tratando la falta de datos como una condición que no satisface ninguna regla de comparación.

---

## ¿Cómo se definen los Blueprints?

En el sistema mana-lite, los **Blueprints** se definen como el mecanismo de configuración que permite variar el comportamiento del sistema según el despliegue (por ejemplo, adaptándolo a una UTI o a una sala general) sin necesidad de recompilar el binario de Rust.

La definición y estructura de los Blueprints se basa en los siguientes puntos clave:

### 1. Ubicación y Archivos de Configuración
Cada Blueprint se organiza dentro del directorio `config/blueprints/` y consta de archivos en formato **TOML** que declaran la intención del programa:
*   **`models.toml`**: Especifica qué modelos de inferencia deben ejecutarse en ese despliegue específico.
*   **`fsm.toml`**: Define las transiciones de la Máquina de Estados Finatarios (FSM) y los predicados (guards) que las disparan.

### 2. Definición de Reglas Clínicas (Guards)
Con la implementación de la tabla de señales, la definición de condiciones clínicas dentro del Blueprint pasa de ser código estático a ser **configuración declarativa**.
*   **Vocabulario Declarado:** En lugar de usar campos privados de Rust, el Blueprint utiliza un catálogo de **tags** (ej. `persona.presente`, `cara.confianza`) para expresar condiciones.
*   **Sintaxis Genérica:** Un Blueprint puede ahora definir un guard usando una estructura simple de **"tag, operador y valor"** directamente en el archivo TOML. Por ejemplo, se puede configurar un guard como `{ type = "signal", tag = "cara.confianza", op = ">=", value = 0.5 }` sin tocar el código fuente.

### 3. Personalización por Servicio
Los Blueprints permiten que cada servicio hospitalario defina sus propios umbrales y reglas:
*   **Umbrales específicos:** Un servicio puede configurar que una alerta de "salida de cama" espere un **dwell** (tiempo de permanencia) distinto al de otro servicio.
*   **Condiciones nuevas:** Se pueden componer nuevas reglas clínicas sobre la evidencia existente (presencia, cardinalidad, confianza facial) simplemente editando el Blueprint del servicio correspondiente.

### 4. Validación en el Arranque (Modelo PLC)
Aunque los Blueprints se definen en archivos de texto (TOML), el sistema garantiza la seguridad clínica mediante un proceso de validación exhaustivo al encender el equipo:
*   **Compilación en Boot:** La función `FsmProgram::compile()` carga el Blueprint y lo valida contra un catálogo estático de señales.
*   **Detección de Errores:** Si el Blueprint contiene un tag inexistente, un operador incompatible (como `>=` sobre un booleano) o un valor fuera de rango, el sistema **rechaza el inicio** y emite mensajes de error detallados.
*   **Inmutabilidad en Ejecución:** Una vez que el programa se valida y arranca, la configuración es **fija**; no se permiten cambios "en caliente" para asegurar un lazo de control determinista.

En resumen, definir un Blueprint consiste en crear un conjunto de archivos TOML que especifican qué modelos usar y qué reglas de control aplicar, utilizando un lenguaje de señales tipadas que el motor de la FSM valida rigurosamente durante el arranque.


---


## ¿Cuáles guards no se migran?


De los 18 guards que existen actualmente en el sistema, **7 no se migran** a la nueva tabla de señales y conservan su propia variante explícita. Estos guards se mantienen fuera del sistema genérico porque su funcionamiento no es una simple comparación de valores, sino que requieren lógica compleja, motores especializados o manejo de historial temporal.

Los guards que permanecen sin cambios se agrupan en tres categorías:

*   **Guards de Zonas (4 variantes):** Incluyen `zone_present`, `zone_occupied`, `zone_vacated` y `all_zones_vacant`. Estos no se convierten en señales porque necesitan interactuar directamente con el **motor de zonas** y gestionar sus propios **timers de histéresis** para mantener el significado clínico de la ocupación espacial. Por ejemplo, el dwell de tres segundos para detectar que una cama quedó vacía sigue dependiendo de esta lógica especializada.
*   **Guards de Salud o Health (2 variantes):** Se trata de `data_stale` y `data_fresh`. Estos guards leen información del componente **Health**, el cual posee su propia máquina de estados interna para determinar si la señal de la cámara es vieja o se ha perdido.
*   **Guard de Regla de Profundidad (1 variante):** El guard `depth_rule` se mantiene como una variante propia debido a que debe evaluar snapshots de reglas contra el **mapa de profundidad** de la escena.

En contraste, los otros **11 guards** (relacionados con la cara, la presencia de personas y la cardinalidad) sí se colapsan en una única variante genérica, ya que su lógica se limita a leer un campo del contexto y compararlo contra un valor. La decisión de dejar estos 7 guards específicos fuera de la tabla garantiza que no se pierdan las capacidades de control determinista y la semántica clínica que ya proveen sus respectivos motores.

### ¿Por qué no se migran los guards de zonas?

Los guards de zonas (como `zone_present`, `zone_occupied`, `zone_vacated` y `all_zones_vacant`) no se migran a la tabla de señales genérica porque su funcionamiento **no es una simple comparación de valores**, sino que dependen de una lógica clínica y temporal compleja que reside en componentes especializados,.

A continuación se detallan las razones técnicas y funcionales para mantenerlos como variantes propias:

### 1. Dependencia del Motor de Zonas
A diferencia de los guards de cara o presencia (que solo leen un campo y lo comparan), los guards de zonas necesitan interactuar directamente con el **motor de zonas**,. Este motor es el encargado de procesar la geometría de la escena y determinar la interacción de los tracks con áreas específicas, una responsabilidad que la tabla de señales no pretende asumir,.

### 2. Gestión de Histéresis y Timers
La lógica de zonas incluye **semántica temporal** crítica para la seguridad clínica.
*   **Timers de Dwell:** Por ejemplo, la transición que detecta que una cama quedó vacía (`zone_vacated`) requiere que la condición se mantenga durante un tiempo determinado (ej. 3 segundos) antes de disparar la alerta,.
*   **Significado Clínico:** Si estas condiciones se convirtieran en simples señales booleanas, se perderían los timers de histéresis que filtran el "ruido" en la detección y le dan estabilidad al sistema. El motor de señales está diseñado para snapshots de un tick, no para gestionar estados temporales acumulados entre ticks.

### 3. Configurabilidad Existente
Muchos de los parámetros que se busca flexibilizar con el nuevo sistema, como los tiempos de permanencia en zona, **ya son configurables** en los blueprints actuales,. Por ejemplo, el valor de `min_duration_ms` para una zona ya vive en el archivo `fsm.toml` del blueprint, por lo que no existe una urgencia técnica de "desbloquear" esa configuración mediante la tabla de señales.

### 4. Preservación de Invariantes Clínicos
El objetivo del rediseño es mover **dónde** vive una decisión (del código al contrato) sin alterar **cuál** es la decisión. Forzar la lógica de zonas dentro de una tabla de señales genérica obligaría a reconstruir resultados complejos a partir de booleanos simples, lo que aumentaría el riesgo de errores en el lazo de control y podría romper el comportamiento actual del sistema,.

En resumen, el sistema distingue entre **evidencia simple** (que pasa a la tabla) y **responsabilidad de componentes especializados** (zonas, salud y profundidad), los cuales conservan sus propios motores y estados para garantizar la precisión de las alertas clínicas,,.


### ¿Por qué no se migran los guards de salud y profundidad?

Los guards de **salud** (health) y **profundidad** (depth) no se migran a la nueva tabla de señales porque su funcionamiento requiere una **lógica compleja y motores especializados** que van más allá de una simple comparación de valores etiquetados.

Mientras que el nuevo sistema genérico está diseñado para predicados del tipo "leer un campo y compararlo", estos guards específicos mantienen sus propias variantes por las siguientes razones técnicas:

### 1. Guards de Salud (`data_stale`, `data_fresh`)
Estos guards no se convierten en señales porque dependen del componente **Health**, el cual posee su propia **máquina de estados interna**.
*   **Responsabilidad:** El motor de Health es el encargado de determinar si la evidencia proveniente de la cámara es "vieja" (stale) o "fresca" (fresh).
*   **Seguridad Clínica:** Esta lógica es fundamental para llevar el sistema a un **estado seguro (blind)** si se pierde la señal de video, una función de seguridad crítica que no debe diluirse en una tabla de comparación de valores simples.

### 2. Guard de Profundidad (`depth_rule`)
El guard de regla de profundidad conserva su variante propia debido a la naturaleza de los datos que procesa.
*   **Evaluador Especializado:** Este guard debe evaluar snapshots de reglas directamente contra el **mapa de profundidad** de la escena, lo cual requiere un evaluador de profundidad específico en lugar de un simple tag en una tabla.
*   **Significado Clínico:** Se utiliza para detectar hitos críticos como el acercamiento de un paciente al borde de la cama (`bed_approaching`), una tarea que exige procesar información espacial volumétrica compleja.

### Resumen de la distinción técnica
El sistema mana-lite sigue un modelo de **PLC** donde se separa la evidencia simple de las responsabilidades de componentes especializados. La tabla de señales asume la **"evidencia simple"** (como presencia o confianza), pero deja la **"lógica real"** y el manejo de estados temporales o espaciales a los motores que ya los resuelven eficazmente:

| Componente | Motivo de permanencia como variante propia |
| :--- | :--- |
| **Salud** | Gestiona la integridad del flujo de datos y estados de error mediante su propio motor. |
| **Profundidad** | Requiere evaluación geométrica contra mapas de profundidad, no comparaciones escalares. |
| **Zonas** | Necesita gestionar histéresis y timers clínicos (dwell) que el motor de señales no posee. |

En conclusión, forzar estos guards dentro de la tabla de señales obligaría a reconstruir resultados complejos a partir de booleanos simples, lo que aumentaría el riesgo de errores y degradaría la robustez del lazo de control clínico.


## ¿Cómo gestiona el motor la salud y profundidad?


El motor gestiona la salud y la profundidad manteniendo sus **motores especializados fuera de la tabla de señales genérica**, ya que estas responsabilidades requieren una lógica compleja que va más allá de una simple comparación de valores etiquetados,,.

A continuación se detalla cómo opera cada uno dentro de la arquitectura:

### 1. Gestión de la Salud (Health)
La salud del sistema no se trata como una señal común, sino que depende de un componente dedicado con su propia **máquina de estados interna**,.
*   **Responsabilidad:** Es el encargado de determinar si la evidencia es "fresca" (`data_fresh`) o "vieja" (`data_stale`),.
*   **Estado Seguro:** Si el motor de Health detecta que no hay señal de cámara o que los datos son obsoletos, es el responsable de conducir al sistema hacia el estado **blind** (ciego), que es el estado de seguridad clínica del equipo,.
*   **Interacción con la FSM:** Los guards de salud (`data_stale` y `data_fresh`) conservan sus variantes explícitas en Rust y leen directamente del componente Health en cada tick, en lugar de consultar la tabla de señales,.

### 2. Gestión de la Profundidad (Depth)
La profundidad se gestiona mediante un **evaluador de profundidad especializado** que procesa información volumétrica de la escena,.
*   **Evaluación de Reglas:** A diferencia de una señal escalar, el guard de profundidad (`depth_rule`) debe evaluar un snapshot de reglas contra el **mapa de profundidad** capturado por la cámara.
*   **Hitos Clínicos:** Se utiliza para detectar condiciones espaciales críticas, como cuando un paciente se acerca al borde de la cama (`bed_approaching`), una transición fundamental en el programa de prevención de caídas.
*   **Integración:** Al igual que con la salud, este motor expone sus resultados directamente a los guards que los necesitan durante el ciclo de control, preservando su lógica geométrica específica,.

### ¿Por qué no se integran en la tabla de señales?
El sistema separa deliberadamente la **evidencia simple** (que va a la tabla) de las **responsabilidades especializadas** por las siguientes razones:
*   **Lógica Real:** Estos componentes poseen "lógica real" y estados temporales o espaciales que no se pueden reducir a una comparación de "etiqueta, operador y valor",.
*   **Modelo PLC:** El sistema opera como un PLC donde la tabla de señales es solo un snapshot de evidencia; forzar motores complejos (como el de profundidad o el de salud) dentro de ella aumentaría el riesgo de errores y degradaría la robustez del lazo de control clínico,.
*   **Determinismo:** Al mantener estos motores independientes, el sistema garantiza que cada componente resuelva su especialidad antes de que la FSM tome una decisión final en el tick,.
