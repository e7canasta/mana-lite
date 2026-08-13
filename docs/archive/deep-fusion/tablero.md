
Valores de **l-640 recalibrado**. Son valores del modelo, no metros físicos.

Referencia vacía:

```text
bed/head  1.329   bed/body  1.097   bed/feet  0.944
floor/head 1.773  floor/body 1.415   floor/feet 1.160
```

| Elemento | `acostado-1` | `sentado-1` |
|---|---:|---:|
| Face bbox, mediana | **1.421** `floor/body` | **1.174** `bed/head` |
| Face p10-p90 | 1.407-1.532 | 1.170-1.188 |
| Área `head` con máscara | 1.421 | 1.174 |
| Keypoints cabeza | 1.415-1.439 | 1.171-1.197 |
| Hombro izquierdo/derecho | 1.313 / 1.408 | 1.154 / 1.205 |
| Torso, área | 1.283 | 1.128 |
| Brazos, áreas izq./der. | 1.373 / 1.449 | 1.145 / 1.182 |
| Codos, rango | 1.350-1.461 | 1.145-1.181 |
| Muñecas, rango | 1.411-1.468 | 1.154-1.177 |
| Cadera izq./der. | 1.139 / 1.185 | 1.085 / 1.107 |
| Piernas, áreas izq./der. | 0.954 / 0.990 | 1.017 / 1.026 |
| Rodillas, izq./der. | 0.968 / 1.007 | 1.023 / 1.040 |
| Tobillos, izq./der. | **0.893 / 0.943** | **0.979 / 1.002** |

**Lectura**

- `acostado-1`: la cabeza/cara está más lejos que el torso, los brazos quedan alrededor de `1.37-1.45` y las piernas cerca de `bed/feet`. Es una firma coherente de acostado.
- `sentado-1`: cara, hombros y brazos quedan juntos en `bed/head`; la cadera baja a `bed/body` y las piernas a `bed/feet`. Es una firma coherente de sentado.
- La visualización Jet/Metric y los JSON están en:
  - `demo-deep-calib-radio/l-640/`
  - `demo-deep-calib-parts/results/*l-640.json`
