use crate::geom::Rect;
use crate::graph::{Dir, EdgeKind, Graph, Relation, Side};
use crate::layout::{
    Boxed, Cand, Config, Drawn, EdgeStyle, FRAME_CLEAR, FRAME_PAD, Kind, Plan, TITLE_BAND,
    label_size,
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

fn cmp_keys(a: &[(f64, usize)], b: &[(f64, usize)]) -> std::cmp::Ordering {
    for (x, y) in a.iter().zip(b) {
        let o = x.0.total_cmp(&y.0).then(x.1.cmp(&y.1));
        if o.is_ne() {
            return o;
        }
    }
    a.len().cmp(&b.len())
}

/// cluster reorders each row so a group's members sit together, and groups keep one order in every row.
///
/// A frame spans every layer its members reach, so members split by another node, or two groups that swap sides between layers, would put non-members inside a frame.
fn cluster(rows: &mut [Vec<usize>], rank: &[f64], groups: &[Vec<usize>]) {
    if groups.is_empty() {
        return;
    }
    let gkey: Vec<f64> = groups
        .iter()
        .map(|m| m.iter().map(|&v| rank[v]).sum::<f64>() / m.len().max(1) as f64)
        .collect();
    let mut outer_first: Vec<usize> = (0..groups.len()).collect();
    outer_first.sort_by_key(|&k| std::cmp::Reverse(groups[k].len()));
    for row in rows.iter_mut() {
        let mut keyed: Vec<(Vec<(f64, usize)>, usize)> = row
            .iter()
            .map(|&v| {
                let mut k: Vec<(f64, usize)> = outer_first
                    .iter()
                    .filter(|&&g| groups[g].contains(&v))
                    .map(|&g| (gkey[g], g))
                    .collect();
                k.push((rank[v], usize::MAX));
                (k, v)
            })
            .collect();
        keyed.sort_by(|a, b| cmp_keys(&a.0, &b.0));
        *row = keyed.into_iter().map(|(_, v)| v).collect();
    }
}

fn order(
    n: usize,
    layer: &[usize],
    dag: &[(usize, usize)],
    source: bool,
    groups: &[Vec<usize>],
) -> Vec<Vec<usize>> {
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
    cluster(&mut rows, &rank, groups);
    set_rank(&rows, &mut rank);
    if source {
        // One downward pass: each node follows its parents, and siblings keep edge order.
        for l in 1..depth {
            let key = |v: usize| {
                let up: Vec<(usize, f64)> = (dag.iter().enumerate())
                    .filter(|&(_, &(a, b))| b == v && layer[a] < l)
                    .map(|(i, &(a, _))| (i, rank[a]))
                    .collect();
                match up.first() {
                    Some(&(i, _)) => (up.iter().map(|p| p.1).sum::<f64>() / up.len() as f64, i),
                    None => (rank[v], usize::MAX),
                }
            };
            let mut keyed: Vec<((f64, usize), usize)> =
                rows[l].iter().map(|&v| (key(v), v)).collect();
            keyed.sort_by(|a, b| a.0.0.total_cmp(&b.0.0).then(a.0.1.cmp(&b.0.1)));
            rows[l] = keyed.into_iter().map(|(_, v)| v).collect();
            set_rank(&rows, &mut rank);
            cluster(&mut rows[l..=l], &rank, groups);
            set_rank(&rows, &mut rank);
        }
        return rows;
    }
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
            cluster(&mut rows[l..=l], &rank, groups);
            set_rank(&rows, &mut rank);
        }
    }
    rows
}

