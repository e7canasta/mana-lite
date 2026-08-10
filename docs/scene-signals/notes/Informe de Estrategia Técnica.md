Informe de Estrategia Técnica: Transformación de la Agilidad Clínica mediante la Tabla de Señales

1. Visión General y Objetivos de Negocio

La transición hacia una arquitectura basada en una tabla de señales no constituye un mero refactor técnico; representa un imperativo estratégico para desacoplar el ciclo de vida del software del ciclo de vida de la práctica clínica. En sistemas de grado médico, la capacidad de respuesta debe residir en la configuración operativa y no en la recompilación de binarios. Esta transformación redefine el sistema como una plataforma flexible capaz de evolucionar a la velocidad de la necesidad clínica, garantizando al mismo tiempo una seguridad operativa absoluta.

Análisis del "Status Quo"

El modelo actual presenta una limitación crítica descrita en el ADR-032: "cambiar una alerta es un release". La lógica de respuesta —como el tiempo de espera (dwell) para una alerta de salida de cama— está fusionada con el código fuente en Rust. Esta rigidez implica que cualquier ajuste en un parámetro clínico exige un ciclo completo de desarrollo, validación y despliegue en hardware. Este acoplamiento binario genera un cuello de botella que asfixia la agilidad del producto y aumenta el riesgo operativo al forzar despliegues frecuentes de binarios completos para cambios paramétricos menores.

Directiva de Valor

Para mitigar este pasivo, la estrategia se centra en tres objetivos primordiales:

* Reducción de Ciclos de Despliegue: Sustitución de releases de software por actualizaciones de configuración (TOML), permitiendo ajustes clínicos en minutos.
* Personalización por Servicio (Blueprints): Capacidad de desplegar lógicas diferenciadas para una UTI o una sala general utilizando un binario único y validado.
* Observabilidad del Gemelo Digital: Transformación de la escena clínica en un flujo de datos transparente que permite la auditoría y reconstrucción precisa de incidentes.

Esta redefinición técnica es el cimiento necesario para evolucionar hacia un sistema de control reactivo y determinista.

2. El Problema del Acoplamiento Binario

Como estrategas de sistemas, debemos distinguir entre el "costo mecánico" del desarrollo y el "costo de oportunidad" de la rigidez clínica. Aunque editar código parece una tarea simple, la carga de validación de un binario clínico completo cada vez que se ajusta un umbral de confianza facial es inasumible para una escala hospitalaria.

Evaluación de la Deuda Técnica

El modelo actual de "guards" cableados crece de forma lineal y permanente, convirtiéndose en un pasivo técnico que dificulta el mantenimiento.

Dimensión	Modelo Actual (Acoplado)	Modelo Propuesto (Contrato)
Esfuerzo Mecánico	6 ediciones en 4 archivos por regla.	1-2 ediciones (Registro de señal).
Escalabilidad	Crecimiento de 18 variantes sincronizadas.	1 variante genérica para la mayoría.
Mantenimiento	Ediciones manuales en fsm/guard.rs.	Catálogo centralizado y tipado.
Costo de Cambio	Despliegue de binario completo.	Actualización de Blueprint (TOML).

Análisis de Interfaz

El enfoque inicial del ADR-031, basado en umbrales de crecimiento de código, era insuficiente por ser puramente reactivo. El ADR-032 redefine la tabla de señales como una interfaz de contrato. En sistemas de misión crítica, esperar a que la complejidad sea inmanejable para definir la interfaz es un error de arquitectura; el contrato debe preceder al consumo para evitar migraciones costosas. La solución no busca ahorrar líneas de código, sino habilitar una capa de configuración externa robusta.

3. Arquitectura de la Tabla de Señales como Contrato

La tabla de señales se define como un "Contrato Publicado". Esta capa de abstracción garantiza que la escena sea interoperable con sistemas externos (Gateways HL7, Workflows) sin que estos requieran conocimiento de las estructuras internas de Rust.

Desglose de la Semántica de Datos

El contrato impone tipos estrictos y operadores específicos para evitar ambigüedades clínicas. Un detalle crítico de seguridad es que el tipo Ratio debe rechazar valores NaN o infinitos, y el sistema prohíbe explícitamente comparaciones de igualdad exacta para evitar errores de precisión en el lazo de control.

Tipo	Semántica Clínica	Operadores Válidos
Bool	Estado binario (ej. presencia).	==, !=
Count	Conteo de entidades (entero ≥ 0).	==, !=, >=, <=, >, <
Ratio	Proporción [0.0, 1.0] (finitud obligatoria).	>=, <=, >, < (Prohibido ==)
Label	Categoría de conjunto cerrado.	==, !=

Reglas de Evolución

Para proteger la integridad del sistema, se aplican reglas de compatibilidad de contrato:

* [ ] Agregar un SignalTag: Siempre compatible.
* [ ] Agregar variante a un Label: Compatible solo si los consumidores tratan lo desconocido como "no coincide".
* [ ] Quitar o renombrar tags: Prohibido (rompe consumidores).
* [ ] Cambiar tipo o rango: Prohibido (ej. cambiar Ratio a porcentaje 0-100).

Este vocabulario declarado (SignalTag) evita la fragmentación semántica y asegura que etiquetas como cara.en_dwell mantengan un significado unificado en toda la plataforma.

4. Transformación de la Agilidad mediante Blueprints

