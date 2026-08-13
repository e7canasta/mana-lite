Sí, tiene sentido. El alcance correcto no es reconstruir una postura 3D, sino estimar **estado relativo a superficies conocidas** dentro de una escena fija.

**Modelo propuesto**

- `bed` se divide en zonas de referencia: `head`, `body`, `feet`.
- Cada zona guarda:
  - Polígono global.
  - Mediana de depth.
  - Envolvente robusta `p10..p90`.
  - `MAD` o dispersión.
  - Cobertura y modelo/ROI usados.
- `floor` guarda una o más zonas equivalentes. Si el piso tiene mucho gradiente por perspectiva, se divide en `near`, `middle` y `far`.
- No asumimos que `y` o el signo de depth indican directamente cerca/lejos. Cada zona aprende su propia referencia.

La terminología correcta sería **baseline espacial**, **envolvente de referencia** o **parches de superficie**, no necesariamente plano 3D.

**Interpretación runtime**

- `lying`: cabeza, torso y pies tienen alta cobertura dentro de la cama y sus residuos están dentro de las envolventes de cama.
- `sitting_on_bed`: pelvis/torso siguen asociados a cama, pero cabeza y torso tienen un residuo distinto al esperado para estar acostados.
- `sitting_at_edge`: pelvis en la zona `body/feet`, pies fuera de cama y compatibles con `floor`.
- `standing`: pies compatibles con piso; torso y cabeza fuera de la envolvente de cama.
- `limb_out`: mano o pie con baja intersección con cama, profundidad válida y persistencia temporal fuera de la zona.
- Estado `unknown` cuando la cobertura o la evidencia temporal no es suficiente.

La profundidad usada para comparar contra cama/piso debe ser `depth-scene`. `depth-person` sirve para comparar partes entre sí, pero no debe compararse directamente contra la calibración de la escena porque su crop es local.

**CLI**

Propondría un binario Rust, porque puede reutilizar exactamente el pipeline actual:

```bash
deep-calib --config config/mana.toml \
  --session config/deep-calib.toml \
  --layer bed --zone head

deep-calib --session config/deep-calib.toml \
  --layer bed --zone body

deep-calib --session config/deep-calib.toml \
  --layer bed --zone feet

deep-calib --session config/deep-calib.toml \
  --layer floor --zone main
```

La sesión se actualizaría atómicamente y guardaría polígonos, muestras y estadísticas. Después existiría una promoción explícita hacia runtime, por ejemplo:

```bash
deep-calib --session config/deep-calib.toml --promote
```

No conviene sobrescribir automáticamente `depth-rules.toml`.

**Punto importante**

La cama actual configurada en `config/zones.toml` (`[100,200,500,800]`) no coincide con el ROI depth actual (`[560,140,1240,820]`). Antes de calibrar hay que validar esa alineación; de lo contrario las referencias y las partes corporales podrían estar en espacios distintos.

Implementaría el sprint en este orden:

1. Schema `deep-calib.toml` y estadísticas por zona.
2. CLI Rust con captura y polígonos reproducibles.
3. Validación de ROI/zona y generación de referencias `bed`/`floor`.
4. `bed_residual`, `floor_match` y cobertura por body part.
5. Clasificador temporal de los cuatro estados.
6. Interfaz de selección con clicks, si la entrada por coordenadas no es suficiente.

Para la primera versión usaría rangos robustos por subzona. Si alguno resulta demasiado ancho por perspectiva, lo dividimos o añadimos una superficie local afín, sin introducir reconstrucción 3D completa.
