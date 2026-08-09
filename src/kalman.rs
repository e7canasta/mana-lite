/// Kalman 7D de SORT para tracking (ADR-013).
///
/// Estado: [cx, cy, s, r, dcx, dcy, ds] donde `s = w*h` (escala), `r = w/h`.
/// Transicion: modelo de velocidad constante parametrizado por `dt` en
/// segundos. Observacion directa de [cx, cy, s, r]. Implementado sin
/// dependencias con matrices f32 de tamano fijo.
#[allow(clippy::many_single_char_names)]
#[derive(Debug, Clone)]
pub struct Kalman7 {
    x: [f32; 7],
    p: [[f32; 7]; 7],
}

/// Paso nominal entre keyframes procesados.
pub const NOMINAL_DT_S: f32 = 0.2;

/// Mas alla de este dt la extrapolacion es ruido: la expiracion por edad
/// (track.rs) debe encargarse antes.
const MAX_DT_S: f32 = 2.0;

/// Matriz de transicion del modelo de velocidad constante para un `dt`
/// dado (en segundos). Con `dt = 1` era la matriz identidad + velocidades.
fn transition(dt: f32) -> [[f32; 7]; 7] {
    let mut f = [[0.0f32; 7]; 7];
    for (i, row) in f.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    f[0][4] = dt; // cx += dcx * dt
    f[1][5] = dt; // cy += dcy * dt
    f[2][6] = dt; // s  += ds  * dt
    f
}

// Las velocidades del estado viven en px/segundo (el modelo viejo las media
// en px/frame). P0/Q conservan los valores de siempre reescalados por
// 1/NOMINAL_DT_S^2 = 25 en las filas/cols 4..6 para que la incertidumbre
// inicial de velocidad y el ruido de proceso signifiquen lo mismo que antes.
const P0: [[f32; 7]; 7] = [
    [10.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 10.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 0.0, 250_000.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 0.0, 0.0, 250_000.0, 0.0],
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 250_000.0],
];

const Q: [[f32; 7]; 7] = [
    [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 0.0, 0.25, 0.0, 0.0],
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.25, 0.0],
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0025],
];

const R: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

impl Kalman7 {
    /// Estado desde un bbox x1y1x2y2, velocidades nulas y covarianza inicial.
    #[must_use]
    pub fn from_bbox(bbox: [f32; 4]) -> Self {
        Self {
            x: bbox_to_state(bbox),
            p: P0,
        }
    }

    #[must_use]
    pub fn bbox(&self) -> [f32; 4] {
        state_to_bbox(self.x)
    }

    /// Paso de prediccion (modelo de velocidad constante, `dt` en segundos).
    #[allow(clippy::needless_range_loop)]
    pub fn predict(&mut self, dt_s: f32) {
        let dt = dt_s.clamp(1e-3, MAX_DT_S);
        let f = transition(dt);
        // Q escalado: la incertidumbre crece con el tiempo transcurrido,
        // no por tick. Con dt = NOMINAL_DT_S queda igual que el modelo viejo.
        let k = dt / NOMINAL_DT_S;
        let mut q = Q;
        for row in &mut q {
            for v in row.iter_mut() {
                *v *= k;
            }
        }
        self.x = mat7_vec7(f, self.x);
        self.p = add7(mat7_mat7(mat7_mat7(f, self.p), transpose7(f)), q);
    }

