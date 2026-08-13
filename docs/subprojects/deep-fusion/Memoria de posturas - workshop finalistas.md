# Memoria de posturas — workshop finalistas (x-320 / l-640 / l-320)

Complementaria a "Memoria spec - comportamiento del modelo depth.md". Corridas: 7 posturas × 3 modelos finalistas, pose + face sobre la sesión de zonas (referencia same-run). Datos crudos en `demo-deep-calib/results/`, previews en `demo-deep-calib/persona/`. Reproducible con `deep-calib-pose --session config/workshop/deep-calib-<modelo>.toml --image samples/<postura>.jpeg --face --json ... --output ...`.

Geometría de cámara (recordatorio): cámara en la pared de los **pies**, picada hacia la cabecera → la cabecera (head) es el extremo lejano, los pies el cercano. Gradiente esperado del cuerpo en la cama: `bed/head → bed/body → bed/feet` de la cabeza a las rodillas.

Leyenda: `fz` = zona de la cara (bbox); profundidades en metros del modelo. "none" = keypoint sin confianza o sin detección.

---

## 1. acostado-1 — persona acostada en la cama

Cabeza sobre la almohada contra la cabecera (extremo lejano).

| parte | x-320 | l-640 | l-320 |
|---|---|---|---|
| face | 3.39 floor/feet | 1.43 floor/feet | 4.05 floor/feet |
| hombros | 2.93 bed/head · 3.12 floor/feet | 1.30 bed/head · 1.41 floor/feet | 3.55 bed/head · 3.76 bed/head |
| caderas | 2.47/2.55 bed/body | 1.08/1.12 bed | 2.94 bed/body · 3.15 bed/head |
| rodillas | 2.16 bed/feet · 2.26 bed/body | 0.87 bed/feet · 0.92 bed/body | 2.62 bed/feet · 2.85 bed/body |

Lectura: la cara lee lejos (amarillo) en los 3 — esperado por geometría (test B). El cuerpo queda azul con gradiente. Los 3 modelos coinciden.

## 2. sentado-1 — persona sentada en la cama

Incorporada a la cabecera; la cara se separa de la pared y lee sobre la cama.

| parte | x-320 | l-640 | l-320 |
|---|---|---|---|
| face | 2.10 bed/head | 0.97 bed/head | 2.13 bed/head |
| hombros | 2.10/2.14 bed/head | 0.95/0.99 bed/head | 2.07/2.19 bed/head |
| caderas | 1.97/2.00 bed/body | 0.90/0.92 bed | 1.86/1.90 bed/body |
| rodillas | 1.86 bed/feet · 1.94 bed/body | 0.84/0.85 bed/feet | 1.77 bed/feet · 1.84 bed/body |

Lectura: gradiente perfecto en los 3 (test A). Δface vs bed/head: x-320 +0.02, l-640 +0.03, l-320 +0.09. El caso canónico de "persona EN la cama".

## 3. sentado-borde-1 — persona sentada al borde de la cama

Tronco sobre el borde (zona bed/body, ya no cabecera); una pierna colgando al piso.

| parte | x-320 | l-640 | l-320 |
|---|---|---|---|
| face | 1.79 bed/body | 0.90 bed/body | 1.72 bed/body |
| hombros | 1.72/1.80 bed/body | 0.79/0.87 bed | 1.62/1.71 bed/body |
| caderas | 1.70/1.71 bed/body | 0.77/0.79 bed/feet | 1.58/1.60 bed/body |
| rodillas | 1.92 bed/head · **3.22 floor/head** | 0.88 bed/body · **1.53 floor/body** | 1.73 bed/body · **2.84 floor/head** |

Lectura: el cuerpo se desplaza de cabecera a borde (todo bed/body, face incluida — ya no bed/head) y **la rodilla del lado del borde lee piso** en los 3 modelos. La firma es coherente y consistente: "tronco en cama, pierna afuera".

## 4. foots-left-bed-1 — pies fuera de la cama (variante 1)

Persona apoyada al costado de la cama, tronco inclinado sobre el borde.

| parte | x-320 | l-640 | l-320 |
|---|---|---|---|
| face | 3.46 floor/body | 1.57 floor/body | 4.19 floor/body |
| hombros | 2.94/3.10 bed/head | 1.45 bed/head · 1.52 floor/body | 3.84/3.87 floor/body |
| caderas | 2.42/2.49 bed/body | 1.15/1.22 floor/feet | 3.14/3.31 floor/feet |
| rodillas | 2.08 bed/feet · 2.25 bed/body | 0.94/1.03 bed | 2.66 bed/feet · 2.99 bed/body |