pub(crate) fn merged(g: &Graph, dir: Dir) -> Vec<usize> {
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

/// Layering is the flow structure of a layered graph: merged ranks, tree edges, and each node's layer.
pub(crate) struct Layering {
    rep: Vec<usize>,
    tree: Vec<(usize, usize)>,
    flat: Vec<bool>,
    back: Vec<bool>,
    pub layer: Vec<usize>,
}

// A `->` between two aside nodes orders the strip, not the flow.
fn strip_edge(g: &Graph, e: &crate::graph::Edge) -> bool {
    g.nodes[e.a].aside && g.nodes[e.b].aside
}

pub(crate) fn layering(g: &Graph, dir: Dir) -> Layering {
    let n = g.nodes.len();
    let rep = merged(g, dir);
    let tree: Vec<(usize, usize)> = g
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Tree && !strip_edge(g, e))
        .map(|e| (e.a, e.b))
        .collect();
    let flat: Vec<bool> = tree.iter().map(|&(a, b)| rep[a] == rep[b]).collect();
    let contracted: Vec<(usize, usize)> = tree.iter().map(|&(a, b)| (rep[a], rep[b])).collect();
    let mut back = back_edges(n, &contracted);
    for (bk, f) in back.iter_mut().zip(&flat) {
        *bk &= !f;
    }
    let mut up: Vec<(usize, usize)> = (0..tree.len())
        .filter(|&i| !back[i] && !flat[i])
        .map(|i| contracted[i])
        .collect();
    // A hard order along the flow also orders layers, so unconnected nodes can stack.
    let ordered: Vec<(usize, usize)> = g
        .constraints
        .iter()
        .filter(|c| c.hard)
        .filter_map(|c| match (dir, &c.rel) {
            (Dir::Down, Relation::Above { a, b, .. }) | (Dir::Right, Relation::LeftOf { a, b, .. }) => {
                Some((rep[*a], rep[*b]))
            }
            _ => None,
        })
        .filter(|&(a, b)| a != b)
        .collect();
    let mut with = up.clone();
    with.extend(&ordered);
    if !back_edges(n, &with).iter().any(|&b| b) {
        up = with;
    }
    let l = layers(n, &up);
    let layer = (0..n).map(|v| l[rep[v]]).collect();
    Layering {
        rep,
        tree,
        flat,
        back,
        layer,
    }
}

/// Frame is one group's share of the cross axis in `pack`: its members, the title width it needs, and how far its title pushes members in from the low edge.
struct Frame {
    items: Vec<usize>,
    min: f64,
    title: f64,
}

/// pack places `members` along the cross axis from 0 and returns the width used.
///
/// Each child frame is one block that covers every row its members reach, so no other node lands inside it; rows that share no block pack independently.
struct Pack<'a> {
    frames: &'a [Frame],
    rows: &'a [Vec<usize>],
    layer: &'a [usize],
    width: &'a dyn Fn(usize) -> f64,
    adj: &'a [Vec<usize>],
    gap: f64,
    pad: f64,
    /// Cross width the top level may spread over.
    room: f64,
}

