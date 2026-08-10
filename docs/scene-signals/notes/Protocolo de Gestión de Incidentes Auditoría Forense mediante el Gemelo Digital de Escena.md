Protocolo de Gestión de Incidentes: Auditoría Forense mediante el Gemelo Digital de Escena

1. Fundamentación Estratégica: El Cambio de Paradigma en la Observabilidad Clínica

Este protocolo establece la obligatoriedad de la reconstrucción determinista de incidentes en el sistema "mana-lite". La transición de un modelo de "caja negra" —donde la lógica de escena dependía de campos internos de Rust y logs definidos ad-hoc— hacia un Gemelo Digital basado en una Tabla de Señales transparente, representa un cambio fundamental en la gestión de riesgos clínicos. Bajo este nuevo paradigma, la visibilidad del estado interno deja de ser un subproducto del desarrollo para convertirse en un contrato publicado y auditable.

Es imperativo notar que, durante las Etapas A a C de la implementación, se ha mantenido el Invariante de Comportamiento: la lógica clínica es byte-idéntica a la anterior. La evolución radica en la transparencia; la capacidad de validar invariantes técnicos sin necesidad de que el equipo de ingeniería intervenga en la extracción de memoria o la recompilación de binarios.

Dimensión	Modelo Anterior (Caja Negra)	Modelo de Gemelo Digital (Señales)
Acoplamiento	Vinculado a campos internos de Rust.	Contrato publicado y versionado (TOML).
Visibilidad de Logs	Logs manuales que a menudo descartaban eventos clave (Occupancy/FsmState).	Volcado determinista del Gemelo Digital (completo por ciclo).
Auditoría	Requiere inferencia y personal de desarrollo.	Auditoría directa por personal clínico/técnico sobre el contrato.
Validación	Fallos detectados en runtime (opacos).	Validación estricta en el arranque (modelo PLC).

Esta transparencia del gemelo digital reduce drásticamente los tiempos de investigación de incidentes. Al desacoplar la evidencia (señales) de la decisión (binario), la institución puede discernir con precisión si un evento adverso fue producto de una detección insuficiente o de una configuración de umbrales clínicos inapropiada en el blueprint.

2. El Contrato de Señales: Vocabulario Declarado para la Reconstrucción de Eventos

La validez de una auditoría forense depende de un vocabulario estandarizado y versionado. El uso de tags estrictos evita la ambigüedad semántica: un incidente no se analiza bajo términos subjetivos, sino bajo señales cuya semántica y tipo están predefinidos en el Catálogo Canónico v1.

Catálogo Canónico v1 de Señales

* persona.presente (Bool): Indica presencia de al menos una detección. Responde si el sistema "sabía" que la habitación estaba ocupada.
* persona.cantidad (Count): Conteo crudo de personas. Identifica situaciones de sobreocupación.
* cara.presente (Bool): Indica si existe una cara seleccionada para el análisis en el ciclo actual.
* cara.confianza (Ratio): Certeza matemática (0.0 a 1.0). Permite validar si la calidad de visión era suficiente para la toma de decisiones.
* cara.en_dwell (Bool): Intersección con la Región de Interés (ROI) de permanencia. Crítico para alertas de salida de cama.
* cara.en_borde (Bool): Indica si la persona seleccionada se encuentra en el límite de la zona de seguridad.
* cara.modelo_corrio (Bool): Validación de integridad del pipeline; confirma si el motor de inferencia facial se ejecutó.
* ocupacion.cardinalidad (Label): Clasificación cerrada (empty, single, multiple). Esencial para descartar interferencias por múltiples actores.
* cara.estuvo_dentro (Bool): Señal derivada (latch) que preserva el historial de permanencia, fundamental para reconstruir la lógica de abandono de zona.

La Semántica de Ausencia como Blindaje Legal: Este protocolo distingue estrictamente entre un valor false (evidencia negativa) y un valor ausente (falta de configuración). En el caso de cara.en_dwell, un valor ausente deslinda al software de responsabilidad técnica si el incidente ocurre en un entorno donde el Blueprint clínico no definió una ROI de permanencia. La ausencia indica que la capacidad no fue solicitada, no que el sistema falló en detectar.

3. Metodología de Auditoría: Análisis del Evento de Volcado (Dump)

El flujo de trabajo de auditoría se centra en el evento SceneSignals. Este volcado no es una simple lista de logs, sino un snapshot sincronizado con el ControlStamp, garantizando que la evidencia y la decisión pertenezcan exactamente al mismo instante temporal.