Los Blueprints son el mecanismo de entrega de variaciones clínicas sin recompilación. Permiten que un servicio de UTI configure una sensibilidad alta en la confianza facial, mientras que una sala general priorice la reducción de fatiga de alarmas, todo operando sobre el mismo motor determinista.

Análisis de Capacidades

El sistema actual colapsa 11 guards simples (basados en 8 señales base y 1 latch de historial) en una única variante genérica. Esto permite que el 60% de la lógica de decisión sea configurable externamente, mientras que los componentes con lógica especializada (Zonas, Health, Profundidad) permanecen como módulos dedicados para preservar su integridad matemática.

Caso Ancla: Prevención de Caídas

En el protocolo de prevención de caídas:

* Lo que permanece especializado: La lógica de histéresis de zonas y el temporizador dwell de 3 segundos (que requieren estado temporal).
* Lo que se convierte en señal: La evidencia de respaldo, como cara.confianza y persona.presente.
* Impacto: Si un hospital requiere que la alerta de "Cama Vacía" sea validada por una confianza facial superior a 0.5, esto ahora se define en el Blueprint. El sistema valida esta regla en el arranque, garantizando que la decisión se base en evidencia auditable.

5. Seguridad Operativa y Modelo PLC

Adoptamos el modelo de Controlador Lógico Programable (PLC): el sistema "compila en boot" y "ejecuta de forma determinista". Esto garantiza que la flexibilidad de la tabla de señales no degrade la seguridad clínica del lazo de control.

Mecanismos de Validación

El compilador FsmProgram::compile() actúa como guardián absoluto. Antes de iniciar el primer tick, el sistema debe rechazar y detener el arranque si detecta:

1. Tags inexistentes: Referencias a señales no declaradas por ningún productor.
2. Operadores incompatibles: Ej. realizar comparaciones de magnitud (>) en booleanos.
3. Igualdad en Ratios: Intento de comparación == en valores de punto flotante.
4. Valores fuera de rango: Ratios menores a 0 o mayores a 1.
5. Labels inválidos: Comparaciones contra variantes inexistentes en el catálogo.

Gestión de Ausencias y fallas

En este modelo, la ausencia de señal no es equivalente a false. La ausencia indica "evidencia inexistente" (ej. cuando no hay un área de interés configurada). Si un productor interno falla durante un tick, el sistema genera un SignalFault, forzando una transición a un "Estado Seguro" (Blind) en lugar de operar con datos corruptos o antiguos. Esta distinción protege al sistema de tomar decisiones basadas en supuestos erróneos.

6. Observabilidad y el Gemelo Digital

La opacidad actual de la escena impide una auditoría efectiva. Sin un snapshot de señales, es imposible reconstruir por qué una alerta no se disparó ante un incidente clínico.

Impacto en el Diagnóstico

El nuevo evento SceneSignals vuelca la tabla completa de etiquetas en cada tick. Esto permite una Reconstrucción de Incidentes con fidelidad total: el log documentará exactamente qué confianza facial o presencia vio el sistema en el milisegundo exacto de la decisión.

Sustitución de Fixtures

Esta transparencia permite eliminar la dependencia de archivos de texto externos como multi_actor_cycle.events.txt. Al ser los logs estructurados y deterministas una representación fiel del gemelo digital, la validación del sistema se vuelve intrínseca y reduce la deuda técnica en las pruebas de regresión.

7. Hoja de Ruta de Implementación (Estrategia de Sprints)

La implementación sigue la metodología de "primero la red, después el cambio", manteniendo los invariantes clínicos byte a byte hasta la fase final.

Fases y Compuertas de Cierre Mecánicas

* Etapa A (Vocabulario): Implementación del catálogo y tipos.
  * Compuerta: grep debe demostrar cero consumidores externos a su módulo; validación de rechazo de Ratio fuera de rango.
* Etapa B (Producción en Paralelo): Poblar la tabla sin consumirla.
  * Compuerta: Test de paridad tick a tick entre el contexto antiguo y la nueva tabla.
* Etapa C (Guard Genérico): Migración de los 11 guards, un commit por guard para mitigar riesgos.
  * Compuerta: Falla demostrada del sistema al cargar catálogos con tags inexistentes o tipos inválidos. Reducción de variantes de FsmGuard de 18 a 8.
* Etapa D (Visibilidad Total): Activación del volcado de señales y limpieza.
  * Compuerta: El golden log previo debe ser un prefijo exacto del nuevo; el volcado debe incluir los 9 tags (8 base + 1 latch).

Gestión de Riesgos

Para mantener la estabilidad, ciertos componentes NO se convertirán en señales en esta fase: motores de zonas (histéresis), salud del sistema (Health) y reglas de profundidad. Al finalizar la Etapa D, el sistema alcanzará la paridad clínica total con una capacidad de inspección radicalmente superior.

8. Conclusiones y Próximos Pasos

Esta estrategia transforma el sistema de un binario rígido en una plataforma clínica dinámica y auditable. Al tratar la escena como un contrato publicado y validar cada regla mediante un modelo PLC, garantizamos agilidad sin comprometer la seguridad del paciente.

El establecimiento de la tabla de señales habilita un futuro de interoperabilidad total, incluyendo consumidores externos, flujos de trabajo personalizados y gateways HL7. El éxito de esta transformación se mide por la paridad clínica absoluta y la capacidad de responder a cualquier incidente con la transparencia total que solo un gemelo digital puede ofrecer.
