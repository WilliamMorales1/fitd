use crate::geom::Rect;
use crate::graph::{Dir, EdgeKind, Graph, Relation};
use crate::layered::{layering, merged};
use crate::layout::{
    Boxed, Cand, Config, Drawn, EdgeStyle, FRAME_PAD, Kind, Plan, TITLE_BAND, label_size,
};
use crate::metrics::Fonts;

/// Lane is one column (or row, for `dir=right`) of a lanes layout.
struct Lane {
    /// Set for a group lane: its nesting depth, title width in ems, and whether it is titled.
    frame: Option<(usize, f64, bool)>,
}

/// assign gives each node its lane: top-level groups in declaration order, then loose nodes and `lane=` heads in node order.
fn assign(g: &Graph) -> (Vec<usize>, Vec<Lane>) {
    let n = g.nodes.len();
    let groups: Vec<&Relation> = g
        .constraints
        .iter()
        .map(|c| &c.rel)
        .filter(|r| matches!(r, Relation::Group { .. }))
        .collect();
    let items = |r: &Relation| match r {
        Relation::Group { items, .. } => items.clone(),
        _ => unreachable!(),
    };
    let mut of = vec![usize::MAX; n];
    let mut lanes = Vec::new();
    for (i, &r) in groups.iter().enumerate() {
        let mine = items(r);
        let inside = groups.iter().enumerate().any(|(j, &o)| {
            let theirs = items(o);
            j != i && theirs.len() > mine.len() && mine.iter().all(|v| theirs.contains(v))
        });
        if inside {
            continue;
        }
        let Relation::Group {
            title, title_em, ..
        } = r
        else {
            unreachable!()
        };
        for &v in &mine {
            of[v] = lanes.len();
        }
        lanes.push(Lane {
            frame: Some((r.depth(), *title_em, title.is_some())),
        });
    }
    for v in 0..n {
        if of[v] == usize::MAX && g.nodes[v].lane.is_none() {
            of[v] = lanes.len();
            lanes.push(Lane { frame: None });
        }
    }
    for v in 0..n {
        if of[v] == usize::MAX {
            of[v] = match g.nodes[v].lane.map(|h| of[h]) {
                Some(k) if k != usize::MAX => k,
                _ => {
                    lanes.push(Lane { frame: None });
                    lanes.len() - 1
                }
            };
        }
    }
    (of, lanes)
}

/// layer is each node's row along the flow; `order=source` gives one row per aligned set, in declaration order, below the `lane=` heads.
pub(crate) fn layer(g: &Graph, dir: Dir) -> Vec<usize> {
    if !g.settings.source_order {
        return layering(g, dir).layer;
    }
    let n = g.nodes.len();
    let rep = merged(g, dir);
    let head: Vec<bool> = (0..n)
        .map(|v| g.nodes.iter().any(|w| w.lane == Some(v)))
        .collect();
    let top = usize::from(head.iter().any(|&h| h));
    let mut reps: Vec<usize> = Vec::new();
    for v in (0..n).filter(|&v| !head[v]) {
        if !reps.contains(&rep[v]) {
            reps.push(rep[v]);
        }
    }
    (0..n)
        .map(|v| match head[v] {
            true => 0,
            false => top + reps.iter().position(|&r| r == rep[v]).unwrap(),
        })
        .collect()
}

