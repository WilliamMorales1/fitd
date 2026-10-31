//! Radial strategy: the most general idea at the center, specifics farther out.
//!
//! Each node's candidate spots lie in an angular wedge sized by its subtree's leaf count (with
//! nudges past it at a cost), stepping outward along stretched polar rays so the map fills a
//! non-square page. Structural constraints: regions stay apart, children sit farther out than
//! parents, tree edges miss every other node, and tree edges never cross.

use crate::layout::{Boxed, Cand, Config, Drawn, EdgeStyle, Kind, Plan, Role};
use crate::geom::Rect;
use crate::graph::{Graph, Place, Relation};
use crate::tree::Tree;

pub fn role(t: &Tree, v: usize) -> Role {
    match t.depth[v] {
        0 => Role::Root,
        1 if !t.children[v].is_empty() => Role::Hub,
        _ => Role::Leaf,
    }
}

/// wedges gives each node an angular interval (center, half-width) in degrees, sized by leaf count.
fn wedges(t: &Tree, children: &[Vec<usize>], rot: f64) -> Vec<(f64, f64)> {
    let mut out = vec![(90.0, 180.0); t.parent.len()];
    let mut stack = vec![(t.root, 90.0 + 180.0, 360.0)];
    while let Some((v, hi, span)) = stack.pop() {
        let total = t.leaves(v) as f64;
        let mut cur = hi;
        if v == t.root && !children[v].is_empty() {
            // Center the first branch at the top.
            cur = 90.0 + rot + span * t.leaves(children[v][0]) as f64 / total / 2.0;
        }
        for &c in &children[v] {
            let w = span * t.leaves(c) as f64 / total;
            out[c] = (cur - w / 2.0, w / 2.0);
            stack.push((c, cur, w));
            cur -= w;
        }
    }
    out
}

/// Cost that moves a spot ahead of (or behind) every spot without it, for `at` cells.
const AT_FIRST: f64 = 1e6;

/// is_under reports whether `u` lies in the subtree of `v`.
fn is_under(t: &Tree, v: usize, mut u: usize) -> bool {
    loop {
        if u == v {
            return true;
        }
        match t.parent[u] {
            Some(p) => u = p,
            None => return false,
        }
    }
}

/// in_place reports whether a center lies in a page cell, as `at` defines it.
fn in_place(c: (f64, f64), place: Place, page: &Rect) -> bool {
    let (w, h) = ((page.x1 - page.x0) / 3.0, (page.y1 - page.y0) / 3.0);
    let (x0, y1) = (page.x0 + w * place.0 as f64, page.y1 - h * place.1 as f64);
    c.0 >= x0 && c.0 <= x0 + w && c.1 <= y1 && c.1 >= y1 - h
}

/// End is where a pull on a node points: another node, or a direction in degrees.
#[derive(Clone, Copy)]
enum End {
    Node(usize),
    Angle(f64),
}