fn pack(members: &[usize], p: &Pack, pos: &mut [f64], top: bool) -> f64 {
    let Pack {
        frames,
        rows,
        layer,
        width,
        gap,
        pad,
        ..
    } = *p;
    let inside =
        |f: &Frame| f.items.len() < members.len() && f.items.iter().all(|v| members.contains(v));
    let kids: Vec<usize> = (0..frames.len())
        .filter(|&i| inside(&frames[i]))
        .filter(|&i| {
            !(0..frames.len()).any(|j| {
                j != i
                    && inside(&frames[j])
                    && frames[j].items.len() > frames[i].items.len()
                    && frames[i].items.iter().all(|v| frames[j].items.contains(v))
            })
        })
        .collect();
    // Unit u < kids.len() is a frame; otherwise it is node `members[u - kids.len()]` on its own.
    let unit_of = |v: usize| {
        kids.iter()
            .position(|&i| frames[i].items.contains(&v))
            .unwrap_or_else(|| kids.len() + members.iter().position(|&m| m == v).unwrap())
    };
    let units = kids.len() + members.len();
    let mut span = vec![(usize::MAX, 0); units];
    let mut key = vec![(0.0, 0usize); units];
    let mut after: Vec<Vec<usize>> = vec![Vec::new(); units];
    let mut used = vec![false; units];
    for row in rows {
        let mut prev: Option<usize> = None;
        for (i, &v) in row.iter().enumerate().filter(|(_, v)| members.contains(v)) {
            let u = unit_of(v);
            used[u] = true;
            span[u] = (span[u].0.min(layer[v]), span[u].1.max(layer[v]));
            key[u] = (key[u].0 + (i as f64 + 0.5) / row.len() as f64, key[u].1 + 1);
            if let Some(p) = prev.filter(|&p| p != u) {
                after[p].push(u);
            }
            prev = Some(u);
        }
    }
    let mut indeg = vec![0usize; units];
    for &u in after.iter().flatten() {
        indeg[u] += 1;
    }
    let mut seq = Vec::new();
    let mut done = vec![false; units];
    for _ in 0..units {
        let pick = (0..units)
            .filter(|&u| used[u] && !done[u])
            .min_by(|&a, &b| {
                (indeg[a] > 0)
                    .cmp(&(indeg[b] > 0))
                    .then((key[a].0 / key[a].1 as f64).total_cmp(&(key[b].0 / key[b].1 as f64)))
            });
        let Some(u) = pick else { break };
        done[u] = true;
        seq.push(u);
        for &w in &after[u] {
            indeg[w] = indeg[w].saturating_sub(1);
        }
    }
    let depth = rows.len();
    let mut cursor = vec![0.0f64; depth];
    let mut xs = vec![0.0f64; units];
    let mut ws = vec![0.0f64; units];
    for &u in &seq {
        let (lo, hi) = span[u];
        let x = cursor[lo..=hi].iter().copied().fold(0.0, f64::max);
        let w = if u < kids.len() {
            let f = &frames[kids[u]];
            let inner = pack(&f.items, p, pos, false);
            let w = (inner + 2.0 * pad + f.title).max(f.min);
            let shift = x + (w - inner - f.title) / 2.0 + f.title;
            for &v in &f.items {
                pos[v] += shift;
            }
            w
        } else {
            let v = members[u - kids.len()];
            pos[v] = x + width(v) / 2.0;
            width(v)
        };
        (xs[u], ws[u]) = (x, w);
        for c in &mut cursor[lo..=hi] {
            *c = x + w + gap;
        }
    }
    let items = |u: usize| -> Vec<usize> {
        if u < kids.len() {
            frames[kids[u]].items.clone()
        } else {
            vec![members[u - kids.len()]]
        }
    };
    let hi0 = seq.iter().map(|&u| xs[u] + ws[u]).fold(0.0, f64::max);
    let moved = |u: usize, x: f64, xs: &mut [f64], pos: &mut [f64]| {
        for v in items(u) {
            pos[v] += x - xs[u];
        }
        xs[u] = x;
    };
    // Left packing pulls children away from parents; a few barycenter passes in both directions, ending left to right so blocks never overlap.
    for pass in 0..if top { 9 } else { 0 } {
        let want = |u: usize, xs: &[f64], pos: &[f64]| {
            let mine = items(u);
            let pulls: Vec<f64> = mine
                .iter()
                .flat_map(|&v| {
                    p.adj[v]
                        .iter()
                        .filter(|w| !mine.contains(w))
                        .map(move |&w| pos[w] - pos[v])
                })
                .collect();
            if pulls.is_empty() {
                xs[u]
            } else {
                xs[u] + pulls.iter().sum::<f64>() / pulls.len() as f64
            }
        };
        if pass % 2 == 0 {
            let mut cursor = vec![0.0; depth];
            for &u in &seq {
                let (lo, hi) = span[u];
                let x = cursor[lo..=hi]
                    .iter()
                    .copied()
                    .fold(want(u, &xs, pos), f64::max);
                moved(u, x, &mut xs, pos);
                for c in &mut cursor[lo..=hi] {
                    *c = x + ws[u] + gap;
                }
            }
        } else {
            let mut cursor = vec![p.room.max(hi0); depth];
            for &u in seq.iter().rev() {
                let (lo, hi) = span[u];
                let x = cursor[lo..=hi]
                    .iter()
                    .map(|c| c - ws[u])
                    .fold(want(u, &xs, pos), f64::min);
                moved(u, x, &mut xs, pos);
                for c in &mut cursor[lo..=hi] {
                    *c = x - gap;
                }
            }
        }
    }
    let lo = seq.iter().map(|&u| xs[u]).fold(f64::INFINITY, f64::min);
    let hi = seq
        .iter()
        .map(|&u| xs[u] + ws[u])
        .fold(f64::NEG_INFINITY, f64::max);
    if seq.is_empty() {
        return 0.0;
    }
    for &u in &seq {
        let x = xs[u] - lo;
        moved(u, x, &mut xs, pos);
    }
    hi - lo
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
    // The key takes the bottom of the page, so the cross axis is not centered on 0.
    let to_xy = |m: f64, c: f64| match dir {
        Dir::Down => (c + (page.x0 + page.x1) / 2.0, page.y1 - m),
        Dir::Right => (page.x0 + m, (page.y0 + page.y1) / 2.0 - c),
    };

    let Layering {
        rep,
        tree,
        flat,
        back,
        mut layer,
    } = layering(g, dir);
    let side = |e: &&crate::graph::Edge| strip_edge(g, e);
    let keep = |i: usize| !back[i] && !flat[i];
    let dag: Vec<(usize, usize)> = (0..tree.len())
        .filter(|&i| keep(i))
        .map(|i| tree[i])
        .collect();
    if cfg.pack_frames {
        // A source on row 0 stretches its frame up over every row above its children, walling off columns other frames need.
        let hard_after = |v: usize| {
            g.constraints.iter().filter(|c| c.hard).filter_map(move |c| match (dir, &c.rel) {
                (Dir::Down, Relation::Above { a, b, .. }) | (Dir::Right, Relation::LeftOf { a, b, .. }) if *a == v => Some(*b),
                _ => None,
            })
        };
        for v in 0..n {
            let alone = rep[v] == v && (0..n).all(|u| u == v || rep[u] != v);
            let fed = dag.iter().any(|&(_, c)| c == v)
                || g.constraints.iter().filter(|c| c.hard).any(|c| matches!((dir, &c.rel),
                    (Dir::Down, Relation::Above { b, .. }) | (Dir::Right, Relation::LeftOf { b, .. }) if *b == v));
            let below = dag.iter().filter(|&&(a, _)| a == v).map(|&(_, c)| layer[c]).chain(hard_after(v).map(|c| layer[c])).min();
            if let (true, false, Some(m)) = (alone, fed, below) {
                layer[v] = layer[v].max(m.saturating_sub(1));
            }
        }
    }
    let along: Vec<(usize, usize)> = (0..tree.len())
        .filter(|&i| flat[i])
        .map(|i| tree[i])
        .collect();
    let sep = (0.6 * b).max(3.0);
    let member = |v: usize| {
        g.constraints
            .iter()
            .any(|c| matches!(&c.rel, Relation::Group { items, .. } if items.contains(&v)))
    };
    let flows: Vec<bool> = {
        let mut f = vec![false; n];
        for &(a, c) in &tree {
            (f[rep[a]], f[rep[c]]) = (true, true);
        }
        f
    };
    let detached: Vec<bool> = (0..n)
        .map(|v| g.nodes[v].aside && !flows[rep[v]] && !member(v) && flows.iter().any(|&f| f))
        .collect();
    let strip = strip_rows(&detached, &size, main_len, sep);
    let strip_len = strip
        .iter()
        .map(|r| r.iter().map(|&v| size(v).1).fold(0.0, f64::max))
        .map(|w| w + 2.0 * b)
        .sum::<f64>();
    let cross_len = cross_len - strip_len;
    let rels: Vec<&Relation> = g
        .constraints
        .iter()
        .map(|c| &c.rel)
        .filter(|r| matches!(r, Relation::Group { .. }))
        .collect();
    let groups: Vec<Vec<usize>> = rels
        .iter()
        .map(|r| match r {
            Relation::Group { items, .. } => items.clone(),
            _ => unreachable!(),
        })
        .collect();
    let mut rows = order(n, &layer, &dag, g.settings.source_order, &groups);
    for r in &mut rows {
        r.retain(|&v| !detached[v]);
    }
    let thick: Vec<f64> = rows
        .iter()
        .map(|r| r.iter().map(|&v| size(v).0).fold(0.0, f64::max))
        .collect();
    let mut room = vec![2.0 * b; rows.len()];
    for e in g
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Tree && !side(e))
    {
        let (la, lb) = (layer[e.a].min(layer[e.b]), layer[e.a].max(layer[e.b]));
        if let (Some(l), true) = (&e.label, lb == la + 1) {
            let (w, h) = label_size(fonts, l, b);
            room[la] = room[la].max(if dir == Dir::Down { h } else { w } + 2.0 * b);
        }
    }
    // A frame's title and padding reach into the gap above and below its layers.
    for (r, items) in rels.iter().zip(&groups).filter(|_| cfg.pack_frames) {
        let ls = items.iter().filter(|&&v| !detached[v]).map(|&v| layer[v]);
        let (Some(lo), Some(hi)) = (ls.clone().min(), ls.max()) else {
            continue;
        };
        let d = r.depth() as f64;
        let top = if dir == Dir::Down {
            FRAME_PAD + TITLE_BAND
        } else {
            FRAME_PAD
        };
        if lo > 0 {
            room[lo - 1] = room[lo - 1].max((d * top + FRAME_CLEAR + 0.5) * b);
        }
        room[hi] = room[hi].max((d * FRAME_PAD + FRAME_CLEAR + 0.5) * b);
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
    let k = if let Some(f) = g.settings.fold {
        f.clamp(1, rows.len().max(1))
    } else if wrap {
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
    let spread = |l: usize| {
        let (lo, hi) = slices[band[l]];
        let total: f64 = rows[l].iter().map(|&v| size(v).1).sum();
        let space = ((hi - lo - total) / rows[l].len() as f64).clamp(0.0, 4.0 * b);
        (total, space)
    };
    // One band with a strip: flow and strip close up and center together, not apart.
    let close = if k == 1 && !strip.is_empty() {
        let widest = (0..rows.len())
            .map(|l| {
                let (total, space) = spread(l);
                total + space * rows[l].len() as f64
            })
            .fold(0.0, f64::max);
        (cross_len - widest).max(0.0) / 2.0
    } else {
        0.0
    };
    let mut even = vec![0.0; n];
    for (l, row) in rows.iter().enumerate() {
        let (lo, hi) = slices[band[l]];
        let (total, space) = spread(l);
        let mut c = (lo + hi) / 2.0 - (total + space * row.len() as f64) / 2.0 + space / 2.0;
        for &v in row {
            even[v] = c + size(v).1 / 2.0 + close;
            c += size(v).1 + space;
        }
    }
    let packed: Option<Vec<f64>> = (cfg.pack_frames && !groups.is_empty()).then(|| {
        let mut pos = vec![0.0; n];
        let mut adj = vec![Vec::new(); n];
        for &(a, c) in &dag {
            adj[a].push(c);
            adj[c].push(a);
        }
        for (r, &(lo, hi)) in bands.iter().zip(&slices) {
            let members: Vec<usize> = rows[r.clone()].concat();
            let frames: Vec<Frame> = rels
                .iter()
                .filter_map(|rel| {
                    let Relation::Group {
                        items,
                        title,
                        title_em,
                        ..
                    } = rel
                    else {
                        return None;
                    };
                    let items: Vec<usize> = items
                        .iter()
                        .copied()
                        .filter(|v| members.contains(v))
                        .collect();
                    let titled = title.is_some();
                    (!items.is_empty()).then(|| Frame {
                        items,
                        min: if titled && dir == Dir::Down {
                            (title_em + 2.0 * FRAME_PAD) * b
                        } else {
                            0.0
                        },
                        title: if titled && dir == Dir::Right {
                            TITLE_BAND * b
                        } else {
                            0.0
                        },
                    })
                })
                .collect();
            let width = |v: usize| size(v).1;
            let p = Pack {
                frames: &frames,
                rows: &rows,
                layer: &layer,
                width: &width,
                adj: &adj,
                gap: 2.0 * b,
                pad: FRAME_PAD * b,
                room: hi - lo,
            };
            let total = pack(&members, &p, &mut pos, true);
            // Stretching centers about the middle widens every gap, so blocks stay apart while fans spread like the unpacked rows.
            let (c0, c1) = members.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, z), &v| (a.min(pos[v]), z.max(pos[v])));
            let span = c1 - c0;
            let stretch = if span > 0.0 { ((0.8 * (hi - lo) - (total - span)) / span).max(1.0) } else { 1.0 };
            for &v in &members {
                pos[v] = (lo + hi) / 2.0 + (pos[v] - total / 2.0) * stretch;
            }
        }
        pos
    });
    let mut cands: Vec<Vec<Cand>> = (0..n).map(|_| Vec::new()).collect();
    for (l, row) in rows.iter().enumerate().filter(|_| feasible) {
        let (lo, hi) = slices[band[l]];
        for &v in row {
            let (_, w) = size(v);
            let ideal = packed.as_ref().map_or(even[v], |p| p[v] + close);
            let (i0, i1) = (
                ((lo + w / 2.0) / step).ceil() as i64,
                ((hi - w / 2.0) / step).floor() as i64,
            );
            for i in i0..=i1 {
                let x = i as f64 * step;
                cands[v].push(Cand {
                    cost: (x - ideal).abs(),
                    at: to_xy(center[l], x - strip_len / 2.0),
                    r: 0.0,
                });
            }
            cands[v].retain(|c| Rect::centered(c.at, boxes[v].w, boxes[v].h).within(&page));
            cands[v].sort_by(|a, b| a.cost.total_cmp(&b.cost));
        }
    }

    let mut c = (cross_len - strip_len) / 2.0 + 2.0 * b - close;
    for r in &strip {
        let thick = r.iter().map(|&v| size(v).1).fold(0.0, f64::max);
        let total = r.iter().map(|&v| size(v).0).sum::<f64>();
        let space = ((main_len - total) / r.len() as f64).clamp(0.0, 4.0 * b);
        let mut m = lead + (main_len - total - space * (r.len() - 1) as f64) / 2.0;
        for &v in r.iter().filter(|_| feasible) {
            let (len, w) = size(v);
            // Sit under the flow nodes it links to; otherwise keep the strip order.
            let linked: Vec<f64> = g
                .edges
                .iter()
                .filter_map(|e| match (e.a == v, e.b == v) {
                    (true, false) if !detached[e.b] => Some(center[layer[e.b]]),
                    (false, true) if !detached[e.a] => Some(center[layer[e.a]]),
                    _ => None,
                })
                .collect();
            let even = m + len / 2.0;
            let ideal = if linked.is_empty() {
                even
            } else {
                linked.iter().sum::<f64>() / linked.len() as f64
            };
            m += len + space;
            let (i0, i1) = (
                ((lead + len / 2.0) / step).ceil() as i64,
                ((lead + main_len - len / 2.0) / step).floor() as i64,
            );
            let at = c + w / 2.0;
            for i in i0..=i1 {
                let x = i as f64 * step;
                cands[v].push(Cand {
                    cost: (x - ideal).abs() + 0.25 * (x - even).abs(),
                    at: to_xy(x, at),
                    r: 0.0,
                });
            }
            cands[v].retain(|c| Rect::centered(c.at, boxes[v].w, boxes[v].h).within(&page));
            cands[v].sort_by(|a, b| a.cost.total_cmp(&b.cost));
        }
        c += thick + 2.0 * b;
    }

    let mut cons = Vec::new();
    for r in &strip {
        for (i, &u) in r.iter().enumerate() {
            for &v in &r[i + 1..] {
                cons.push((vec![u, v], Kind::Apart(sep), 0));
            }
        }
    }
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
    if g.settings.source_order {
        let ahead = match dir {
            Dir::Down => (1.0, 0.0),
            Dir::Right => (0.0, -1.0),
        };
        for row in &rows {
            for w in row.windows(2) {
                cons.push((vec![w[0], w[1]], Kind::Before(ahead.0, ahead.1), 0));
            }
        }
    }
    for &(a, c) in &along {
        for u in (0..n).filter(|&u| u != a && u != c && layer[u] == layer[a]) {
            cons.push((vec![a, c, u], Kind::Clear(sep * 0.25), 0));
        }
    }
    let near = |l: usize, lo: usize, hi: usize| k > 1 || (l >= lo && l <= hi);
    // An edge that ends on a group is drawn from the frame, so the stretch past its stand-in members needs no clearance.
    let hidden = |a: usize, c: usize, u: usize| {
        g.edges.iter().filter(|e| (e.a, e.b) == (a, c)).any(|e| {
            e.groups.iter().flatten().any(|&ci| {
                matches!(&g.constraints[ci].rel, Relation::Group { items, .. } if items.contains(&u))
            })
        })
    };
    for (i, &(a, c)) in dag.iter().enumerate() {
        let (lo, hi) = (layer[a].min(layer[c]), layer[a].max(layer[c]));
        for u in (0..n).filter(|&u| u != a && u != c && near(layer[u], lo, hi) && !hidden(a, c, u)) {
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
        .filter(|e| e.kind == EdgeKind::Tree && !side(e))
        .zip(&back)
    {
        let style = if *is_back {
            EdgeStyle::Link
        } else {
            EdgeStyle::Arrow
        };
        edges.push(Drawn::new(e.a, e.b, style, 0.12, e.label.clone()).on(g, e));
    }
    for e in g
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Tree && side(e))
    {
        edges.push(Drawn::new(e.a, e.b, EdgeStyle::Arrow, 0.1, e.label.clone()).on(g, e));
    }
    for e in g.edges.iter().filter(|e| e.kind == EdgeKind::Link) {
        edges.push(Drawn::new(e.a, e.b, EdgeStyle::Link, 0.1, e.label.clone()).on(g, e));
    }
    let (mut order, rest): (Vec<usize>, Vec<usize>) =
        rows.concat().into_iter().partition(|&v| member(v));
    order.extend(rest);
    order.extend(strip.concat());
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

/// strip_rows packs detached nodes, in source order, into rows no longer than `main_len`.
fn strip_rows(
    detached: &[bool],
    size: impl Fn(usize) -> (f64, f64),
    main_len: f64,
    sep: f64,
) -> Vec<Vec<usize>> {
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut used = 0.0;
    for v in (0..detached.len()).filter(|&v| detached[v]) {
        let len = size(v).0;
        match rows.last_mut() {
            Some(r) if used + sep + len <= main_len => {
                r.push(v);
                used += sep + len;
            }
            _ => {
                rows.push(vec![v]);
                used = len;
            }
        }
    }
    rows
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
        let rows = order(4, &[0, 0, 1, 1], &[(0, 3), (1, 2)], false, &[]);
        assert_eq!(rows[1], vec![3, 2]);
    }
}
