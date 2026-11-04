
use crate::geom::Rect;
use crate::graph::{Dir, EdgeKind, Graph, Relation, Side};
use crate::layout::{
    Boxed, Cand, Config, Drawn, EdgeStyle, FRAME_PAD, Kind, Plan, TITLE_BAND, label_size,
};
use crate::metrics::Fonts;

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
        let seq: Vec<usize> = if down {
            (1..depth).collect()
        } else {
            (0..depth.saturating_sub(1)).rev().collect()
        };
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
                if nb.is_empty() {
                    rank[v]
                } else {
                    nb.iter().sum::<f64>() / nb.len() as f64
                }
            };
            let mut keyed: Vec<(f64, usize)> = rows[l].iter().map(|&v| (key(v), v)).collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            rows[l] = keyed.into_iter().map(|(_, v)| v).collect();
            set_rank(&rows, &mut rank);
        }
    }
    rows
}

fn merged(g: &Graph, dir: Dir) -> Vec<usize> {
    let side = match dir {
        Dir::Down => Side::Row,
        Dir::Right => Side::Column,
    };
    let mut rep: Vec<usize> = (0..g.nodes.len()).collect();
    fn find(rep: &mut [usize], v: usize) -> usize {
        if rep[v] != v {
            rep[v] = find(rep, rep[v]);
        }
        rep[v]
    }
    for c in g.constraints.iter().filter(|c| c.hard) {
        if let Relation::Align { side: s, items } = &c.rel
            && *s == side
        {
            for w in items.windows(2) {
                let (a, b) = (find(&mut rep, w[0]), find(&mut rep, w[1]));
                rep[a] = b;
            }
        }
    }
    (0..rep.len()).map(|v| find(&mut rep, v)).collect()
}

