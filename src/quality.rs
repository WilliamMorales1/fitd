
use crate::geom::{P, Rect, segment_hits_rect, segments_cross};

pub struct Input<'a> {
    pub rects: &'a [Rect],
    pub tree: &'a [(usize, usize)],
    pub links: &'a [(usize, usize)],
    pub branch: &'a [Option<usize>],
    pub flow: Option<bool>,
    pub page: Rect,
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

const W_CROSS: f64 = 40.0;
const W_PROX: f64 = 1.0;
const W_LINK: f64 = 0.3;
const W_GROUP: f64 = 2.0;
const W_ANGLE: f64 = 20.0;
const W_ALIGN: f64 = 300.0;
const W_SPREAD: f64 = 4000.0;

const CELLS: usize = 8;

impl Score {
    pub fn total(&self) -> f64 {
        self.crossings + self.proximity + self.grouping + self.angles + self.alignment + self.spread
    }
}

pub fn gap(a: &Rect, b: &Rect) -> f64 {
    let dx = (a.x0 - b.x1).max(b.x0 - a.x1).max(0.0);
    let dy = (a.y0 - b.y1).max(b.y0 - a.y1).max(0.0);
    dx.hypot(dy)
}

fn center(r: &Rect) -> P {
    ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0)
}

const BENDS: [f64; 5] = [0.0, 0.22, -0.22, 0.4, -0.4];

fn curve(c: &[P], a: usize, b: usize, bow: f64) -> [P; 5] {
    let (pa, pb) = (c[a], c[b]);
    let m = ((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0);
    let d = (pb.0 - pa.0).hypot(pb.1 - pa.1).max(1e-9);
    let mut nrm = (-(pb.1 - pa.1) / d, (pb.0 - pa.0) / d);
    if nrm.0 * m.0 + nrm.1 * m.1 < 0.0 {
        nrm = (-nrm.0, -nrm.1);
    }
    let k = (m.0 + bow * d * nrm.0, m.1 + bow * d * nrm.1);
    std::array::from_fn(|i| {
        let t = i as f64 / 4.0;
        let (u, w) = ((1.0 - t) * (1.0 - t), 2.0 * (1.0 - t) * t);
        (
            u * pa.0 + w * k.0 + t * t * pb.0,
            u * pa.1 + w * k.1 + t * t * pb.1,
        )
    })
}

fn bbox(pts: &[P]) -> Rect {
    pts.iter().fold(
        Rect {
            x0: f64::INFINITY,
            y0: f64::INFINITY,
            x1: f64::NEG_INFINITY,
            y1: f64::NEG_INFINITY,
        },
        |r, &(px, py)| Rect {
            x0: r.x0.min(px),
            y0: r.y0.min(py),
            x1: r.x1.max(px),
            y1: r.y1.max(py),
        },
    )
}

fn rails(x: &Input, c: &[P], a: usize, b: usize, down: bool) -> [[P; 4]; 2] {
    let split = |p: P| if down { (p.1, p.0) } else { (p.0, p.1) };
    let join = |m: f64, k: f64| if down { (k, m) } else { (m, k) };
    let (ma, mb) = (split(c[a]).0, split(c[b]).0);
    let (lo, hi) = (ma.min(mb), ma.max(mb));
    let (mut k0, mut k1) = (
        split(c[a]).1.min(split(c[b]).1),
        split(c[a]).1.max(split(c[b]).1),
    );
    for r in x
        .rects
        .iter()
        .enumerate()
        .filter(|&(u, _)| u != a && u != b)
        .map(|(_, r)| r)
    {
        let ((m0, n0), (m1, n1)) = (split((r.x0, r.y0)), split((r.x1, r.y1)));
        if m1 > lo && m0 < hi {
            (k0, k1) = (k0.min(n0), k1.max(n1));
        }
    }
    let side = |k: f64| [c[a], join(ma, k), join(mb, k), c[b]];
    [side(k0 - x.unit), side(k1 + x.unit)]
}

fn link_hits(x: &Input, c: &[P], a: usize, b: usize, bow: f64) -> usize {
    path_hits(x, c, a, b, &curve(c, a, b, bow))
}

fn path_hits(x: &Input, c: &[P], a: usize, b: usize, pts: &[P]) -> usize {
    let bb = bbox(pts);
    let mut hits = 0;
    for &(p, q) in x.tree.iter().chain(x.links) {
        let (cp, cq) = (c[p], c[q]);
        let seg = Rect {
            x0: cp.0.min(cq.0),
            y0: cp.1.min(cq.1),
            x1: cp.0.max(cq.0),
            y1: cp.1.max(cq.1),
        };
        if (p, q) != (a, b)
            && seg.intersects(&bb)
            && pts.windows(2).any(|s| segments_cross(s[0], s[1], cp, cq))
        {
            hits += 1;
        }
    }
    for u in (0..x.rects.len()).filter(|&u| u != a && u != b) {
        if x.rects[u].intersects(&bb)
            && pts
                .windows(2)
                .any(|s| segment_hits_rect(s[0], s[1], &x.rects[u]))
        {
            hits += 1;
        }
    }
    hits
}

#[cfg(test)]
fn score(x: &Input) -> Score {
    Incremental::new(x).score()
}

#[derive(Default)]
struct Terms {
    rects: Vec<Rect>,
    c: Vec<P>,
    cross: Vec<bool>,
    ncross: usize,
    link_hits: Vec<usize>,
    link_bb: Vec<Rect>,
    prox: Vec<f64>,
    link_prox: Vec<f64>,
    own: Vec<f64>,
    other: Vec<(f64, usize)>,
    angles: Vec<f64>,
    align: Vec<f64>,
    cover: Vec<f64>,
}

impl Terms {
    fn copy_from(&mut self, o: &Terms) {
        self.rects.clone_from(&o.rects);
        self.c.clone_from(&o.c);
        self.cross.clone_from(&o.cross);
        self.ncross = o.ncross;
        self.link_hits.clone_from(&o.link_hits);
        self.link_bb.clone_from(&o.link_bb);
        self.prox.clone_from(&o.prox);
        self.link_prox.clone_from(&o.link_prox);
        self.own.clone_from(&o.own);
        self.other.clone_from(&o.other);
        self.angles.clone_from(&o.angles);
        self.align.clone_from(&o.align);
        self.cover.clone_from(&o.cover);
    }
}

pub struct Incremental<'a> {
    tree: &'a [(usize, usize)],
    links: &'a [(usize, usize)],
    branch: &'a [Option<usize>],
    flow: Option<bool>,
    page: Rect,
    unit: f64,
    edges_at: Vec<Vec<usize>>,
    links_at: Vec<Vec<usize>>,
    nbrs: Vec<Vec<usize>>,
    cur: Terms,
    trial: Terms,
}

