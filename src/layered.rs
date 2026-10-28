//! Layered strategy (Sugiyama-style flowchart): edges point down (or right).
//!
//! Classic pipeline, with the last step replaced by the CSP:
//! 1. break cycles by dropping DFS back edges from the layering (they are drawn as links);
//! 2. assign layers by longest path, so every edge points forward;
//! 3. order each layer by barycenter sweeps, which only sets value preference;
//! 4. let the solver pick each node's cross-axis position from a grid. Same-layer regions stay
//!    apart and edges miss other nodes (hard); edge crossings cost 1 each (soft).

use crate::geom::Rect;
use crate::graph::{Dir, EdgeKind, Graph};
use crate::layout::{Boxed, Cand, Config, Drawn, EdgeStyle, Kind, Plan};

/// back_edges returns the indices of tree edges that close a directed cycle in DFS order.
fn back_edges(n: usize, edges: &[(usize, usize)]) -> Vec<bool> {
    let mut out_arcs = vec![Vec::new(); n];
    for (i, &(a, b)) in edges.iter().enumerate() {
        out_arcs[a].push((b, i));
    }
    let mut state = vec![0u8; n];
    let mut back = vec![false; edges.len()];
    for s in 0..n {
        if state[s] != 0 {
            continue;
        }
        let mut stack = vec![(s, 0)];
        state[s] = 1;
        while let Some(&mut (v, ref mut k)) = stack.last_mut() {
            if let Some(&(w, i)) = out_arcs[v].get(*k) {
                *k += 1;
                match state[w] {
                    0 => {
                        state[w] = 1;
                        stack.push((w, 0));
                    }
                    1 => back[i] = true,
                    _ => {}
                }
            } else {
                state[v] = 2;
                stack.pop();
            }
        }
    }
    back
}

/// layers assigns each node its longest-path distance from a source.
fn layers(n: usize, dag: &[(usize, usize)]) -> Vec<usize> {
    let mut indeg = vec![0; n];
    let mut succ = vec![Vec::new(); n];
    for &(a, b) in dag {
        indeg[b] += 1;
        succ[a].push(b);
    }
    let mut layer = vec![0; n];
    let mut ready: Vec<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    while let Some(v) = ready.pop() {
        for &w in &succ[v] {
            layer[w] = layer[w].max(layer[v] + 1);
            indeg[w] -= 1;
            if indeg[w] == 0 {
                ready.push(w);
            }
        }
    }
    layer
}

/// order sorts each layer by the mean position of its neighbors, sweeping down then up.
fn order(n: usize, layer: &[usize], dag: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let depth = layer.iter().max().map_or(0, |m| m + 1);
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); depth];
    for v in 0..n {
        rows[layer[v]].push(v);
    }
    let mut rank = vec![0.0; n];
    let set_rank = |rows: &[Vec<usize>], rank: &mut Vec<f64>| {
        for row in rows {
            for (i, &v) in row.iter().enumerate() {
                rank[v] = (i as f64 + 0.5) / row.len() as f64;
            }
        }
    };
    set_rank(&rows, &mut rank);
    for sweep in 0..8 {
        let down = sweep % 2 == 0;
        let seq: Vec<usize> = if down { (1..depth).collect() } else { (0..depth.saturating_sub(1)).rev().collect() };
        for l in seq {
            let key = |v: usize| {
                let nb: Vec<f64> = dag
                    .iter()
                    .filter_map(|&(a, b)| match down {
                        true if b == v && layer[a] < l => Some(rank[a]),
                        false if a == v && layer[b] > l => Some(rank[b]),
                        _ => None,
                    })
                    .collect();
                if nb.is_empty() { rank[v] } else { nb.iter().sum::<f64>() / nb.len() as f64 }
            };
            let mut keyed: Vec<(f64, usize)> = rows[l].iter().map(|&v| (key(v), v)).collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            rows[l] = keyed.into_iter().map(|(_, v)| v).collect();
            set_rank(&rows, &mut rank);
        }
    }
    rows
}

