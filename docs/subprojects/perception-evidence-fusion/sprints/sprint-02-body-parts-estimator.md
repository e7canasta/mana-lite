# Sprint 2: Body Parts Estimator

**Estado:** implementado inicialmente
**Objetivo:** generar una geometria explicable y parcial de partes corporales a
partir de face, pose, segmentacion y calidad disponible.

## Alcance

- definir tipos internos para `BodyPartsEstimate` y `BodyPartEstimate`;
- consumir la evidencia rica antes de la consolidacion stateless;
- implementar cabeza y tronco primero;
- agregar brazos y piernas con segmentos ensanchados;
- usar face como ancla de cabeza;
- usar mascara para refinar cobertura y borde;
- mantener calidad, fuentes y frame por parte;
- visualizar resultado en Rerun o sink de diagnostico.

La primera implementación publica el resultado como evento JSONL
`type=body_parts`. Rerun conserva los dibujos de pose y máscara existentes; la
geometría derivada no entra todavía al contrato clínico.

## Orden recomendado

1. cabeza;
2. tronco;
3. brazos;
4. piernas;
5. manos y pies, solo si los joints y la resolucion lo justifican.

## Criterios de aceptacion

- cada parte tiene calidad independiente;
- un joint ausente produce una parte parcial, no una persona invalida;
- la geometria usa frame original y no doble-aplica el offset de crop;
- el MVP exige un actor confirmado o declara la salida `FrameLocal` sin historial;
- la mascara original no se modifica;
- el soporte local de una parte no depende del convex hull global;
- cada poligono o capsula puede explicar sus fuentes;
- el resultado stale es visible como stale.

## Salida inicial

- `head`: bbox de face, con joints de cabeza como corroboración;
- `torso`: polígono hombros-caderas o geometría parcial;
- brazos y piernas: polilíneas con radio relativo al bbox del actor;
- máscara: cobertura local y calidad, sin mutar `DetectionMask`;
- `Track(id)` para targets confirmados y `FrameLocal` sin historial cuando falta
  identidad;
- `stale=false` en modo `validator`; el modo `advanced` puede marcar geometría
  recuperada desde el historial del track como `stale=true`.

Los umbrales, radios y pesos no están en el mecanismo Rust: se cargan desde
`[perception.body_parts]` en `config/mana.toml`. La validación cruzada usa la
sección hermana `[perception.validation]`. El modo `advanced` es opt-in y exige
respaldo de la máscara para reutilizar geometría temporal.

## No hacer

- presentar el poligono derivado como ground truth;
- enviar geometrias crudas al FSM;
- modificar `Detection`, `DetectionEvidence` o `ConsolidatedObservation` para
  recuperar keypoints;
- forzar simetria corporal cuando la evidencia no la soporta;
- usar una inclusion binaria como unico criterio de calidad.