/// arrange orders branches so that cross links, `near`s, and `at`s point where their far ends
/// are: the root's branches by trying every order (up to 7 branches), the rest by barycenter
/// sweeps as in layered layouts. None when the source order already does best.
pub fn arrange(g: &Graph, t: &Tree, cfg: &Config) -> Option<Tree> {
    let n = t.parent.len();
    let (kx, ky) = ((cfg.page_w / cfg.page_h).sqrt(), (cfg.page_h / cfg.page_w).sqrt());
    let mut pulls: Vec<Vec<(End, f64)>> = vec![Vec::new(); n];
    let mut aimed = false;
    for l in &t.links {
        pulls[l.a].push((End::Node(l.b), 1.0));
        pulls[l.b].push((End::Node(l.a), 1.0));
    }
    for c in &g.constraints {
        let w = if c.hard { 10.0 } else { c.weight as f64 };
        match c.rel {
            // A missed `at` costs about as much as many crossings (see `merit`).
            Relation::At { a, place } if place != Place(1, 1) => {
                let w = 10.0 * w;
                // The cell's center seen from the page center, on the stretched polar rays.
                let (x, y) = ((place.0 as f64 - 1.0) * cfg.page_w / 3.0, (1.0 - place.1 as f64) * cfg.page_h / 3.0);
                pulls[a].push((End::Angle((y / ky).atan2(x / kx).to_degrees()), w));
                aimed = true;
            }
            Relation::Near { a, b, .. } => {
                pulls[a].push((End::Node(b), w));
                pulls[b].push((End::Node(a), w));
            }
            _ => {}
        }
    }
    let under = |c: usize, u: usize| is_under(t, c, u);
    let turn = |d: f64| (d + 540.0).rem_euclid(360.0) - 180.0;
    let aim = |wedge: &[(f64, f64)], e: End| match e {
        End::Node(w) => wedge[w].0,
        End::Angle(a) => a,
    };
    let cost = |children: &[Vec<usize>]| {
        let wedge = wedges(t, children, 0.0);
        (0..n).filter(|&v| v != t.root).flat_map(|v| pulls[v].iter().map(move |&(e, w)| (v, e, w))).map(|(v, e, w)| w * turn(aim(&wedge, e) - wedge[v].0).abs()).sum::<f64>()
    };
    let mut children = t.children.clone();
    let roots = children[t.root].clone();
    // A few rounds settle it: each one moves wedges, which moves the far ends.
    for _ in 0..3 {
        // Links alone are served by the sweeps below; turning whole branches pays off for `at`.
        if aimed && roots.len() <= 7 {
            let mut best = (cost(&children), children[t.root].clone());
            let mut perm = roots.clone();
            permutations(&mut perm, 0, &mut |p| {
                let mut c = children.clone();
                c[t.root] = p.to_vec();
                let k = cost(&c);
                if k < best.0 - 1e-9 {
                    best = (k, p.to_vec());
                }
            });
            children[t.root] = best.1;
        }
        let wedge = wedges(t, &children, 0.0);
        for v in (0..n).filter(|&v| v != t.root) {
            let key = |c: usize| {
                let (mut sum, mut total) = (0.0, 0.0);
                for u in (0..n).filter(|&u| under(c, u)) {
                    for &(e, w) in &pulls[u] {
                        if let End::Node(x) = e
                            && under(c, x)
                        {
                            continue;
                        }
                        sum += w * turn(aim(&wedge, e) - wedge[v].0);
                        total += w;
                    }
                }
                if total == 0.0 { turn(wedge[c].0 - wedge[v].0) } else { sum / total }
            };
            let mut keyed: Vec<(f64, usize)> = children[v].iter().map(|&c| (key(c), c)).collect();
            // Wedges run clockwise from the first child, so the most counterclockwise goes first.
            keyed.sort_by(|a, b| b.0.total_cmp(&a.0));
            children[v] = keyed.into_iter().map(|(_, c)| c).collect();
        }
    }
    (children != t.children).then(|| Tree { root: t.root, parent: t.parent.clone(), children, depth: t.depth.clone(), order: t.order.clone(), links: t.links.clone(), notes: Vec::new() })
}

/// permutations calls `f` with every order of `xs[k..]`, the given order first.
fn permutations(xs: &mut [usize], k: usize, f: &mut dyn FnMut(&[usize])) {
    if k == xs.len() {
        return f(xs);
    }
    for i in k..xs.len() {
        xs.swap(k, i);
        permutations(xs, k + 1, f);
        xs.swap(k, i);
    }
}

pub fn plan(g: &Graph, t: &Tree, boxes: &[Boxed], cfg: &Config, b: f64, rot: f64) -> Plan {
    let n = boxes.len();
    let page = cfg.page();
    let kx = (cfg.page_w / cfg.page_h).sqrt();
    let ky = 1.0 / kx;
    let wedge = wedges(t, &t.children, rot);
    let step = 0.3 * b;
    let lambda = 0.4 * b;
    // Each depth gets an ideal ring so levels spread over the page instead of piling at the center.
    let max_depth = t.depth.iter().copied().max().unwrap_or(1).max(1) as f64;
    let reach = 0.92 * (cfg.page_w * cfg.page_h).sqrt() / 2.0;
    let ideal = |v: usize| reach * t.depth[v] as f64 / (max_depth + 0.35);

    let at: Vec<(usize, Place)> = g.constraints.iter().filter_map(|c| if let Relation::At { a, place } = c.rel { Some((a, place)) } else { None }).collect();
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
        // Spots in a node's `at` cell go first.
        for &(_, place) in at.iter().filter(|a| a.0 == v) {
            vals.iter_mut().filter(|c| in_place(c.at, place, &page)).for_each(|c| c.cost -= AT_FIRST);
        }
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
    let mut drawn: Vec<Drawn> = edges.iter().map(|&(p, v)| Drawn::new(p, v, EdgeStyle::Line, if t.depth[v] == 1 { 0.45 } else { 0.18 }, None)).collect();
    drawn.extend(t.links.iter().map(|l| Drawn::new(l.a, l.b, EdgeStyle::Link, 0.1, l.label.clone())));
    // Group members go first, as in layered layouts: once a frame is known, forward checking
    // keeps everyone else off it.
    let member = |v: usize| g.constraints.iter().any(|c| matches!(&c.rel, Relation::Group { items, .. } if items.contains(&v)));
    let (mut order, rest): (Vec<usize>, Vec<usize>) = t.order.iter().partition(|&&v| member(v));
    order.extend(rest);
    Plan { order, cands, cons, edges: drawn, kx, ky, tol: 0.5 * step, crowded: None }
}