pub fn plan(g: &Graph, fonts: &Fonts, boxes: &[Boxed], cfg: &Config, b: f64, dir: Dir) -> Plan {
    let n = boxes.len();
    let page = cfg.page();
    let depth = g
        .constraints
        .iter()
        .map(|c| c.rel.depth())
        .max()
        .unwrap_or(0) as f64;
    let (lead, trail) = match dir {
        Dir::Down => (depth * (FRAME_PAD + TITLE_BAND) * b, depth * FRAME_PAD * b),
        Dir::Right => (depth * FRAME_PAD * b, depth * FRAME_PAD * b),
    };
    let (main_len, cross_len) = match dir {
        Dir::Down => (page.y1 - page.y0 - lead - trail, page.x1 - page.x0),
        Dir::Right => (page.x1 - page.x0 - lead - trail, page.y1 - page.y0),
    };
    let size = |v: usize| match dir {
        Dir::Down => (boxes[v].h, boxes[v].w),
        Dir::Right => (boxes[v].w, boxes[v].h),
    };
    let to_xy = |m: f64, c: f64| match dir {
        Dir::Down => (c, page.y1 - m),
        Dir::Right => (page.x0 + m, -c),
    };

    let rep = merged(g, dir);
    let tree: Vec<(usize, usize)> = g
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Tree)
        .map(|e| (e.a, e.b))
        .collect();
    let flat: Vec<bool> = tree.iter().map(|&(a, b)| rep[a] == rep[b]).collect();
    let contracted: Vec<(usize, usize)> = tree.iter().map(|&(a, b)| (rep[a], rep[b])).collect();
    let mut back = back_edges(n, &contracted);
    for (bk, f) in back.iter_mut().zip(&flat) {
        *bk &= !f;
    }
    let keep = |i: usize| !back[i] && !flat[i];
    let dag: Vec<(usize, usize)> = (0..tree.len())
        .filter(|&i| keep(i))
        .map(|i| tree[i])
        .collect();
    let layer: Vec<usize> = {
        let up: Vec<(usize, usize)> = (0..tree.len())
            .filter(|&i| keep(i))
            .map(|i| contracted[i])
            .collect();
        let l = layers(n, &up);
        (0..n).map(|v| l[rep[v]]).collect()
    };
    let along: Vec<(usize, usize)> = (0..tree.len())
        .filter(|&i| flat[i])
        .map(|i| tree[i])
        .collect();
    let rows = order(n, &layer, &dag);

    let sep = (0.6 * b).max(3.0);
    let thick: Vec<f64> = rows
        .iter()
        .map(|r| r.iter().map(|&v| size(v).0).fold(0.0, f64::max))
        .collect();
    let mut room = vec![2.0 * b; rows.len()];
    for e in g.edges.iter().filter(|e| e.kind == EdgeKind::Tree) {
        let (la, lb) = (layer[e.a].min(layer[e.b]), layer[e.a].max(layer[e.b]));
        if let (Some(l), true) = (&e.label, lb == la + 1) {
            let (w, h) = label_size(fonts, l, b);
            room[la] = room[la].max(if dir == Dir::Down { h } else { w } + 2.0 * b);
        }
    }
    let gaps = |r: &std::ops::Range<usize>| room[r.start..r.end - 1].iter().sum::<f64>();
    let span: Vec<f64> = rows
        .iter()
        .map(|r| r.iter().map(|&v| size(v).1).sum::<f64>() + sep * r.len().saturating_sub(1) as f64)
        .collect();
    let bands = |k: usize| -> Vec<std::ops::Range<usize>> {
        let per = rows.len().div_ceil(k.max(1)).max(1);
        (0..rows.len())
            .step_by(per)
            .map(|s| s..(s + per).min(rows.len()))
            .collect()
    };
    let fits = |k: usize, scale: f64| {
        let bs = bands(k);
        let along = bs
            .iter()
            .all(|r| scale * (thick[r.clone()].iter().sum::<f64>() + gaps(r)) <= main_len);
        let across = scale
            * (bs
                .iter()
                .map(|r| span[r.clone()].iter().copied().fold(0.0, f64::max))
                .sum::<f64>()
                + 2.0 * b * (bs.len() - 1) as f64);
        along && (bs.len() == 1 || across <= cross_len)
    };
    let wrap = !fits(1, cfg.legible / b);
    let most = g.settings.bands.unwrap_or(rows.len()).min(rows.len());
    let k = if wrap {
        (1..=most).find(|&k| fits(k, 1.0)).unwrap_or(1)
    } else {
        1
    };
    let bands = bands(k);
    let feasible = fits(k, 1.0);
    let band: Vec<usize> = (0..rows.len())
        .map(|l| bands.iter().position(|r| r.contains(&l)).unwrap())
        .collect();
    let pinned = g
        .constraints
        .iter()
        .any(|c| matches!(c.rel, Relation::At { .. }));
    let cap = if pinned { f64::INFINITY } else { 4.0 * b };
    let extra = bands
        .iter()
        .filter(|r| r.len() > 1)
        .map(|r| {
            (main_len - thick[r.clone()].iter().sum::<f64>() - gaps(r)) / (r.len() - 1) as f64
                - 1e-6
        })
        .fold(cap, f64::min)
        .max(0.0);
    let mut center = vec![0.0; rows.len()];
    for (i, r) in bands.iter().enumerate() {
        let used = thick[r.clone()].iter().sum::<f64>() + gaps(r) + extra * (r.len() - 1) as f64;
        let mut m = lead + (main_len - used).max(0.0) / 2.0;
        for l in r.clone() {
            center[l] = if i % 2 == 0 {
                m + thick[l] / 2.0
            } else {
                2.0 * lead + main_len - m - thick[l] / 2.0
            };
            m += thick[l] + room[l] + extra;
        }
    }
    let need: Vec<f64> = bands
        .iter()
        .map(|r| span[r.clone()].iter().copied().fold(0.0, f64::max))
        .collect();
    let slack =
        ((cross_len - need.iter().sum::<f64>() - 2.0 * b * (k - 1) as f64) / k as f64).max(0.0);
    let mut slices = Vec::new();
    let mut c0 = -cross_len / 2.0;
    for (i, w) in need.iter().enumerate() {
        let w = if k == 1 { cross_len } else { w + slack };
        slices.push((c0, c0 + w));
        c0 += w + if i + 1 < k { 2.0 * b } else { 0.0 };
    }

    let step = 0.5 * b;
    let mut cands: Vec<Vec<Cand>> = (0..n).map(|_| Vec::new()).collect();
    for (l, row) in rows.iter().enumerate().filter(|_| feasible) {
        let (lo, hi) = slices[band[l]];
        let total: f64 = row.iter().map(|&v| size(v).1).sum();
        let space = ((hi - lo - total) / row.len() as f64).clamp(0.0, 4.0 * b);
        let span = total + space * row.len() as f64;
        let mut c = (lo + hi) / 2.0 - span / 2.0 + space / 2.0;
        for &v in row {
            let (_, w) = size(v);
            let ideal = c + w / 2.0;
            c += w + space;
            let (i0, i1) = (
                ((lo + w / 2.0) / step).ceil() as i64,
                ((hi - w / 2.0) / step).floor() as i64,
            );
            for i in i0..=i1 {
                let x = i as f64 * step;
                cands[v].push(Cand {
                    cost: (x - ideal).abs(),
                    at: to_xy(center[l], x),
                    r: 0.0,
                });
            }
            cands[v].retain(|c| Rect::centered(c.at, boxes[v].w, boxes[v].h).within(&page));
            cands[v].sort_by(|a, b| a.cost.total_cmp(&b.cost));
        }
    }

    let mut cons = Vec::new();
    for row in &rows {
        for (i, &u) in row.iter().enumerate() {
            for &v in &row[i + 1..] {
                let joined = along
                    .iter()
                    .any(|&(p, q)| (p, q) == (u, v) || (q, p) == (u, v));
                cons.push((
                    vec![u, v],
                    Kind::Apart(if joined { 2.0 * b } else { sep }),
                    0,
                ));
            }
        }
    }
    for &(a, c) in &along {
        for u in (0..n).filter(|&u| u != a && u != c && layer[u] == layer[a]) {
            cons.push((vec![a, c, u], Kind::Clear(sep * 0.25), 0));
        }
    }
    let near = |l: usize, lo: usize, hi: usize| k > 1 || (l >= lo && l <= hi);
    for (i, &(a, c)) in dag.iter().enumerate() {
        let (lo, hi) = (layer[a].min(layer[c]), layer[a].max(layer[c]));
        for u in (0..n).filter(|&u| u != a && u != c && near(layer[u], lo, hi)) {
            cons.push((vec![a, c, u], Kind::Clear(sep * 0.25), 0));
        }
        for &(p, q) in &dag[i + 1..] {
            let overlap = k > 1 || layer[p].max(layer[a]) < layer[q].min(layer[c]);
            if overlap && a != p && a != q && c != p && c != q {
                cons.push((vec![a, c, p, q], Kind::NoCross, 1));
            }
        }
    }

    let mut edges = Vec::new();
    for (e, is_back) in g
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Tree)
        .zip(&back)
    {
        let style = if *is_back {
            EdgeStyle::Link
        } else {
            EdgeStyle::Arrow
        };
        edges.push(Drawn::new(e.a, e.b, style, 0.12, e.label.clone()));
    }
    for e in g.edges.iter().filter(|e| e.kind == EdgeKind::Link) {
        edges.push(Drawn::new(e.a, e.b, EdgeStyle::Link, 0.1, e.label.clone()));
    }
    let member = |v: usize| {
        g.constraints
            .iter()
            .any(|c| matches!(&c.rel, Relation::Group { items, .. } if items.contains(&v)))
    };
    let (mut order, rest): (Vec<usize>, Vec<usize>) =
        rows.concat().into_iter().partition(|&v| member(v));
    order.extend(rest);
    let crowded = (!feasible).then(|| {
        (
            rows.len(),
            thick.iter().sum::<f64>() + gaps(&(0..rows.len())),
            main_len,
        )
    });
    Plan {
        order,
        cands,
        cons,
        edges,
        kx: 1.0,
        ky: 1.0,
        tol: 0.5 * step,
        crowded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_is_broken_and_layers_point_forward() {
        let edges = [(0, 1), (1, 2), (2, 0), (0, 3)];
        let back = back_edges(4, &edges);
        assert_eq!(back, vec![false, false, true, false]);
        let dag: Vec<_> = edges
            .iter()
            .zip(&back)
            .filter(|(_, b)| !**b)
            .map(|(e, _)| *e)
            .collect();
        assert_eq!(layers(4, &dag), vec![0, 1, 2, 1]);
    }

    #[test]
    fn barycenter_uncrosses_two_edges() {
        let rows = order(4, &[0, 0, 1, 1], &[(0, 3), (1, 2)]);
        assert_eq!(rows[1], vec![3, 2]);
    }
}
