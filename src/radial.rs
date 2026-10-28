//! Radial strategy: the most general idea at the center, specifics farther out.
//!
//! Each node's candidate spots lie in an angular wedge sized by its subtree's leaf count (with
//! nudges past it at a cost), stepping outward along stretched polar rays so the map fills a
//! non-square page. Structural constraints: regions stay apart, children sit farther out than
//! parents, tree edges miss every other node, and tree edges never cross.

use crate::layout::{Boxed, Cand, Config, Drawn, EdgeStyle, Kind, Plan, Role};
use crate::geom::Rect;
use crate::tree::Tree;

pub fn role(t: &Tree, v: usize) -> Role {
    match t.depth[v] {
        0 => Role::Root,
        1 if !t.children[v].is_empty() => Role::Hub,
        _ => Role::Leaf,
    }
}

/// wedges gives each node an angular interval (center, half-width) in degrees, sized by leaf count.
fn wedges(t: &Tree) -> Vec<(f64, f64)> {
    let mut out = vec![(90.0, 180.0); t.parent.len()];
    let mut stack = vec![(t.root, 90.0 + 180.0, 360.0)];
    while let Some((v, hi, span)) = stack.pop() {
        let total = t.leaves(v) as f64;
        let mut cur = hi;
        if v == t.root && !t.children[v].is_empty() {
            // Center the first branch at the top.
            cur = 90.0 + span * t.leaves(t.children[v][0]) as f64 / total / 2.0;
        }
        for &c in &t.children[v] {
            let w = span * t.leaves(c) as f64 / total;
            out[c] = (cur - w / 2.0, w / 2.0);
            stack.push((c, cur, w));
            cur -= w;
        }
    }
    out
}

pub fn plan(t: &Tree, boxes: &[Boxed], cfg: &Config, b: f64) -> Plan {
    let n = boxes.len();
    let page = cfg.page();
    let kx = (cfg.page_w / cfg.page_h).sqrt();
    let ky = 1.0 / kx;
    let wedge = wedges(t);
    let step = 0.3 * b;
    let lambda = 0.4 * b;
    // Each depth gets an ideal ring so levels spread over the page instead of piling at the center.
    let max_depth = t.depth.iter().copied().max().unwrap_or(1).max(1) as f64;
    let reach = 0.92 * (cfg.page_w * cfg.page_h).sqrt() / 2.0;
    let ideal = |v: usize| reach * t.depth[v] as f64 / (max_depth + 0.35);

    let mut cands = Vec::with_capacity(n);
    for v in 0..n {
        let fits = |c: &Cand| Rect::centered(c.at, boxes[v].w, boxes[v].h).within(&page);
        let mut vals = Vec::new();
        if t.parent[v].is_none() {
            vals.push(Cand { cost: 0.0, at: (0.0, 0.0), r: 0.0 });
        } else {
            // Nudges past the wedge are allowed (at a cost): wide boxes rarely fit thin wedges.
            let (c, hw) = wedge[v];
            let dmax = hw.max(30.0);
            let mut d = -dmax;
            while d <= dmax + 1e-9 {
                let th = (c + d).to_radians();
                let mut r = step;
                loop {
                    let cand = Cand { cost: (r - ideal(v)).abs() + lambda * d.abs(), at: (kx * r * th.cos(), ky * r * th.sin()), r };
                    if !fits(&cand) {
                        break;
                    }
                    vals.push(cand);
                    r += step;
                }
                d += 3.0;
            }
        }
        vals.retain(fits);
        vals.sort_by(|a, b| a.cost.total_cmp(&b.cost));
        cands.push(vals);
    }

    let gap = (0.6 * b).max(3.0);
    let mut cons = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            cons.push((vec![i, j], Kind::Apart(gap), 0));
        }
    }
    let edges: Vec<(usize, usize)> = t.order.iter().filter_map(|&v| t.parent[v].map(|p| (p, v))).collect();
    for (k, &(p, v)) in edges.iter().enumerate() {
        cons.push((vec![p, v], Kind::Outward(step), 0));
        for u in (0..n).filter(|&u| u != p && u != v) {
            cons.push((vec![p, v, u], Kind::Clear(gap * 0.25), 0));
        }
        for &(q, w) in &edges[k + 1..] {
            if p != q && p != w && v != q && v != w {
                cons.push((vec![p, v, q, w], Kind::NoCross, 0));
            }
        }
    }
    let mut drawn: Vec<Drawn> = edges.iter().map(|&(p, v)| Drawn { a: p, b: v, style: EdgeStyle::Line, width: if t.depth[v] == 1 { 0.45 } else { 0.18 }, label: None }).collect();
    drawn.extend(t.links.iter().map(|l| Drawn { a: l.a, b: l.b, style: EdgeStyle::Link, width: 0.1, label: l.label.clone() }));
    Plan { order: t.order.clone(), cands, cons, edges: drawn, kx, ky, tol: 0.5 * step }
}