impl<'a> Incremental<'a> {
    pub fn new(x: &Input<'a>) -> Incremental<'a> {
        let n = x.rects.len();
        let (mut edges_at, mut links_at, mut nbrs) = (
            vec![Vec::new(); n],
            vec![Vec::new(); n],
            vec![Vec::new(); n],
        );
        for (k, &(a, b)) in x.tree.iter().enumerate() {
            edges_at[a].push(k);
            edges_at[b].push(k);
            nbrs[a].push(b);
            nbrs[b].push(a);
        }
        for (j, &(a, b)) in x.links.iter().enumerate() {
            links_at[a].push(j);
            links_at[b].push(j);
        }
        let mut inc = Incremental {
            tree: x.tree,
            links: x.links,
            branch: x.branch,
            flow: x.flow,
            page: x.page,
            unit: x.unit,
            edges_at,
            links_at,
            nbrs,
            cur: Terms::default(),
            trial: Terms::default(),
        };
        inc.init(x.rects);
        inc
    }

    fn em(&self, d: f64) -> f64 {
        d / self.unit
    }

    fn init(&mut self, rects: &[Rect]) {
        let (n, e) = (rects.len(), self.tree.len());
        let mut t = Terms {
            rects: rects.to_vec(),
            c: rects.iter().map(center).collect(),
            cross: vec![false; e * e],
            ..Terms::default()
        };
        for k in 0..e {
            for l in k + 1..e {
                t.cross[k * e + l] = self.crosses(&t.c, k, l);
                t.ncross += t.cross[k * e + l] as usize;
            }
        }
        t.prox = (0..e).map(|k| self.prox(&t.rects, k)).collect();
        t.link_prox = (0..self.links.len())
            .map(|j| self.link_prox(&t.rects, j))
            .collect();
        t.own = (0..e).map(|k| self.own(&t.rects, k)).collect();
        t.other = (0..e).map(|k| self.other(&t.rects, k)).collect();
        t.angles = (0..n).map(|v| self.angle(&t.c, v)).collect();
        t.align = (0..e).map(|k| self.align(&t.c, k)).collect();
        t.link_hits = vec![0; self.links.len()];
        t.link_bb = vec![
            Rect {
                x0: 0.0,
                y0: 0.0,
                x1: 0.0,
                y1: 0.0
            };
            self.links.len()
        ];
        for j in 0..self.links.len() {
            self.refresh_link(&mut t, j);
        }
        if self.flow.is_none() {
            let (nx, ny, _) = self.cells();
            t.cover = vec![0.0; nx * ny];
            for r in rects {
                self.add_cover(&mut t.cover, r, 1.0);
            }
        }
        self.cur = t;
    }

    fn crosses(&self, c: &[P], k: usize, l: usize) -> bool {
        let ((a, b), (p, q)) = (self.tree[k], self.tree[l]);
        segments_cross(c[a], c[b], c[p], c[q])
    }

    fn prox(&self, r: &[Rect], k: usize) -> f64 {
        let (a, b) = self.tree[k];
        W_PROX * self.em(gap(&r[a], &r[b])).powi(2) / 10.0
    }

    fn link_prox(&self, r: &[Rect], j: usize) -> f64 {
        let (a, b) = self.links[j];
        W_LINK * self.em(gap(&r[a], &r[b])).powi(2) / 10.0
    }

    fn own(&self, r: &[Rect], k: usize) -> f64 {
        let (p, v) = self.tree[k];
        gap(&r[v], &r[p])
    }

    fn other(&self, r: &[Rect], k: usize) -> (f64, usize) {
        let v = self.tree[k].1;
        let Some(bv) = self.branch[v] else {
            return (f64::MAX, usize::MAX);
        };
        let mut best = (f64::MAX, usize::MAX);
        for u in (0..r.len()).filter(|&u| self.branch[u].is_some_and(|bu| bu != bv)) {
            let g = gap(&r[v], &r[u]);
            if g < best.0 {
                best = (g, u);
            }
        }
        best
    }

    fn angle(&self, c: &[P], v: usize) -> f64 {
        if self.nbrs[v].len() < 2 {
            return 0.0;
        }
        let mut d: Vec<f64> = self.nbrs[v]
            .iter()
            .map(|&u| (c[u].1 - c[v].1).atan2(c[u].0 - c[v].0))
            .collect();
        d.sort_by(f64::total_cmp);
        let target = (std::f64::consts::TAU / d.len() as f64).min(std::f64::consts::FRAC_PI_4);
        let mut prev = d[d.len() - 1] - std::f64::consts::TAU;
        let mut s = 0.0;
        for &a in &d {
            s += W_ANGLE * (target - (a - prev)).max(0.0) / target;
            prev = a;
        }
        s
    }

    fn align(&self, c: &[P], k: usize) -> f64 {
        let Some(down) = self.flow else { return 0.0 };
        let (a, b) = self.tree[k];
        let off = if down {
            c[a].0 - c[b].0
        } else {
            c[a].1 - c[b].1
        };
        W_ALIGN * self.em(off.abs())
    }

    fn refresh_link(&self, t: &mut Terms, j: usize) {
        let (a, b) = self.links[j];
        let x = Input {
            rects: &t.rects,
            tree: self.tree,
            links: self.links,
            branch: self.branch,
            flow: self.flow,
            page: self.page,
            unit: self.unit,
        };
        t.link_hits[j] = BENDS
            .iter()
            .map(|&bow| link_hits(&x, &t.c, a, b, bow))
            .min()
            .unwrap_or(0);
        let mut all: Vec<P> = BENDS
            .iter()
            .flat_map(|&bow| curve(&t.c, a, b, bow))
            .collect();
        if let Some(down) = self.flow {
            for rail in rails(&x, &t.c, a, b, down) {
                t.link_hits[j] = t.link_hits[j].min(path_hits(&x, &t.c, a, b, &rail));
                all.extend(rail);
            }
        }
        t.link_bb[j] = bbox(&all);
    }

    fn cells(&self) -> (usize, usize, f64) {
        let (pw, ph) = (self.page.x1 - self.page.x0, self.page.y1 - self.page.y0);
        let cell = pw.max(ph) / CELLS as f64;
        (
            (pw / cell).ceil() as usize,
            (ph / cell).ceil() as usize,
            cell,
        )
    }

    fn add_cover(&self, cover: &mut [f64], r: &Rect, sign: f64) {
        let (nx, ny, cell) = self.cells();
        let pg = &self.page;
        let (i0, i1) = (
            ((r.x0 - pg.x0) / cell).floor().max(0.0) as usize,
            (((r.x1 - pg.x0) / cell).ceil() as usize).min(nx),
        );
        let (j0, j1) = (
            ((r.y0 - pg.y0) / cell).floor().max(0.0) as usize,
            (((r.y1 - pg.y0) / cell).ceil() as usize).min(ny),
        );
        for i in i0..i1 {
            for j in j0..j1 {
                let cx = pg.x0 + i as f64 * cell;
                let cy = pg.y0 + j as f64 * cell;
                let w = (r.x1.min(cx + cell) - r.x0.max(cx)).max(0.0);
                let h = (r.y1.min(cy + cell) - r.y0.max(cy)).max(0.0);
                cover[j * nx + i] += sign * w * h / (cell * cell);
            }
        }
    }

    pub fn score(&self) -> Score {
        let t = &self.cur;
        let mut s = Score::default();
        let hits: usize = t.ncross + t.link_hits.iter().sum::<usize>();
        s.crossings = W_CROSS * hits as f64;
        s.proximity = t.prox.iter().sum::<f64>() + t.link_prox.iter().sum::<f64>();
        s.grouping = t
            .own
            .iter()
            .zip(&t.other)
            .map(|(&own, &(other, _))| W_GROUP * self.em((own - other).max(0.0)))
            .sum();
        s.angles = t.angles.iter().sum();
        if self.flow.is_some() {
            s.alignment = t.align.iter().sum();
            return s;
        }
        let mean = t.cover.iter().sum::<f64>() / t.cover.len() as f64;
        s.spread = W_SPREAD * t.cover.iter().map(|k| (k - mean).powi(2)).sum::<f64>()
            / t.cover.len() as f64;
        s
    }

    pub fn try_move(&mut self, u: usize, r: Rect) -> Score {
        let mut t = std::mem::take(&mut self.trial);
        t.copy_from(&self.cur);
        let old = t.rects[u];
        t.rects[u] = r;
        t.c[u] = center(&r);
        let e = self.tree.len();
        for &k in &self.edges_at[u] {
            for l in (0..e).filter(|&l| l != k) {
                let (k, l) = (k.min(l), k.max(l));
                let now = self.crosses(&t.c, k, l);
                if t.cross[k * e + l] != now {
                    t.cross[k * e + l] = now;
                    if now { t.ncross += 1 } else { t.ncross -= 1 }
                }
            }
            t.prox[k] = self.prox(&t.rects, k);
            t.own[k] = self.own(&t.rects, k);
            t.align[k] = self.align(&t.c, k);
        }
        for &j in &self.links_at[u] {
            t.link_prox[j] = self.link_prox(&t.rects, j);
        }
        for k in 0..e {
            let v = self.tree[k].1;
            if v == u {
                t.other[k] = self.other(&t.rects, k);
                continue;
            }
            let (Some(bv), Some(bu)) = (self.branch[v], self.branch[u]) else {
                continue;
            };
            if bu == bv {
                continue;
            }
            let g = gap(&t.rects[v], &r);
            if g < t.other[k].0 || (g == t.other[k].0 && t.other[k].1 == u) {
                t.other[k] = (g, u);
            } else if t.other[k].1 == u {
                t.other[k] = self.other(&t.rects, k);
            }
        }
        t.angles[u] = self.angle(&t.c, u);
        for &v in &self.nbrs[u] {
            t.angles[v] = self.angle(&t.c, v);
        }
        let (was, now) = (center(&old), t.c[u]);
        let seg = |p: P, q: P| Rect {
            x0: p.0.min(q.0),
            y0: p.1.min(q.1),
            x1: p.0.max(q.0),
            y1: p.1.max(q.1),
        };
        for j in 0..self.links.len() {
            let (a, b) = self.links[j];
            let bb = t.link_bb[j];
            let near = a == u
                || b == u
                || old.intersects(&bb)
                || r.intersects(&bb)
                || self.nbrs[u]
                    .iter()
                    .chain(self.links_at[u].iter().map(|&i| {
                        if self.links[i].0 == u {
                            &self.links[i].1
                        } else {
                            &self.links[i].0
                        }
                    }))
                    .any(|&v| seg(was, t.c[v]).intersects(&bb) || seg(now, t.c[v]).intersects(&bb));
            if near {
                self.refresh_link(&mut t, j);
            }
        }
        if self.flow.is_none() {
            self.add_cover(&mut t.cover, &old, -1.0);
            self.add_cover(&mut t.cover, &r, 1.0);
        }
        self.trial = std::mem::replace(&mut self.cur, t);
        let s = self.score();
        std::mem::swap(&mut self.cur, &mut self.trial);
        s
    }

    pub fn debug_hits(&self) -> Vec<usize> {
        self.cur.link_hits.clone()
    }

    pub fn accept(&mut self) {
        std::mem::swap(&mut self.cur, &mut self.trial);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incremental_matches_full_score() {
        let mut rects: Vec<Rect> = (0..8)
            .map(|i| {
                Rect::centered(
                    (
                        (i * 7 % 5) as f64 * 9.0 - 20.0,
                        (i * 3 % 4) as f64 * 11.0 - 15.0,
                    ),
                    6.0,
                    4.0,
                )
            })
            .collect();
        let tree = [(0, 1), (0, 2), (1, 3), (1, 4), (2, 5), (2, 6), (0, 7)];
        let links = [(3, 6), (4, 5)];
        let branch = [
            None,
            Some(1),
            Some(2),
            Some(1),
            Some(1),
            Some(2),
            Some(2),
            Some(7),
        ];
        for flow in [None, Some(true)] {
            let x = Input {
                rects: &rects.clone(),
                tree: &tree,
                links: &links,
                branch: &branch,
                flow,
                page: Rect::centered((0.0, 0.0), 100.0, 80.0),
                unit: 1.0,
            };
            let mut inc = Incremental::new(&x);
            let mut rng: u64 = 12345;
            for step in 0..300 {
                rng = rng
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let u = (rng >> 33) as usize % rects.len();
                let at = (
                    ((rng >> 13) % 80) as f64 - 40.0,
                    ((rng >> 23) % 60) as f64 - 30.0,
                );
                let r = Rect::centered(at, 6.0, 4.0);
                let got = inc.try_move(u, r);
                if step % 3 != 0 {
                    inc.accept();
                    rects[u] = r;
                    let want = score(&Input { rects: &rects, ..x });
                    assert!(
                        (got.total() - want.total()).abs() < 1e-6,
                        "{got:?} vs {want:?}"
                    );
                }
            }
        }
    }

    fn input<'a>(
        rects: &'a [Rect],
        tree: &'a [(usize, usize)],
        branch: &'a [Option<usize>],
    ) -> Input<'a> {
        Input {
            rects,
            tree,
            links: &[],
            branch,
            flow: None,
            page: Rect::centered((0.0, 0.0), 100.0, 100.0),
            unit: 1.0,
        }
    }

    #[test]
    fn nearer_child_scores_better() {
        let near = [
            Rect::centered((0.0, 0.0), 4.0, 4.0),
            Rect::centered((6.0, 0.0), 4.0, 4.0),
        ];
        let far = [near[0], Rect::centered((30.0, 0.0), 4.0, 4.0)];
        let b = [None, Some(1)];
        assert!(
            score(&input(&near, &[(0, 1)], &b)).proximity
                < score(&input(&far, &[(0, 1)], &b)).proximity
        );
    }

    #[test]
    fn child_closer_to_other_branch_is_penalized() {
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
