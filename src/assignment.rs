/// Asignacion lineal optima (algoritmo hungaro / Kuhn-Munkres) para el
/// matching de tracks (ADR-013). Complejidad O(n^3); n clinico es pequeno.
///
/// Costo `cost[i][j]` entre track `i` y deteccion `j`; `INF` marca pares
/// prohibidos (clase distinta). Devuelve la asignacion `(track_idx, det_idx)`
/// con costo total minimo, sin pares prohibidos y sin exceder `max_cost`.
#[allow(
    clippy::many_single_char_names,
    clippy::needless_range_loop,
    clippy::items_after_statements
)]
#[must_use]
pub fn hungarian_min(
    cost: &[Vec<f32>],
    max_cost: f32,
) -> (Vec<(usize, usize)>, Vec<usize>, Vec<usize>) {
    let n_tracks = cost.len();
    if n_tracks == 0 {
        return (Vec::new(), Vec::new(), Vec::new());
    }
    let n_dets = cost[0].len();

    // Matriz cuadrada (1-based, e-maxx/Kuhn-Munkres): n = max(n_tracks,
    // n_dets). Pares prohibidos -> sentinela finito grande; el resultado se
    // filtra por `max_cost` y por prohibicion al final.
    const FORBIDDEN: f32 = 1.0e6;
    let n = n_tracks.max(n_dets);
    let mut a = vec![vec![FORBIDDEN; n + 1]; n + 1];
    for i in 0..n_tracks {
        for j in 0..n_dets {
            a[i + 1][j + 1] = if cost[i][j].is_finite() {
                cost[i][j]
            } else {
                FORBIDDEN
            };
        }
    }
    // Filas/cols ficticias (1-based): costo 0 para que el matching exista.
    for i in 1..=n {
        for j in 1..=n {
            if i > n_tracks || j > n_dets {
                a[i][j] = 0.0;
            }
        }
    }

    // p[j] = fila asignada a la columna j; columna 0 = virtual.
    let mut u = vec![0.0f32; n + 1];
    let mut v = vec![0.0f32; n + 1];
    let mut p = vec![0usize; n + 1];
    let mut way = vec![0usize; n + 1];

    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0usize;
        let mut minv = vec![f32::INFINITY; n + 1];
        let mut used = vec![false; n + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = f32::INFINITY;
            let mut j1 = 0usize;
            for j in 1..=n {
                if used[j] {
                    continue;
                }
                let cur = a[i0][j] - u[i0] - v[j];
                if cur < minv[j] {
                    minv[j] = cur;
                    way[j] = j0;
                }
                if minv[j] < delta {
                    delta = minv[j];
                    j1 = j;
                }
            }
            for j in 0..=n {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }

    let mut matched: Vec<(usize, usize)> = Vec::new();
    let mut track_matched = vec![false; n_tracks];
    let mut det_matched = vec![false; n_dets];
    for j in 1..=n_dets {
        let i = p[j];
        if i == 0 || i > n_tracks {
            continue;
        }
        let (track, det) = (i - 1, j - 1);
        if !cost[track][det].is_finite() || cost[track][det] > max_cost {
            continue;
        }
        matched.push((track, det));
        track_matched[track] = true;
        det_matched[det] = true;
    }

    let unmatched_tracks: Vec<usize> = (0..n_tracks)
        .filter(|&i| !track_matched[i])
        .collect();
    let unmatched_dets: Vec<usize> = (0..n_dets).filter(|&j| !det_matched[j]).collect();

    (matched, unmatched_tracks, unmatched_dets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tracks_leaves_all_detections_unmatched() {
        let (matched, _, unmatched_dets) = hungarian_min(&[], 0.7);
        assert!(matched.is_empty());
        assert!(unmatched_dets.is_empty());
    }

    #[test]
    fn assigns_smallest_total_cost() {
        let cost = vec![vec![1.0, 10.0], vec![10.0, 1.0]];
        let (matched, _, _) = hungarian_min(&cost, 2.0);
        assert_eq!(matched, vec![(0, 0), (1, 1)]);
    }

    #[test]
    fn beats_greedy_when_high_confidence_choice_blocks() {
        // det0 (alta prioridad de greedy) superpone track0 0.5 y track1 0.4;
        // det1 solo superpone track1 (0.1). Greedy elige (det0, track0),
        // dejando det1 sin track. El hungaro elige (det0, track0, 0.5) y
        // (det1, track1, 0.1): ambos emparejan dentro del umbral.
        let cost = vec![vec![0.5, 1.0], vec![0.4, 0.1]];
        let (matched, _, _) = hungarian_min(&cost, 0.7);
        assert!(matched.contains(&(0, 0)));
        assert!(matched.contains(&(1, 1)));
    }

    #[test]
    fn forbidden_pairs_are_skipped() {
        // track0 solo puede con det1; track1 solo con det0.
        let cost = vec![vec![f32::INFINITY, 0.2], vec![0.3, f32::INFINITY]];
        let (matched, _, _) = hungarian_min(&cost, 0.7);
        assert!(matched.contains(&(0, 1)));
        assert!(matched.contains(&(1, 0)));
        assert_eq!(matched.len(), 2);
    }

    #[test]
    fn cost_above_threshold_drops_pair() {
        let cost = vec![vec![0.9, 0.1]];
        let (matched, unmatched_tracks, _) = hungarian_min(&cost, 0.7);
        assert_eq!(matched, vec![(0, 1)]);
        assert!(unmatched_tracks.is_empty());
    }

    #[test]
    fn rectangular_matrix_smaller_tracks() {
        let cost = vec![vec![0.1, 0.9], vec![0.8, 0.2]];
        let (matched, _, unmatched_dets) = hungarian_min(&cost, 0.7);
        assert_eq!(matched.len(), 2);
        assert!(unmatched_dets.is_empty());
    }
}