Lectura: **la cara lee piso en los 3** (cabeza proyectada sobre el costado de la cama, fuera del borde). Hombros/caderas dudan entre bed y floor según modelo — es la franja de transición borde de cama. Las rodillas siguen leyendo bed (tronco inclinado sobre la cama).

## 5. foot-left-bed-2 — pies fuera de la cama (variante 2)

| parte | x-320 | l-640 | l-320 |
|---|---|---|---|
| face | **3.40 bed/head** | 1.38 floor/body | 3.75 floor/body |
| hombros | 3.13/3.26 bed/head | 1.30 bed/head · 1.36 floor/body | 3.48/3.56 bed/head |
| caderas | 2.60/2.66 bed/body | 1.08/1.12 bed | 2.86 bed/body · 2.95 floor/feet |
| rodillas | 2.27/2.47 bed | 0.92/0.97 bed | 2.58/2.82 bed |
| tobillo | — · 2.73 floor/feet | — · 1.04 bed/body | — · 3.00 floor/feet |

Lectura: **discrepancia en la cara**: x-320 la lee en bed/head (cabeza a la altura de la cabecera) mientras l-640/l-320 leen floor. El tobillo del lado del borde lee floor (pie apoyado en el piso). Igual que en parado: x-320 confunde la cara alta con la cama.

## 6. leaving-bed-aside-head-1 — saliendo de la cama por la cabecera

Persona todavía sobre la cama, saliendo de costado por la zona de la cabeza.

| parte | x-320 | l-640 | l-320 |
|---|---|---|---|
| face | 2.72 bed/head | 1.22 bed/head | 3.35 bed/head |
| hombros | 2.63/2.57 bed/head | 1.19/1.16 bed/head | 3.28/3.20 bed/head |
| caderas | 2.32/2.32 bed/body | 1.01/1.00 bed/body | 2.84/2.82 bed/body |
| rodillas | 2.27 bed/body · 2.23 bed/feet | 0.96 bed/body · 0.92 bed/feet | 2.79 bed/body · 2.73 bed/feet |
| tobillos | 2.18 bed/feet | 0.90 bed/feet | 2.62 bed/feet |

Lectura: **los 3 modelos leen el cuerpo completo en bed** con gradiente completo hasta los tobillos — persona aún sobre la cama. Máxima consistencia de la tanda.

## 7. parado-aside-1 — persona parada al costado de la cama

| parte | x-320 | l-640 | l-320 |
|---|---|---|---|
| face | **2.27 bed/head** | 1.31 floor/head | 1.96 floor/head |
| hombros | 2.13 floor/feet · — | 1.15 floor/head · — | 1.92 floor/head · — |
| caderas | 2.03 floor/body · 1.94 bed/body | 0.82/0.81 floor/body | 1.67 floor/body · 1.62 bed/body |
| rodillas | 2.11/2.13 floor/feet | 0.85/0.87 floor | 1.70/1.69 floor/feet |

Lectura: el cuerpo lee floor en los 3 (persona parada, fuera de la cama). **Discrepancia en la cara**: x-320 la lee en bed/head — la cabeza parada queda a la altura/profundidad de la cabecera (más cerca de la cámara por estar de pie) y el modelo la confunde con la cama. l-320/l-640 leen floor/head.

---

## Síntesis

- **Firma por postura**: los nombres de los samples describen exactamente la firma depth (acostado = cara lejos; sentado = gradiente azul cabecera→pie; borde = tronco bed/body + rodilla piso; parado = cuerpo floor).
- **Consistencia entre los 3 finalistas**: alta en acostado, sentado, sentado-borde, leaving-bed.
- **Debilidad puntual de x-320**: la cara de una persona parada/inclinada en el costado de la cabecera lee bed/head (falso "en cama" si se usa la cara sola). Los l lo leen floor.
- **Regla para el FSM**: la cara sola NO decide "persona en la cama". El voto robusto es el cuerpo (hombros/caderas/rodillas leen floor en parado en los 3 modelos).

## Próximos pasos propuestos

1. **Campaña 2 — body parts (seg + pose)**: combinar la máscara de segmentación (`seg-standard`) con los keypoints para obtener áreas/polígonos por parte (cabeza, torso, extremidades) y probar la mediana por área en vez del punto único — más robusto que el punto del keypoint y que el bbox de cara (que mostró p90 3.27 ruidoso en el crop de persona).
2. **Coeficiente de escala por modelo (sin tablero)**: estimar `k_model = distancia real / distancia modelo` usando la geometría conocida de la escena (x,y + dimensiones reales de la cama), como la calibración por zona ya hace con las medianas. Normalizaría la escala entre modelos (factores 2–3×) manteniendo la estructura relativa como señal principal.
