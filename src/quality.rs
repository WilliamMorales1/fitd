//! Readability score of a finished layout, from the graph-drawing literature.
//!
//! Each term is a penalty (lower is better). Lengths are in a fixed unit, the legibility
//! floor's em, not the body font's, so layouts at different fonts compare fairly. Terms, most
//! important first:
//!
//! - Crossings (Purchase 1997: the aesthetic with the largest effect on understanding): tree
//!   edges crossing each other (only possible in layered layouts), cross links over edges,
//!   and links tunneling through unrelated boxes (Dunne and Shneiderman), at the link's best
//!   bend.
//! - Proximity (Gestalt; stress-based layout): related nodes sit close. Tree edges and cross
//!   links are charged their gap, squared, which also evens out edge lengths (Huang and Eades).
//! - Grouping: a node should be nearer its own branch than any other branch, or the eye
//!   groups it with the wrong one.
//! - Angular resolution (Dunne and Shneiderman): edges at a node do not fan out too narrowly.
//! - Alignment (Purchase, Pilcher, and Plimmer 2012): in a flowchart, edges run straight
//!   along the flow, so nodes line up on a grid.
//! - Spread: ink is evenly distributed over the page, without large holes or dense knots.

use crate::geom::{segment_hits_rect, segments_cross, Rect, P};

pub struct Input<'a> {
    pub rects: &'a [Rect],
    pub tree: &'a [(usize, usize)],
    pub links: &'a [(usize, usize)],
    /// Branch of each node (the root's child it descends from), if the layout has branches.
    pub branch: &'a [Option<usize>],
    /// Flow axis of a layered layout (true: down), whose edges should run straight along it.
    pub flow: Option<bool>,
    pub page: Rect,
    /// Length unit in points, the same for every layout being compared.
    pub unit: f64,
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Score {
    pub crossings: f64,
    pub proximity: f64,
    pub grouping: f64,
    pub angles: f64,
    pub alignment: f64,
    pub spread: f64,
}

/// Weights per term: a crossing costs as much as a few extra ems of every edge.
const W_CROSS: f64 = 40.0;
const W_PROX: f64 = 1.0;
const W_LINK: f64 = 0.3;
const W_GROUP: f64 = 2.0;
const W_ANGLE: f64 = 20.0;
const W_ALIGN: f64 = 2.0;
const W_SPREAD: f64 = 4000.0;

/// Cells across the page's long side for the spread term.
const CELLS: usize = 8;

impl Score {
    pub fn total(&self) -> f64 {
        self.crossings + self.proximity + self.grouping + self.angles + self.alignment + self.spread
    }
}

/// gap is the empty distance between two boxes, 0 when they touch or overlap.
pub fn gap(a: &Rect, b: &Rect) -> f64 {
    let dx = (a.x0 - b.x1).max(b.x0 - a.x1).max(0.0);
    let dy = (a.y0 - b.y1).max(b.y0 - a.y1).max(0.0);
    dx.hypot(dy)
}

fn center(r: &Rect) -> P {
    ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0)
}

/// Bends tried for a link, a subset of the router's.
const BENDS: [f64; 5] = [0.0, 0.22, -0.22, 0.4, -0.4];