    /// Paso de actualizacion con una medicion [cx, cy, s, r].
    #[allow(clippy::needless_range_loop)]
    pub fn update(&mut self, z: [f32; 4]) {
        let innovation: [f32; 4] = [
            z[0] - self.x[0],
            z[1] - self.x[1],
            z[2] - self.x[2],
            z[3] - self.x[3],
        ];

        // S = H P H^T + R  (4x4): H selecciona las primeras 4 filas/cols de P.
        let mut s = [[0.0f32; 4]; 4];
        for i in 0..4 {
            for j in 0..4 {
                s[i][j] = R[i][j] + self.p[i][j];
            }
        }
        let s_inv = invert4(s);

        // K = P H^T S^-1 (7x4): PH^T son las primeras 4 columnas de P.
        let mut k = [[0.0f32; 7]; 4];
        for i in 0..7 {
            for j in 0..4 {
                let mut acc = 0.0;
                for (kk, s_inv_row) in s_inv.iter().enumerate() {
                    acc = self.p[i][kk].mul_add(s_inv_row[j], acc);
                }
                k[j][i] = acc;
            }
        }

        // x += K @ innovation
        for i in 0..7 {
            let mut acc = 0.0;
            for j in 0..4 {
                acc = k[j][i].mul_add(innovation[j], acc);
            }
            self.x[i] += acc;
        }

        // P -= K @ (H P) ; H P son las primeras 4 filas de P.
        let mut hp = [[0.0f32; 7]; 4];
        hp.copy_from_slice(&self.p[..4]);
        for i in 0..7 {
            for j in 0..7 {
                let mut acc = 0.0;
                for row in 0..4 {
                    acc = k[row][i].mul_add(hp[row][j], acc);
                }
                self.p[i][j] -= acc;
            }
        }
    }
}

impl Default for Kalman7 {
    fn default() -> Self {
        Self { x: [0.0; 7], p: P0 }
    }
}

fn bbox_to_state(bbox: [f32; 4]) -> [f32; 7] {
    let [x1, y1, x2, y2] = bbox;
    let w = (x2 - x1).max(1e-3);
    let h = (y2 - y1).max(1e-3);
    [
        (x1 + x2) * 0.5,
        (y1 + y2) * 0.5,
        w * h,
        w / h,
        0.0,
        0.0,
        0.0,
    ]
}

#[allow(clippy::many_single_char_names)]
fn state_to_bbox(x: [f32; 7]) -> [f32; 4] {
    let cx = x[0];
    let cy = x[1];
    let s = x[2].max(1e-3);
    let r = x[3].max(1e-3);
    let w = (s * r).sqrt();
    let h = (s / r).sqrt();
    [cx - w * 0.5, cy - h * 0.5, cx + w * 0.5, cy + h * 0.5]
}

#[allow(clippy::many_single_char_names)]
fn mat7_mat7(a: [[f32; 7]; 7], b: [[f32; 7]; 7]) -> [[f32; 7]; 7] {
    let mut out = [[0.0f32; 7]; 7];
    for i in 0..7 {
        for j in 0..7 {
            let mut acc = 0.0;
            for k in 0..7 {
                acc = a[i][k].mul_add(b[k][j], acc);
            }
            out[i][j] = acc;
        }
    }
    out
}

fn transpose7(a: [[f32; 7]; 7]) -> [[f32; 7]; 7] {
    let mut out = [[0.0f32; 7]; 7];
    for i in 0..7 {
        for j in 0..7 {
            out[j][i] = a[i][j];
        }
    }
    out
}

fn mat7_vec7(a: [[f32; 7]; 7], v: [f32; 7]) -> [f32; 7] {
    let mut out = [0.0f32; 7];
    for i in 0..7 {
        let mut acc = 0.0;
        for j in 0..7 {
            acc = a[i][j].mul_add(v[j], acc);
        }
        out[i] = acc;
    }
    out
}

fn add7(a: [[f32; 7]; 7], b: [[f32; 7]; 7]) -> [[f32; 7]; 7] {
    let mut out = a;
    for i in 0..7 {
        for j in 0..7 {
            out[i][j] += b[i][j];
        }
    }
    out
}

