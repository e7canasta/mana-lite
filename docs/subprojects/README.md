# Subproyectos

Los subproyectos se documentan por contrato, no por historial de tareas.

Cada subproyecto conserva solamente:

- `README.md`: alcance y estado actual;
- `spec.md`: contrato funcional;
- `technical-memory.md`: evidencia, limites y decisiones empiricas;
- `adrs/`: decisiones que deben permanecer estables.

Los manuales operativos se conservan cuando existe un procedimiento reproducible
para ejecutar el componente. Los sprints, roadmaps, handoffs y diseños de fases
se destilan en esos documentos y no forman parte de la documentacion vigente.

## Activos

- [Scheduler cooperativo](cooperative-inference-scheduler/README.md): cadencia,
  latest-wins, urgencias y validacion face/pose.
- [Fusion de evidencias](perception-evidence-fusion/README.md): validacion
  cruzada, body parts, depth diagnostico y calibracion de superficies.
- [Analisis de postura](posture-analysis-engine/README.md): clasificacion offline
  por perfiles, consenso exacto/semantico y manual por imagen.

La historia completa permanece en el historial de Git. No se mantiene una
segunda narrativa de proyecto dentro del checkout activo.