/// link_hits counts what a link bent by `bow` crosses: edges, plus unrelated boxes it tunnels
/// through, which hide where it goes. The curve is the router's (see `layout::curve`), coarsely.
fn link_hits(x: &Input, c: &[P], a: usize, b: usize, bow: f64) -> usize {
    let (pa, pb) = (c[a], c[b]);
    let m = ((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0);
    let d = (pb.0 - pa.0).hypot(pb.1 - pa.1).max(1e-9);
    let mut nrm = (-(pb.1 - pa.1) / d, (pb.0 - pa.0) / d);
    if nrm.0 * m.0 + nrm.1 * m.1 < 0.0 {
        nrm = (-nrm.0, -nrm.1);
    }
    let k = (m.0 + bow * d * nrm.0, m.1 + bow * d * nrm.1);
    let pts: Vec<P> = (0..=4)
        .map(|i| {
            let t = i as f64 / 4.0;
            let (u, w) = ((1.0 - t) * (1.0 - t), 2.0 * (1.0 - t) * t);
            (u * pa.0 + w * k.0 + t * t * pb.0, u * pa.1 + w * k.1 + t * t * pb.1)
        })
        .collect();
    let mut hits = 0;
    for &(p, q) in x.tree.iter().chain(x.links) {
        if (p, q) != (a, b) && pts.windows(2).any(|s| segments_cross(s[0], s[1], c[p], c[q])) {
            hits += 1;
        }
    }
    for u in (0..x.rects.len()).filter(|&u| u != a && u != b) {
        if pts.windows(2).any(|s| segment_hits_rect(s[0], s[1], &x.rects[u])) {
            hits += 1;
        }
    }
    hits
}

pub fn score(x: &Input) -> Score {
    let em = |d: f64| d / x.unit;
    let c: Vec<P> = x.rects.iter().map(center).collect();
    let mut s = Score::default();

    for (k, &(a, b)) in x.tree.iter().enumerate() {
        let crosses = x.tree[k + 1..].iter().filter(|&&(p, q)| segments_cross(c[a], c[b], c[p], c[q])).count();
        s.crossings += W_CROSS * crosses as f64;
        s.proximity += W_PROX * em(gap(&x.rects[a], &x.rects[b])).powi(2) / 10.0;
    }
    for &(a, b) in x.links {
        s.proximity += W_LINK * em(gap(&x.rects[a], &x.rects[b])).powi(2) / 10.0;
        // The router later picks a bend; charge the link its best bend.
        let best = BENDS.iter().map(|&bow| link_hits(x, &c, a, b, bow)).min().unwrap_or(0);
        s.crossings += W_CROSS * best as f64;
    }

    let n = x.rects.len();
    for &(p, v) in x.tree {
        let Some(bv) = x.branch[v] else { continue };
        let own = gap(&x.rects[v], &x.rects[p]);
        let other = (0..n).filter(|&u| x.branch[u].is_some_and(|bu| bu != bv)).map(|u| gap(&x.rects[v], &x.rects[u])).fold(f64::MAX, f64::min);
        s.grouping += W_GROUP * em((own - other).max(0.0));
    }

    let mut dirs: Vec<Vec<f64>> = vec![Vec::new(); n];
    for &(a, b) in x.tree {
        dirs[a].push((c[b].1 - c[a].1).atan2(c[b].0 - c[a].0));
        dirs[b].push((c[a].1 - c[b].1).atan2(c[a].0 - c[b].0));
    }
    for d in &mut dirs {
        if d.len() < 2 {
            continue;
        }
        d.sort_by(f64::total_cmp);
        let target = (std::f64::consts::TAU / d.len() as f64).min(std::f64::consts::FRAC_PI_4);
        let mut prev = d[d.len() - 1] - std::f64::consts::TAU;
        for &a in d.iter() {
            s.angles += W_ANGLE * (target - (a - prev)).max(0.0) / target;
            prev = a;
        }
    }

    if let Some(down) = x.flow {
        for &(a, b) in x.tree {
            let off = if down { c[a].0 - c[b].0 } else { c[a].1 - c[b].1 };
            s.alignment += W_ALIGN * em(off.abs());
        }
        // A flowchart's shape follows its flow, so even coverage is not a goal there.
        return s;
    }

    // Coverage per cell; the penalty is the variance across cells.
    let (pw, ph) = (x.page.x1 - x.page.x0, x.page.y1 - x.page.y0);
    let cell = pw.max(ph) / CELLS as f64;
    let (nx, ny) = ((pw / cell).ceil() as usize, (ph / cell).ceil() as usize);
    let mut cover = vec![0.0; nx * ny];
    for r in x.rects {
        let (i0, i1) = (((r.x0 - x.page.x0) / cell).floor().max(0.0) as usize, (((r.x1 - x.page.x0) / cell).ceil() as usize).min(nx));
        let (j0, j1) = (((r.y0 - x.page.y0) / cell).floor().max(0.0) as usize, (((r.y1 - x.page.y0) / cell).ceil() as usize).min(ny));
        for i in i0..i1 {
            for j in j0..j1 {
                let cx = x.page.x0 + i as f64 * cell;
                let cy = x.page.y0 + j as f64 * cell;
                let w = (r.x1.min(cx + cell) - r.x0.max(cx)).max(0.0);
                let h = (r.y1.min(cy + cell) - r.y0.max(cy)).max(0.0);
                cover[j * nx + i] += w * h / (cell * cell);
            }
        }
    }
    let mean = cover.iter().sum::<f64>() / cover.len() as f64;
    s.spread = W_SPREAD * cover.iter().map(|k| (k - mean).powi(2)).sum::<f64>() / cover.len() as f64;
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(rects: &'a [Rect], tree: &'a [(usize, usize)], branch: &'a [Option<usize>]) -> Input<'a> {
        Input { rects, tree, links: &[], branch, flow: None, page: Rect::centered((0.0, 0.0), 100.0, 100.0), unit: 1.0 }
    }

    #[test]
    fn nearer_child_scores_better() {
        let near = [Rect::centered((0.0, 0.0), 4.0, 4.0), Rect::centered((6.0, 0.0), 4.0, 4.0)];
        let far = [near[0], Rect::centered((30.0, 0.0), 4.0, 4.0)];
        let b = [None, Some(1)];
        assert!(score(&input(&near, &[(0, 1)], &b)).proximity < score(&input(&far, &[(0, 1)], &b)).proximity);
    }

    #[test]
    fn child_closer_to_other_branch_is_penalized() {
        // Hubs 1 and 2; leaf 3 hangs off 1 but sits beside 2.
        let r = [
            Rect::centered((0.0, 0.0), 4.0, 4.0),
            Rect::centered((-20.0, 0.0), 4.0, 4.0),
            Rect::centered((20.0, 0.0), 4.0, 4.0),
            Rect::centered((20.0, 8.0), 4.0, 4.0),
        ];
        let tree = [(0, 1), (0, 2), (1, 3)];
        let b = [None, Some(1), Some(2), Some(1)];
        assert!(score(&input(&r, &tree, &b)).grouping > 0.0);
    }
}