/// Inversa 4x4 por Gauss-Jordan con pivoteo parcial.
fn invert4(m: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut a = m;
    let mut inv = [[0.0f32; 4]; 4];
    for (i, row) in inv.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for col in 0..4 {
        let mut pivot = col;
        for row in col + 1..4 {
            if a[row][col].abs() > a[pivot][col].abs() {
                pivot = row;
            }
        }
        a.swap(col, pivot);
        inv.swap(col, pivot);
        let divisor = a[col][col];
        for j in 0..4 {
            a[col][j] /= divisor;
            inv[col][j] /= divisor;
        }
        for row in 0..4 {
            if row == col {
                continue;
            }
            let factor = a[row][col];
            for j in 0..4 {
                a[row][j] = factor.mul_add(-a[col][j], a[row][j]);
                inv[row][j] = factor.mul_add(-inv[col][j], inv[row][j]);
            }
        }
    }
    inv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbox_round_trip() {
        let kalman = Kalman7::from_bbox([100.0, 200.0, 300.0, 500.0]);
        let bbox = kalman.bbox();
        assert!((bbox[0] - 100.0).abs() < 1e-2);
        assert!((bbox[1] - 200.0).abs() < 1e-2);
        assert!((bbox[2] - 300.0).abs() < 1e-2);
        assert!((bbox[3] - 500.0).abs() < 1e-2);
    }

    #[test]
    fn predict_keeps_static_position() {
        let mut kalman = Kalman7::from_bbox([0.0, 0.0, 100.0, 200.0]);
        kalman.predict(NOMINAL_DT_S);
        let bbox = kalman.bbox();
        assert!((bbox[0] - 0.0).abs() < 1e-2);
        assert!((bbox[2] - 100.0).abs() < 1e-2);
        assert!((bbox[3] - 200.0).abs() < 1e-2);
    }

    #[test]
    fn update_converges_to_measurement() {
        let mut kalman = Kalman7::from_bbox([0.0, 0.0, 100.0, 200.0]);
        for _ in 0..30 {
            kalman.predict(NOMINAL_DT_S);
            kalman.update([50.0, 250.0, 20_000.0, 1.0]);
        }
        // Medicion [cx=50, cy=250, s=20000, r=1.0] -> bbox
        // [-20.7, 179.3, 120.7, 320.7] (w = h = sqrt(20000)).
        let bbox = kalman.bbox();
        assert!((bbox[0] - (-20.7)).abs() < 1.0, "x1={}", bbox[0]);
        assert!((bbox[1] - 179.3).abs() < 1.0, "y1={}", bbox[1]);
        assert!((bbox[2] - 120.7).abs() < 1.0, "x2={}", bbox[2]);
        assert!((bbox[3] - 320.7).abs() < 1.0, "y2={}", bbox[3]);
    }

    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn track_motion_is_smoothed() {
        let mut kalman = Kalman7::from_bbox([0.0, 0.0, 50.0, 100.0]);
        for step in 1..=10 {
            let x = step as f32 * 10.0;
            kalman.predict(NOMINAL_DT_S);
            kalman.update([x + 25.0, 50.0, 5_000.0, 0.5]);
        }
        let bbox = kalman.bbox();
        assert!(
            bbox[0] > 60.0,
            "smoothed x1 should track motion: {}",
            bbox[0]
        );
        assert!(bbox[0] < 130.0, "should lag slightly behind: {}", bbox[0]);
    }

    #[test]
    fn invert4_identity() {
        let mut identity = [[0.0f32; 4]; 4];
        for (i, row) in identity.iter_mut().enumerate() {
            row[i] = 1.0;
        }
        let inv = invert4(identity);
        for i in 0..4 {
            for j in 0..4 {
                assert!((inv[i][j] - identity[i][j]).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn invert4_known_matrix() {
        let m = [
            [4.0, 7.0, 2.0, 1.0],
            [3.0, 5.0, 9.0, 2.0],
            [1.0, 2.0, 3.0, 4.0],
            [6.0, 8.0, 1.0, 5.0],
        ];
        let inv = invert4(m);
        for (i, m_row) in m.iter().enumerate() {
            for (j, _) in inv.iter().enumerate() {
                let mut acc = 0.0;
                for k in 0..4 {
                    acc = m_row[k].mul_add(inv[k][j], acc);
                }
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!((acc - expected).abs() < 1e-3, "m*inv[{i}][{j}] = {acc}");
            }
        }
    }
}