El Factor de Frescura de Datos

El auditor debe inspeccionar en el ControlStamp los campos observations_age_ms y depth_age_ms. Estos valores permiten responder con autoridad: "¿Estaba el sistema operando con datos 'stale' (viejos) durante el incidente?". Si la latencia de datos excede los umbrales de seguridad, el análisis se desplaza hacia la salud del transporte y no hacia la lógica de detección.

Proceso de Reconstrucción Forense (4 Pasos)

1. Localización: Identificación del scan_seq y evidence_frame_id en el momento del reporte clínico.
2. Extracción: Aislamiento del snapshot de la SignalTable del log JSONL.
3. Correlación: Comparación entre los valores de las señales (evidencia) y la transición de estado del FSM.
4. Validación de Invariantes: Verificación de la ausencia de SignalFault.

Definición de SignalFault (Red Flag): Un SignalFault registrado en el log es un indicador crítico de fallo de integridad. Representa un "defecto interno" donde un productor de señales intentó emitir un dato que contradice el catálogo (ej. un Ratio fuera de rango). El sistema, por diseño, entra en un estado seguro ante un SignalFault. Su presencia en un incidente señala un fallo de programa, no un error de configuración.

4. Interpretación de Evidencia Crítica: Confianza, Umbrales y Ratios

En este protocolo, la interpretación de la evidencia es determinista. El sistema adopta el modelo PLC: la configuración del programa se valida al arranque y se rechaza cualquier definición que viole los invariantes de seguridad.

Invariantes de los Guards Genéricos

* Prohibición de Igualdad en Ratios: El sistema rechaza en el arranque (boot-time) cualquier regla que intente una comparación exacta (==) sobre señales de tipo Ratio. Esta restricción elimina "bugs esperando a suceder" (bugs waiting to happen) por imprecisión de punto flotante, protegiendo la estabilidad del lazo clínico.
* Validación de Ratios: Si cara.confianza < min_confidence, el sistema no está ignorando al paciente. El auditor debe certificar que, según el contrato, la evidencia era insuficiente para activar una transición de alta sensibilidad.
* Umbrales por Servicio: El protocolo permite verificar si un incidente en la UTI fue causado por un umbral (dwell) diseñado para Sala General. Al estar estas definiciones en el TOML del blueprint y no en el binario, la auditoría puede concluir si el error reside en la prescripción técnica del servicio sin necesidad de intervenir el código fuente.

5. Límites del Protocolo: Componentes de Lógica Especializada

La Tabla de Señales no intenta aplanar la complejidad total de la escena. Ciertos componentes requieren su propio análisis especializado debido a su naturaleza temporal y espacial.

* Motor de Zonas: Excluido de las señales genéricas para preservar la integridad de la histéresis y los timers espaciales. Una salida de zona no es un simple booleano; es una acumulación temporal que no debe ser simplificada.
* Health (Salud del Sistema): Evalúa la frescura y la ruta hacia el estado blind. Su lógica de "keep-alive" es independiente del contrato de escena.
* Reglas de Profundidad: Basadas en snapshots de nubes de puntos; su auditoría exige revisar el mapa de profundidad y no solo la tabla de señales.

Esta separación garantiza que eventos complejos, como la salida de cama, mantengan su rigor clínico al no ser forzados en una tabla de predicados simples.

6. Gobernanza y Evolución del Protocolo de Auditoría

El "Contrato Publicado" es la base de la responsabilidad institucional. Para garantizar que el conocimiento acumulado y las herramientas de auditoría externa sigan siendo válidos, se establecen reglas de evolución estrictas:

Acción	Impacto en Auditoría	Compatibilidad
Agregar un Tag	Amplía la capacidad forense.	Compatible
Renombrar/Quitar Tag	Invalida historiales legales y herramientas.	No Compatible
Cambiar semántica de Ratio	Altera fundamentalmente la interpretación legal.	Prohibido

Conclusión: Este protocolo desplaza la carga de la prueba de la "confianza en una caja negra" a la "auditoría de un gemelo digital". Al proporcionar una visibilidad determinista y validada en el arranque, mana-lite se posiciona como un referente de responsabilidad técnica. La capacidad de responder con exactitud técnica a la pregunta "¿Por qué el sistema tomó esta decisión?" es, en última instancia, la mayor garantía de seguridad para el paciente y la institución.