pub fn plan(g: &Graph, boxes: &[Boxed], cfg: &Config, b: f64, dir: Dir) -> Plan {
    let n = boxes.len();
    let page = cfg.page();
    let (main_len, cross_len) = match dir {
        Dir::Down => (page.y1 - page.y0, page.x1 - page.x0),
        Dir::Right => (page.x1 - page.x0, page.y1 - page.y0),
    };
    // (main, cross) extent of each box: main runs along the flow.
    let size = |v: usize| match dir {
        Dir::Down => (boxes[v].h, boxes[v].w),
        Dir::Right => (boxes[v].w, boxes[v].h),
    };
    let to_xy = |m: f64, c: f64| match dir {
        Dir::Down => (c, page.y1 - m),
        Dir::Right => (page.x0 + m, -c),
    };

    let tree: Vec<(usize, usize)> = g.edges.iter().filter(|e| e.kind == EdgeKind::Tree).map(|e| (e.a, e.b)).collect();
    let back = back_edges(n, &tree);
    let dag: Vec<(usize, usize)> = tree.iter().zip(&back).filter(|(_, b)| !**b).map(|(e, _)| *e).collect();
    let layer = layers(n, &dag);
    let rows = order(n, &layer, &dag);

    // Spread layers over the page, with at least two ems between them for arrows.
    let thick: Vec<f64> = rows.iter().map(|r| r.iter().map(|&v| size(v).0).fold(0.0, f64::max)).collect();
    let used: f64 = thick.iter().sum();
    let gaps = rows.len().saturating_sub(1) as f64;
    let gap = if gaps > 0.0 { ((main_len - used) / gaps).min(6.0 * b) } else { 0.0 };
    let feasible = gaps == 0.0 || gap >= 2.0 * b;
    let mut m = (main_len - used - gap * gaps).max(0.0) / 2.0;
    let mut center = Vec::new();
    for t in &thick {
        center.push(m + t / 2.0);
        m += t + gap;
    }

    let step = 0.5 * b;
    let mut cands: Vec<Vec<Cand>> = (0..n).map(|_| Vec::new()).collect();
    for row in rows.iter().filter(|_| feasible) {
        // Ideal spots spread the row evenly across the page, in barycenter order.
        let total: f64 = row.iter().map(|&v| size(v).1).sum();
        let space = ((cross_len - total) / row.len() as f64).clamp(0.0, 4.0 * b);
        let span = total + space * row.len() as f64;
        let mut c = -span / 2.0 + space / 2.0;
        for &v in row {
            let (_, w) = size(v);
            let ideal = c + w / 2.0;
            c += w + space;
            // One grid shared by every node, so centers can line up exactly.
            let k = ((cross_len - w) / 2.0 / step).floor() as i64;
            for i in -k..=k {
                let x = i as f64 * step;
                cands[v].push(Cand { cost: (x - ideal).abs(), at: to_xy(center[layer[v]], x), r: 0.0 });
            }
            cands[v].retain(|c| Rect::centered(c.at, boxes[v].w, boxes[v].h).within(&page));
            cands[v].sort_by(|a, b| a.cost.total_cmp(&b.cost));
        }
    }

    let sep = (0.6 * b).max(3.0);
    let mut cons = Vec::new();
    for row in &rows {
        for (i, &u) in row.iter().enumerate() {
            for &v in &row[i + 1..] {
                cons.push((vec![u, v], Kind::Apart(sep), 0));
            }
        }
    }
    for (k, &(a, c)) in dag.iter().enumerate() {
        let (lo, hi) = (layer[a].min(layer[c]), layer[a].max(layer[c]));
        for u in (0..n).filter(|&u| u != a && u != c && layer[u] >= lo && layer[u] <= hi) {
            cons.push((vec![a, c, u], Kind::Clear(sep * 0.25), 0));
        }
        for &(p, q) in &dag[k + 1..] {
            let overlap = layer[p].max(layer[a]) < layer[q].min(layer[c]);
            if overlap && a != p && a != q && c != p && c != q {
                cons.push((vec![a, c, p, q], Kind::NoCross, 1));
            }
        }
    }

    let mut edges = Vec::new();
    for (e, is_back) in g.edges.iter().filter(|e| e.kind == EdgeKind::Tree).zip(&back) {
        let style = if *is_back { EdgeStyle::Link } else { EdgeStyle::Arrow };
        edges.push(Drawn::new(e.a, e.b, style, 0.12, e.label.clone()));
    }
    for e in g.edges.iter().filter(|e| e.kind == EdgeKind::Link) {
        edges.push(Drawn::new(e.a, e.b, EdgeStyle::Link, 0.1, e.label.clone()));
    }
    let order: Vec<usize> = rows.concat();
    Plan { order, cands, cons, edges, kx: 1.0, ky: 1.0, tol: 0.5 * step }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_is_broken_and_layers_point_forward() {
        let edges = [(0, 1), (1, 2), (2, 0), (0, 3)];
        let back = back_edges(4, &edges);
        assert_eq!(back, vec![false, false, true, false]);
        let dag: Vec<_> = edges.iter().zip(&back).filter(|(_, b)| !**b).map(|(e, _)| *e).collect();
        assert_eq!(layers(4, &dag), vec![0, 1, 2, 1]);
    }

    #[test]
    fn barycenter_uncrosses_two_edges() {
        // 0->3 and 1->2: putting 3 before 2 removes the crossing.
        let rows = order(4, &[0, 0, 1, 1], &[(0, 3), (1, 2)]);
        assert_eq!(rows[1], vec![3, 2]);
    }
}
