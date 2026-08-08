# ADR-020 — Flag `enabled` por modelo

**Estado:** Accepted
**Fecha:** 2026-08-06
**Contexto:** las ramas del cascade (face, pose, seg) son hermanas e
independientes. Se necesita apagar ramas desde TOML para controlar coste
computacional. El mecanismo existente `inference.disabled_tasks` opera por
`task` (ej. `detect`), lo que apagaría también a `detect-fast` (raíz del
cascade).

**Decisión:** agregar `enabled: bool` a `ModelEntry` (default `true`) en
`config/models.toml`. El scheduler (`App::resolve_models` en `src/main.rs`)
excluye modelos con `enabled = false`, antes del filtro por `disabled_tasks`.
Un modelo deshabilitado no se programa ni se infiere; sus reglas de cascade
siguen siendo válidas (el toggle es de ejecución, no de config).

**Consecuencias:**
- Backward compatible: configs existentes sin el campo siguen funcionando
  (serde default `true`).
- El orden de filtrado es: `enabled` → `disabled_tasks`.
- `enabled = false` sobre `face-yolo` o `pose-standard` reduce coste sin
  tocar `detect-fast`.

## Interaction with Blueprints

When `inference.blueprint_file` is set, `blueprint.models` is the explicit
runtime selection and takes precedence over the catalog `enabled` values for
the selected process. The catalog flag remains the default mechanism for
legacy configurations without a blueprint.
