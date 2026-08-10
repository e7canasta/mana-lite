Guía de Lógica de Datos: Del Código Interno al Contrato de Señales

Como Arquitecto de Sistemas Clínicos, mi prioridad no es simplemente que el código compile, sino que el sistema sea intrínsecamente seguro, determinista y auditable. En un entorno hospitalario, la forma en que estructuramos los datos no es una preferencia estética; es un contrato de seguridad clínica.

Esta guía detalla la transición de una lógica de programación rígida basada en campos internos hacia un modelo de Tabla de Señales, transformando el "Gemelo Digital" de la habitación en una interfaz pública, inmutable y verificable en tiempo de arranque.

1. La Evolución del Gemelo Digital: De Campos a Señales

Históricamente, el estado de una escena se capturaba en un struct plano de Rust (FsmSceneContext). Cualquier nueva necesidad clínica —como añadir una validación de presencia facial— requería modificar el núcleo del sistema.

Siguiendo los lineamientos de los ADR-031 y ADR-032, hemos migrado hacia una Tabla de Señales Etiquetadas. Esta arquitectura preserva el split entre FsmGuard (la intención en el TOML) y ProgramGuard (el programa compilado), emulando el rigor de un PLC industrial: se compila en el arranque y se ejecuta de forma determinista.

Comparativa de Evolución Arquitectónica

Característica	Antes (Estructura Plana)	Después (Tabla de Señales)
Costo de Desarrollo	6 ediciones en 4 archivos por cada regla nueva.	1 a 2 ediciones (registrar el productor de la señal).
Visibilidad	Detalle interno: solo el programador conoce la estructura.	Contrato Publicado: accesible para auditorías e integraciones.
Impacto Clínico	Requiere un nuevo binario (release) para cambiar un umbral.	Cambio en el blueprint TOML; permite umbrales específicos por servicio (UTI vs. Sala General).

Narrativa de aprendizaje: Esta estructura no es solo un ahorro de tiempo para el equipo de ingeniería; es la garantía de que el sistema puede adaptarse a la sensibilidad clínica de diferentes unidades médicas sin alterar la integridad del binario certificado.

2. Anatomía de los Tipos de Datos y su Semántica

Un dato clínico sin restricciones es un riesgo. Por ello, los cuatro tipos de SignalValue poseen reglas de construcción y operación estrictas, definidas en el contrato público del sistema.

* Bool (Booleano): Representa estados binarios puros. Solo admite operadores de igualdad (==, !=). Es el cimiento de la presencia y la detección.
* Count (Conteo): Un entero no negativo de ancho fijo. A diferencia de los tipos nativos que pueden variar según el procesador, el ancho fijo garantiza un comportamiento independiente de la arquitectura, asegurando que la lógica sea idéntica en cualquier hardware desplegado.
* Ratio (Proporción): Representa valores en el rango [0.0, 1.0].
  * Integridad del Dato: Es un tipo opaco. Su constructor rechaza explícitamente valores NaN, Infinitos o fuera del rango definido.
  * Restricción Crítica: Se prohíbe la comparación por igualdad (==). En lazos de control, comparar flotantes por igualdad genera "jitter" o fallos de alerta; se restringe estrictamente a operadores de umbral (>, <, >=, <=).
* Label (Etiqueta): Define la naturaleza de un objeto mediante un conjunto cerrado y conocido (ej. ocupación: empty, single, multiple).

Narrativa de aprendizaje: Estos tipos son versionados e inmutables. Cualquier cambio en el rango o semántica de un tag se considera un "Breaking Change" que exige un nuevo tag y un periodo de convivencia, asegurando que el sistema sea auditable sin necesidad de leer una sola línea de código fuente en Rust.

3. El Cruce Clínico: Falso vs. Ausente

Confundir una observación negativa con la falta de datos es un error crítico en la seguridad del paciente. La tabla de señales introduce la distinción semántica entre un valor false y una señal ausente.

Caso de Estudio Clínico: El estado de la mirada (cara.en_dwell)

Estado de la Señal	Resultado Lógico	Significado en la Habitación
True	Coincide	La cara está dentro del área de interés (ROI).
False	No coincide	La capacidad está activa, pero la cara está fuera del ROI.
Ausente	Nunca coincide	No hay ROI definido en la configuración de este cuarto.

Regla de Oro: Una señal Ausente nunca satisface un guard, incluso si el operador es de desigualdad (!=). Si el sistema tratara la "Ausencia" como "Falso", un cuarto mal configurado podría ignorar riesgos reales. En su lugar, la ausencia es consumida por el componente de Health, que activa la ruta segura hacia el estado blind (ciego), alertando al personal de que el sistema no tiene evidencia suficiente para operar.

4. Validación en el Arranque (Modelo PLC)

Fieles a la filosofía de los sistemas de control industrial, aplicamos la "Validación en Boot vs. Runtime". Un programa de seguridad no debe aceptar reglas nuevas "en caliente", ya que esto destruiría el determinismo del lazo de control.

El compilador de programas (FsmProgram::compile) sustituye los chequeos genéricos por un sistema de errores acumulados. El sistema detecta y rechaza 5 fallos críticos antes de iniciar:

1. Tag inexistente: Uso de una etiqueta no declarada por ningún productor.
2. Operador incompatible: Intentar, por ejemplo, un > sobre un tipo Bool.
3. Igualdad en Ratio: El uso de == sobre proporciones es rechazado por el compilador.
4. Valor fuera de rango: Configuraciones imposibles (ej. Ratio de 1.5).
5. Etiqueta inválida: Comparar un Label contra un valor fuera de su catálogo cerrado.

[BOOT ERROR] Transition 'watching -> bed_alert': Tag 'cara.confianza' (Ratio) uses invalid operator '=='. Expected threshold operators (>=, <=, >, <).

Narrativa de aprendizaje: Esta validación exhaustiva garantiza que, una vez que el sistema arranca, el programa es lógicamente perfecto, eliminando fallos imprevistos durante el monitoreo activo del paciente.

5. El Gemelo Digital Visible y la Observabilidad

La implementación de la Etapa D de nuestra arquitectura convierte la escena en una "caja de cristal". Mediante el evento SceneSignals y el uso del ControlStamp (sincronizando scan_seq y evidence_frame_id), el gemelo digital se vuelve totalmente observable.

¿Qué preguntas podemos responder tras un incidente?

* "¿Qué confianza exacta de detección facial veía el sistema antes de la caída?"
* "¿Estaba la señal de presencia en false o estaba ausente por falta de configuración?"
* "¿Por qué no se disparó la alerta si la persona estaba en el borde de la cama?"

Esta arquitectura elimina la necesidad de depender de "Golden JSONL" parciales o registros de texto ambiguos. Al convertir la escena en un Contrato Publicado, permitimos que sistemas externos (Gateways HL7, tableros de enfermería) consuman la realidad de la habitación con la certeza de que están leyendo un snapshot matemático exacto, sin necesidad de conocer las complejidades internas de Rust. Hemos transformado el código en un lenguaje clínico universal.