pub fn plan(g: &Graph, fonts: &Fonts, boxes: &[Boxed], cfg: &Config, b: f64, dir: Dir) -> Plan {
    let n = boxes.len();
    let page = cfg.page();
    let (of, lanes) = assign(g);
    let layer = layer(g, dir);
    let depth = lanes
        .iter()
        .filter_map(|l| l.frame.map(|f| f.0))
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
    let along = |wh: (f64, f64)| if dir == Dir::Down { wh.1 } else { wh.0 };
    let across = |wh: (f64, f64)| if dir == Dir::Down { wh.0 } else { wh.1 };
    let to_xy = |m: f64, c: f64| match dir {
        Dir::Down => (c + (page.x0 + page.x1) / 2.0, page.y1 - m),
        Dir::Right => (page.x0 + m, (page.y0 + page.y1) / 2.0 - c),
    };
    let sep = (0.6 * b).max(3.0);
    let step = 0.5 * b;

    let nrows = layer.iter().max().map_or(0, |m| m + 1);
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); nrows];
    for v in 0..n {
        rows[layer[v]].push(v);
    }
    for r in &mut rows {
        r.sort_by_key(|&v| (of[v], v));
    }
    let flows: Vec<&crate::graph::Edge> = g
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Tree)
        .collect();

    // Along the flow: rows as thick as their nodes and level messages' labels.
    let mut thick: Vec<f64> = rows
        .iter()
        .map(|r| r.iter().map(|&v| size(v).0).fold(0.0, f64::max))
        .collect();
    let mut room = vec![2.0 * b; nrows];
    for e in &flows {
        let Some(l) = &e.label else { continue };
        let wh = label_size(fonts, l, b);
        let (la, lb) = (layer[e.a].min(layer[e.b]), layer[e.a].max(layer[e.b]));
        if la == lb {
            thick[la] = thick[la].max(along(wh) + b);
        } else if lb == la + 1 {
            room[la] = room[la].max(along(wh) + 2.0 * b);
        }
    }
    let gaps = room[..nrows.saturating_sub(1)].iter().sum::<f64>();
    let used = thick.iter().sum::<f64>() + gaps;
    let fits_along = used <= main_len;
    let extra = if nrows > 1 {
        ((main_len - used) / (nrows - 1) as f64 - 1e-6).clamp(0.0, 4.0 * b)
    } else {
        0.0
    };
    let mut center = vec![0.0; nrows];
    let mut m = lead + (main_len - used - extra * nrows.saturating_sub(1) as f64).max(0.0) / 2.0;
    for l in 0..nrows {
        center[l] = m + thick[l] / 2.0;
        m += thick[l] + room[l] + extra;
    }

    // Across the flow: each lane as wide as its widest row, framed lanes padded.
    let nl = lanes.len();
    let of = &of;
    let cell = |k: usize, l: usize| rows[l].iter().copied().filter(move |&v| of[v] == k);
    let content = |k: usize, l: usize| {
        let ws: Vec<f64> = cell(k, l).map(|v| size(v).1).collect();
        ws.iter().sum::<f64>() + sep * ws.len().saturating_sub(1) as f64
    };
    // (pad before, pad after, inner width) per lane, on the cross axis.
    let shape: Vec<(f64, f64, f64)> = (0..nl)
        .map(|k| {
            let inner = (0..nrows).map(|l| content(k, l)).fold(0.0, f64::max);
            match lanes[k].frame {
                None => (0.0, 0.0, inner),
                Some((d, title_em, titled)) => {
                    let pad = d as f64 * FRAME_PAD * b;
                    match dir {
                        Dir::Down => (pad, pad, inner.max(title_em * b)),
                        Dir::Right => {
                            let band = if titled {
                                d as f64 * TITLE_BAND * b
                            } else {
                                0.0
                            };
                            (pad + band, pad, inner)
                        }
                    }
                }
            }
        })
        .collect();
    let width: Vec<f64> = shape.iter().map(|s| s.0 + s.1 + s.2).collect();
    let mut gap = vec![2.0 * b; nl.saturating_sub(1)];
    let mid = |gap: &[f64]| {
        let mut c = 0.0;
        (0..nl)
            .map(|k| {
                let at = c + shape[k].0 + shape[k].2 / 2.0;
                c += width[k] + gap.get(k).copied().unwrap_or(0.0);
                at
            })
            .collect::<Vec<f64>>()
    };
    // A level message needs room for its label between the two lanes it joins.
    let mut level: Vec<(usize, usize, f64)> = flows
        .iter()
        .filter(|e| layer[e.a] == layer[e.b] && of[e.a] != of[e.b])
        .map(|e| {
            let (ka, kb) = (of[e.a].min(of[e.b]), of[e.a].max(of[e.b]));
            let text = e
                .label
                .as_ref()
                .map_or(0.0, |l| across(label_size(fonts, l, b)));
            (ka, kb, text + 3.0 * b)
        })
        .collect();
    level.sort_by_key(|&(ka, kb, _)| kb - ka);
    for _ in 0..3 {
        for &(ka, kb, need) in &level {
            let c = mid(&gap);
            let short = need - (c[kb] - c[ka]);
            if short > 0.0 {
                for gk in &mut gap[ka..kb] {
                    *gk += short / (kb - ka) as f64;
                }
            }
        }
    }
    let total = width.iter().sum::<f64>() + gap.iter().sum::<f64>();
    let fits_across = total <= cross_len;
    if nl > 1 && fits_across {
        let more = ((cross_len - total) / (nl - 1) as f64).min(4.0 * b);
        for gk in &mut gap {
            *gk += more;
        }
    }
    let total = width.iter().sum::<f64>() + gap.iter().sum::<f64>();
    let mut start = vec![0.0; nl];
    let mut c = -total / 2.0;
    for k in 0..nl {
        start[k] = c;
        c += width[k] + gap.get(k).copied().unwrap_or(0.0);
    }
    let feasible = fits_along && fits_across;

    let mut cands: Vec<Vec<Cand>> = (0..n).map(|_| Vec::new()).collect();
    for l in (0..nrows).filter(|_| feasible) {
        for k in 0..nl {
            let (lo, hi) = (start[k] + shape[k].0, start[k] + shape[k].0 + shape[k].2);
            let mut x = (lo + hi) / 2.0 - content(k, l) / 2.0;
            // A node alone in its cell stays on the lane's center line.
            let alone = cell(k, l).count() == 1;
            for v in cell(k, l) {
                let (_, w) = size(v);
                let ideal = x + w / 2.0;
                x += w + sep;
                let (i0, i1) = (
                    ((lo + w / 2.0) / step).ceil() as i64,
                    ((hi - w / 2.0) / step).floor() as i64,
                );
                let mut at: Vec<f64> = (i0..=i1).map(|i| i as f64 * step).collect();
                if at.is_empty() || alone {
                    at.clear();
                    at.push(ideal);
                }
                for x in at {
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
    }

    let mut cons = Vec::new();
    for row in &rows {
        for (i, &u) in row.iter().enumerate() {
            for &v in &row[i + 1..] {
                cons.push((vec![u, v], Kind::Apart(sep), 0));
            }
        }
    }
    for (i, e) in flows.iter().enumerate() {
        let (lo, hi) = (layer[e.a].min(layer[e.b]), layer[e.a].max(layer[e.b]));
        for u in (0..n).filter(|&u| u != e.a && u != e.b && (lo..=hi).contains(&layer[u])) {
            cons.push((vec![e.a, e.b, u], Kind::Clear(sep * 0.25), 0));
        }
        for f in &flows[i + 1..] {
            let overlap = layer[f.a].min(layer[f.b]) <= hi && layer[f.a].max(layer[f.b]) >= lo;
            let ends = [e.a, e.b];
            if overlap && !ends.contains(&f.a) && !ends.contains(&f.b) {
                cons.push((vec![e.a, e.b, f.a, f.b], Kind::NoCross, 1));
            }
        }
    }

    let mut edges = Vec::new();
    for e in &flows {
        let style = if layer[e.a] > layer[e.b] {
            EdgeStyle::Link
        } else {
            EdgeStyle::Arrow
        };
        edges.push(Drawn::new(e.a, e.b, style, 0.12, e.label.clone()));
    }
    for e in g.edges.iter().filter(|e| e.kind == EdgeKind::Link) {
        edges.push(Drawn::new(e.a, e.b, EdgeStyle::Link, 0.1, e.label.clone()));
    }
    let crowded = (!fits_along).then_some((nrows, used, main_len));
    Plan {
        order: rows.concat(),
        cands,
        cons,
        edges,
        kx: 1.0,
        ky: 1.0,
        tol: 0.5 * step,
        crowded,
    }
}
