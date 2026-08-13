Implementado y commiteado.

**Commits**
- `f961321` `feat(perception): add diagnostic depth evidence for body parts`
- `6f6d682` `feat(depth): add relative surface calibration`
- `bba6f1f` `fix(depth): validate surface calibration context`

**Incluido**
- `deep-calib` como binario Rust aislado del comando principal.
- Sesiones TOML reanudables y escritura atómica.
- Zonas `bed/head`, `bed/body`, `bed/feet` y `floor/*`.
- Estadísticas robustas: mediana, p10, p90, MAD y cobertura.
- Fingerprint del modelo, ROI y resolución.
- Promoción explícita de sesión.
- Evidencia `bed/floor` por body part usando únicamente `depth-scene`.
- Intersección real entre huella corporal, máscara y zona de superficie.
- JSONL y Rerun con `residual` e `in_envelope`.
- Memoria técnica, spec, ADR-004 y Sprint 4.

**Uso actual**

```bash
cargo run --bin deep-calib -- \
  --session config/deep-calib.toml \
  --image /path/frame.png \
  --layer bed \
  --zone head \
  --polygon "560,140;800,140;800,300;560,300"
```

Se pueden repetir `--image` para acumular muestras. Para piso:

```bash
cargo run --bin deep-calib -- \
  --session config/deep-calib.toml \
  --image /path/frame.png \
  --layer floor \
  --zone main \
  --polygon "0,500;800,500;800,900;0,900"
```

Promoción explícita:

```bash
cargo run --bin deep-calib -- \
  --session config/deep-calib.toml \
  --promote config/deep-calib.promoted.toml
```

La activación runtime requiere configurar `inference.depth_calibration_file` y habilitar el modelo depth correspondiente. La selección interactiva con clicks y captura RTSP quedan como el siguiente adaptador; el núcleo ya no depende de esa UI.

**Validación**

- `cargo test --workspace --features rerun`: pasa.
- `cargo fmt --all -- --check`: pasa.
- `git diff --check`: pasa.
- Worktree limpio.
- Clippy global sigue bloqueado por warnings preexistentes de `mana-geometry`.
