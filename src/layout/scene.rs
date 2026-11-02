//! The scene CSP: a strategy's plan plus user constraints, as a `csp::Problem`.

use super::*;
use std::cell::RefCell;

/// Margin in ems that links and labels try to keep off boxes, and labels off each other.
const CLEAR: f64 = 0.4;

/// Margin in ems that labels must keep.
const LABEL_CLEAR: f64 = 0.15;

/// Route is an edge whose bend or label position the solver chooses. Its variable comes after
/// every node's, so all positions are known when its values are checked.
pub(super) struct Route {
    pub(super) edge: usize,
    pub(super) a: usize,
    pub(super) b: usize,
    /// Bends: bows of the curve, then detours around boxes (untrimmed paths).
    pub(super) bows: Vec<f64>,
    pub(super) vias: Vec<Vec<P>>,
    /// (bend, t) pairs in preference order, every bend with the same label spots.
    pub(super) opts: Vec<(usize, f64)>,
    pub(super) label: Option<(f64, f64)>,
    /// Path per option, filled when both endpoints are fixed; curves dominate route checks.
    pub(super) paths: Vec<Vec<P>>,
}

impl Route {
    pub(super) fn bends(&self) -> usize {
        self.bows.len() + self.vias.len()
    }
}

/// Grid buckets one variable's candidate centers. Every candidate of a node has the same box,
/// so "its box meets region R" is "its center lies in R grown by half the box": a range query.
pub(super) struct Grid {
    pub(super) cell: f64,
    /// Cell coordinates of the first column and row, and the row count.
    x0: i64,
    y0: i64,
    ny: i64,
    /// Candidates of cell k (column-major) are `items[start[k]..start[k + 1]]`, so they come out
    /// in the same order every run and `polish` is reproducible.
    start: Vec<u32>,
    items: Vec<u32>,
    /// Non-empty cells as (column, row, cell index).
    full: Vec<(i64, i64, usize)>,
}

impl Grid {
    pub(super) fn new(at: &[P]) -> Grid {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in at {
            (x0, y0, x1, y1) = (x0.min(p.0), y0.min(p.1), x1.max(p.0), y1.max(p.1));
        }
        // About sixteen candidates per bucket.
        let cell = (((x1 - x0) * (y1 - y0)).max(1.0) * 16.0 / at.len().max(1) as f64).sqrt().max(1.0);
        let key = |p: &P| ((p.0 / cell).floor() as i64, (p.1 / cell).floor() as i64);
        let (cx0, cy0, cx1, cy1) = at.iter().map(key).fold((i64::MAX, i64::MAX, i64::MIN, i64::MIN), |(a, b, c, d), (x, y)| (a.min(x), b.min(y), c.max(x), d.max(y)));
        let (nx, ny) = if at.is_empty() { (0, 0) } else { (cx1 - cx0 + 1, cy1 - cy0 + 1) };
        let index = |(x, y): (i64, i64)| ((x - cx0) * ny + (y - cy0)) as usize;
        let mut start = vec![0u32; (nx * ny) as usize + 1];
        for p in at {
            start[index(key(p)) + 1] += 1;
        }
        for k in 1..start.len() {
            start[k] += start[k - 1];
        }
        let mut fill = start.clone();
        let mut items = vec![0; at.len()];
        for (i, p) in at.iter().enumerate() {
            let k = index(key(p));
            items[fill[k] as usize] = i as u32;
            fill[k] += 1;
        }
        let full = (0..nx).flat_map(|x| (0..ny).map(move |y| (cx0 + x, cy0 + y))).filter(|&c| start[index(c)] < start[index(c) + 1]).map(|c| (c.0, c.1, index(c))).collect();
        Grid { cell, x0: cx0, y0: cy0, ny, start, items, full }
    }

    fn bucket(&self, k: usize) -> &[u32] {
        &self.items[self.start[k] as usize..self.start[k + 1] as usize]
    }

    /// shadow calls `f` with every candidate whose center may lie in the wedge seen from
    /// `apex` across the convex hull of `pts` (and some that do not). None when the wedge is
    /// 180 degrees or wider, which happens when `apex` touches the hull.
    pub(super) fn shadow(&self, apex: P, pts: &[P], mut f: impl FnMut(u32)) -> Option<u64> {
        let cross = |a: P, b: P| a.0 * b.1 - a.1 * b.0;
        let n = pts.len() as f64;
        let mid = (pts.iter().map(|p| p.0).sum::<f64>() / n - apex.0, pts.iter().map(|p| p.1).sum::<f64>() / n - apex.1);
        if mid.0.hypot(mid.1) < 1e-9 {
            return None;
        }
        let (mut lo, mut hi) = ((f64::MAX, (0.0, 0.0)), (f64::MIN, (0.0, 0.0)));
        for p in pts {
            let d = (p.0 - apex.0, p.1 - apex.1);
            let ang = cross(mid, d).atan2(mid.0 * d.0 + mid.1 * d.1);
            if ang < lo.0 {
                lo = (ang, d);
            }
            if ang > hi.0 {
                hi = (ang, d);
            }
        }
        if hi.0 - lo.0 >= std::f64::consts::PI - 1e-6 {
            return None;
        }
        let (dlo, dhi) = (lo.1, hi.1);
        let mut n = 0;
        for &(cx, cy, k) in &self.full {
            let (x0, y0) = (cx as f64 * self.cell - apex.0, cy as f64 * self.cell - apex.1);
            let (x1, y1) = (x0 + self.cell, y0 + self.cell);
            let corners = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)];
            // The wedge is left of `dlo` and right of `dhi`; skip cells wholly outside either side.
            if corners.iter().all(|&c| cross(dlo, c) < 0.0) || corners.iter().all(|&c| cross(dhi, c) > 0.0) {
                continue;
            }
            let b = self.bucket(k);
            n += b.len() as u64;
            b.iter().for_each(|&i| f(i));
        }
        Some(n)
    }

    /// near calls `f` with every candidate whose center may lie in `r` (and some that do not).
    pub(super) fn near(&self, r: &Rect, mut f: impl FnMut(u32)) -> u64 {
        let (cx0, cx1) = ((r.x0 / self.cell).floor() as i64, (r.x1 / self.cell).floor() as i64);
        let (cy0, cy1) = ((r.y0 / self.cell).floor() as i64, (r.y1 / self.cell).floor() as i64);
        if (cx1 - cx0 + 1) * (cy1 - cy0 + 1) > self.full.len() as i64 {
            self.items.iter().for_each(|&i| f(i));
            return self.items.len() as u64;
        }
        let nx = (self.start.len() as i64 - 1) / self.ny.max(1);
        let (cx0, cx1) = (cx0.max(self.x0), cx1.min(self.x0 + nx - 1));
        let (cy0, cy1) = (cy0.max(self.y0), cy1.min(self.y0 + self.ny - 1));
        let mut n = 0;
        for cx in cx0..=cx1 {
            // A column's cells are contiguous, so its candidates are one slice.
            if cy0 > cy1 {
                break;
            }
            let k = ((cx - self.x0) * self.ny + (cy0 - self.y0)) as usize;
            let b = &self.items[self.start[k] as usize..self.start[k + (cy1 - cy0) as usize + 1] as usize];
            n += b.len() as u64;
            b.iter().for_each(|&i| f(i));
        }
        n
    }
}

/// Memo is a label box keyed by the values that place it.
type Memo = ([u32; 3], Rect);

/// Scene is a plan plus user constraints, as a `csp::Problem` over variables in plan order.
pub(super) struct Scene<'a> {
    pub(super) g: &'a Graph,
    pub(super) page: Rect,
    pub(super) body: f64,
    pub(super) tol: f64,
    /// Node of each variable, and the variable of each node.
    pub(super) node: Vec<usize>,
    pub(super) var: Vec<usize>,
    pub(super) at: Vec<Vec<P>>,
    pub(super) r: Vec<Vec<f64>>,
    pub(super) rects: Vec<Vec<Rect>>,
    pub(super) scopes: Vec<Vec<usize>>,
    pub(super) kinds: Vec<Kind>,
    pub(super) weights: Vec<u64>,
    /// Candidate box size and shape per node variable, and a spatial index over its candidates.
    pub(super) size: Vec<(f64, f64)>,
    pub(super) shapes: Vec<Shape>,
    pub(super) grids: Vec<Grid>,
    pub(super) routes: Vec<Route>,
    /// Per constraint, when routing: whether two links stay apart, by bend of each (BOWS order).
    pub(super) apart: Vec<Vec<bool>>,
    /// Per route, its last label box keyed by the values of its endpoints and itself: labels
    /// are checked against every node, so the same curve is asked for many times in a row.
    pub(super) labels: RefCell<Vec<Option<Memo>>>,
}

impl Scene<'_> {
    pub(super) fn center(&self, u: usize, val: &[u32]) -> P {
        self.at[self.var[u]][val[self.var[u]] as usize]
    }

    pub(super) fn node_rect(&self, u: usize, val: &[u32]) -> Rect {
        self.rects[self.var[u]][val[self.var[u]] as usize]
    }

    pub(super) fn route_path(&self, i: usize, val: &[u32]) -> Cow<'_, [P]> {
        let r = &self.routes[i];
        let k = val[self.node.len() + i] as usize;
        if let Some(p) = r.paths.get(k) {
            return Cow::Borrowed(p);
        }
        let inside = |u: usize, p: P| {
            let (w, h) = self.size[self.var[u]];
            inside_box(self.center(u, val), self.shapes[self.var[u]], w, h, p)
        };
        Cow::Owned(match r.opts[k].0.checked_sub(r.bows.len()) {
            Some(j) => trim(&r.vias[j], &|p| inside(r.a, p), &|p| inside(r.b, p)),
            None => curve(self.center(r.a, val), &|p| inside(r.a, p), self.center(r.b, val), &|p| inside(r.b, p), r.bows[r.opts[k].0]),
        })
    }

    pub(super) fn route_label(&self, i: usize, val: &[u32]) -> Rect {
        let r = &self.routes[i];
        let key = [val[self.var[r.a]], val[self.var[r.b]], val[self.node.len() + i]];
        let mut memo = self.labels.borrow_mut();
        if memo.len() <= i {
            memo.resize(self.routes.len(), None);
        }
        if let Some((k, rect)) = memo[i]
            && k == key
        {
            return rect;
        }
        let (_, t) = r.opts[key[2] as usize];
        let rect = label_rect(&self.route_path(i, val), t, r.label.unwrap());
        memo[i] = Some((key, rect));
        rect
    }
}

impl Problem for Scene<'_> {
    fn num_vars(&self) -> usize {
        self.node.len() + self.routes.len()
    }

    fn domain_size(&self, v: usize) -> usize {
        match v.checked_sub(self.node.len()) {
            Some(i) => self.routes[i].opts.len(),
            None => self.at[v].len(),
        }
    }

    fn scopes(&self) -> &[Vec<usize>] {
        &self.scopes
    }

    fn weight(&self, c: usize) -> u64 {
        self.weights[c]
    }

    fn propagate(&self, c: usize) -> bool {
        matches!(self.kinds[c], Kind::Rel(_) | Kind::Aligned(_))
    }

    fn filter(&self, c: usize, val: &[u32], y: usize, removed: &[u32], bad: &mut Vec<usize>) -> Option<u64> {
        if self.grids.is_empty() || y >= self.node.len() {
            return None;
        }
        let s = &self.scopes[c];
        let (w, h) = self.size[y];
        let mut tests = 0;
        // Generic, not a closure, so `hit` inlines into the scan.
        fn visit(a: u32, removed: &[u32], tests: &mut u64, bad: &mut Vec<usize>, hit: impl Fn(usize) -> bool) {
            let a = a as usize;
            if removed[a] == csp::UNSET {
                *tests += 1;
                if hit(a) {
                    bad.push(a);
                }
            }
        }
        match self.kinds[c] {
            Kind::Apart(gap) => {
                let other = if s[0] == y { s[1] } else { s[0] };
                let r = self.rects[other][val[other] as usize].inflate(gap);
                let grown = Rect { x0: r.x0 - w / 2.0, y0: r.y0 - h / 2.0, x1: r.x1 + w / 2.0, y1: r.y1 + h / 2.0 };
                self.grids[y].near(&grown, |a| visit(a, removed, &mut tests, bad, |a| r.intersects(&self.rects[y][a])));
                Some(tests)
            }
            Kind::Clear(thin) if s[2] == y => {
                let (p, q) = (self.at[s[0]][val[s[0]] as usize], self.at[s[1]][val[s[1]] as usize]);
                let grown = Rect { x0: p.0.min(q.0) - thin - w / 2.0, y0: p.1.min(q.1) - thin - h / 2.0, x1: p.0.max(q.0) + thin + w / 2.0, y1: p.1.max(q.1) + thin + h / 2.0 };
                self.grids[y].near(&grown, |a| visit(a, removed, &mut tests, bad, |a| segment_hits_rect(p, q, &self.rects[y][a].inflate(thin))));
                Some(tests)
            }
            Kind::Clear(thin) => {
                // An endpoint is open: the edge hits the box only from inside the box's shadow.
                let (apex, open) = if s[0] == y { (self.at[s[1]][val[s[1]] as usize], 0) } else { (self.at[s[0]][val[s[0]] as usize], 1) };
                let r = self.rects[s[2]][val[s[2]] as usize].inflate(thin);
                let corners = [(r.x0, r.y0), (r.x1, r.y0), (r.x0, r.y1), (r.x1, r.y1)];
                let ends = |a: usize| if open == 0 { (self.at[y][a], apex) } else { (apex, self.at[y][a]) };
                self.grids[y].shadow(apex, &corners, |a| {
                    visit(a, removed, &mut tests, bad, |a| {
                        let (p, q) = ends(a);
                        segment_hits_rect(p, q, &r)
                    })
                })?;
                Some(tests)
            }
            Kind::NoCross => {
                // The open endpoint's edge crosses the other edge only from inside its shadow.
                let k = s.iter().position(|&v| v == y).unwrap();
                let partner = k ^ 1;
                let (o0, o1) = if k < 2 { (2, 3) } else { (0, 1) };
                let pos = |i: usize| self.at[s[i]][val[s[i]] as usize];
                let apex = pos(partner);
                let other = [pos(o0), pos(o1)];
                self.grids[y].shadow(apex, &other, |a| {
                    visit(a, removed, &mut tests, bad, |a| {
                        let mut seg = [(0.0, 0.0); 4];
                        for (i, p) in seg.iter_mut().enumerate() {
                            *p = if i == k { self.at[y][a] } else { pos(i) };
                        }
                        segments_cross(seg[0], seg[1], seg[2], seg[3])
                    })
                })?;
                Some(tests)
            }
            Kind::Outside(ci, _) if s[s.len() - 1] == y => {
                let f = frame(&self.g.constraints[ci].rel, &|v| self.node_rect(v, val), self.body).inflate(FRAME_CLEAR * self.body);
                let grown = Rect { x0: f.x0 - w / 2.0, y0: f.y0 - h / 2.0, x1: f.x1 + w / 2.0, y1: f.y1 + h / 2.0 };
                self.grids[y].near(&grown, |a| visit(a, removed, &mut tests, bad, |a| f.intersects(&self.rects[y][a])));
                Some(tests)
            }
            _ => None,
        }
    }

    fn holds(&self, c: usize, val: &[u32]) -> bool {
        let s = &self.scopes[c];
        let at = |k: usize| self.at[s[k]][val[s[k]] as usize];
        let rect = |k: usize| self.rects[s[k]][val[s[k]] as usize];
        match self.kinds[c] {
            Kind::Apart(gap) => !rect(0).inflate(gap).intersects(&rect(1)),
            Kind::Outward(step) => self.r[s[1]][val[s[1]] as usize] >= self.r[s[0]][val[s[0]] as usize] + step - 1e-9,
            Kind::Clear(thin) => !segment_hits_rect(at(0), at(1), &rect(2).inflate(thin)),
            Kind::NoCross => !segments_cross(at(0), at(1), at(2), at(3)),
            Kind::Aligned(side) => aligned(side, &rect(0), &rect(1), self.tol),
            Kind::Rel(ci) => {
                let by_node = |u: usize| self.rects[self.var[u]][val[self.var[u]] as usize];
                relation_holds(&self.g.constraints[ci].rel, &by_node, self.body, &self.page, self.tol)
            }
            Kind::LabelClear(i, u, m) => !self.route_label(i, val).intersects(&self.node_rect(u, val).inflate(m * self.body)),
            Kind::LabelPage(i) => self.route_label(i, val).within(&self.page),
            Kind::LabelApart(i, j, m) => !self.route_label(i, val).inflate(m * self.body).intersects(&self.route_label(j, val)),
            Kind::LinkApart(i, j) if self.apart.get(c).is_some_and(|t| !t.is_empty()) => {
                let bend = |i: usize| self.routes[i].opts[val[self.node.len() + i] as usize].0;
                self.apart[c][bend(i) * self.routes[j].bends() + bend(j)]
            }
            Kind::LinkApart(i, j) => {
                let (ri, rj) = (&self.routes[i], &self.routes[j]);
                let skip: Vec<Rect> = [ri.a, ri.b]
                    .into_iter()
                    .filter(|u| *u == rj.a || *u == rj.b)
                    .map(|u| self.node_rect(u, val).inflate(1.5 * self.body))
                    .collect();
                !paths_meet(&self.route_path(i, val), &self.route_path(j, val), 0.5 * self.body, &skip)
            }
            Kind::Outside(ci, u) => {
                let rel = &self.g.constraints[ci].rel;
                frame_clear(&frame(rel, &|v| self.node_rect(v, val), self.body), &self.node_rect(u, val), self.body)
            }
            Kind::Frames(ci, cj) => {
                let f = |c: usize| frame(&self.g.constraints[c].rel, &|v| self.node_rect(v, val), self.body);
                frame_clear(&f(ci), &f(cj), self.body)
            }
            Kind::LabelFrame(i, ci) => {
                let rel = &self.g.constraints[ci].rel;
                let f = frame(rel, &|v| self.node_rect(v, val), self.body);
                let l = self.route_label(i, val);
                let titled = matches!(rel, Relation::Group { title: Some(_), .. });
                let inner = Rect { y1: f.y1 - if titled { TITLE_BAND * self.body } else { 0.0 }, ..f }.inflate(-LABEL_CLEAR * self.body);
                !l.intersects(&f.inflate(CLEAR * self.body)) || l.within(&inner)
            }
            Kind::LinkEdge(i, p, q) => {
                let (a, b) = (self.center(p, val), self.center(q, val));
                !self.route_path(i, val).windows(2).any(|s| segments_cross(s[0], s[1], a, b))
            }
            Kind::LinkClear(i, u, margin) => {
                let (w, h) = self.size[self.var[u]];
                let (w, h) = (w + 2.0 * margin * self.body, h + 2.0 * margin * self.body);
                let c = self.center(u, val);
                let shape = self.shapes[self.var[u]];
                !self.route_path(i, val).iter().any(|&p| inside_box(c, shape, w, h, p))
            }
        }
    }
}

/// build turns one plan into a CSP scene, plus the plan's edges and its (kx, ky, tol).
#[allow(clippy::too_many_arguments)]
pub(super) fn build<'a>(g: &'a Graph, t: &Tree, fonts: &Fonts, cfg: &Config, b: f64, users: Users, kind: LayoutKind, boxes: &[Boxed], rot: f64) -> (Scene<'a>, Vec<Drawn>, (f64, f64, f64)) {
    let n = g.nodes.len();
    let plan = match kind {
        LayoutKind::Radial => radial::plan(g, t, boxes, cfg, b, rot),
        LayoutKind::Layered(dir) => layered::plan(g, fonts, boxes, cfg, b, dir),
    };
    let mut plan = plan;
    // Locked spots go before the plan's own; a known layout below goes before both.
    if let Some(a) = cfg.anchor {
        for (v, c) in plan.cands.iter_mut().enumerate() {
            if let Some(q) = a.pos[v] {
                c.insert(0, Cand { cost: f64::NEG_INFINITY, at: q, r: (q.0 / plan.kx).hypot(q.1 / plan.ky) });
            }
        }
    }
    // Starting from a known layout guarantees a first solution; search then improves on it.
    if let Users::Shrunk(prev) | Users::All(prev) | Users::Routes(prev) = users {
        for (v, c) in plan.cands.iter_mut().enumerate() {
            c.insert(0, Cand { cost: f64::NEG_INFINITY, at: prev.pos[v], r: prev.radius(v) });
        }
    }
    let mut var = vec![0; n];
    for (i, &v) in plan.order.iter().enumerate() {
        var[v] = i;
    }
    let mut sc = Scene {
        g,
        page: cfg.page(),
        body: b,
        tol: plan.tol,
        node: plan.order.clone(),
        var,
        at: Vec::new(),
        r: Vec::new(),
        rects: Vec::new(),
        scopes: Vec::new(),
        kinds: Vec::new(),
        weights: Vec::new(),
        size: Vec::new(),
        shapes: Vec::new(),
        grids: Vec::new(),
        routes: Vec::new(),
        apart: Vec::new(),
        labels: RefCell::new(Vec::new()),
    };
    for &v in &plan.order {
        let c = &plan.cands[v];
        sc.size.push((boxes[v].w, boxes[v].h));
        sc.shapes.push(boxes[v].shape);
        sc.at.push(c.iter().map(|c| c.at).collect());
        sc.r.push(c.iter().map(|c| c.r).collect());
        sc.rects.push(c.iter().map(|c| Rect::centered(c.at, boxes[v].w, boxes[v].h)).collect());
    }
    let mut cons = plan.cons;
    for (ci, c) in g.constraints.iter().enumerate() {
        let on = match users {
            Users::Hard(set) => c.hard && set.contains(&ci),
            Users::All(_) | Users::Wanted => true,
            Users::Routes(_) | Users::Shrunk(_) => c.hard,
        };
        if !on {
            continue;
        }
        let w = if c.hard || matches!(users, Users::Wanted) { 0 } else { c.weight as u64 };
        match &c.rel {
            Relation::Align { side, items } if c.hard => {
                for (i, &a) in items.iter().enumerate() {
                    for &b in &items[i + 1..] {
                        cons.push((vec![a, b], Kind::Aligned(*side), 0));
                    }
                }
            }
            Relation::Group { items, .. } => {
                cons.push((items.clone(), Kind::Rel(ci), 0));
                for u in (0..n).filter(|u| !items.contains(u)) {
                    cons.push(([&items[..], &[u]].concat(), Kind::Outside(ci, u), 0));
                }
                for (cj, d) in g.constraints.iter().enumerate().take(ci) {
                    if let Relation::Group { items: other, .. } = &d.rel
                        && cons.iter().any(|c| matches!(c.1, Kind::Rel(k) if k == cj))
                    {
                        cons.push(([&items[..], &other[..]].concat(), Kind::Frames(cj, ci), 0));
                    }
                }
            }
            rel => cons.push((rel.nodes(), Kind::Rel(ci), w)),
        }
    }
    sc.grids = sc.at.iter().map(|a| Grid::new(a)).collect();
    let optimizing = matches!(users, Users::All(_));
    let routing = matches!(users, Users::Routes(_));
    if let Users::Routes(prev) = users {
        for (i, &v) in sc.node.iter().enumerate() {
            let k = sc.at[i].iter().position(|&p| p == prev.pos[v]).expect("fixed node keeps its spot");
            sc.at[i] = vec![sc.at[i][k]];
            sc.r[i] = vec![sc.r[i][k]];
            sc.rects[i] = vec![sc.rects[i][k]];
        }
    }
    let mut edges = plan.edges;
    for (k, e) in edges.iter_mut().enumerate() {
        let link = e.style == EdgeStyle::Link;
        if !link && e.label.is_none() {
            continue;
        }
        let bows: Vec<f64> = if link { BOWS.to_vec() } else { vec![0.0] };
        let mut vias = Vec::new();
        if link && routing {
            // Nodes are fixed, so detours around the other boxes can be computed up front.
            let others: Vec<Rect> = (0..n).filter(|&u| u != e.a && u != e.b).map(|u| sc.rects[sc.var[u]][0]).collect();
            vias = detours(sc.at[sc.var[e.a]][0], sc.at[sc.var[e.b]][0], &others, &sc.page, 0.4 * b);
        }
        let spots: &[f64] = if e.label.is_some() { &SPOTS } else { &[0.5] };
        let opts = (0..bows.len() + vias.len()).flat_map(|k| spots.iter().map(move |&t| (k, t))).collect();
        let label = e.label.as_ref().map(|l| label_size(fonts, l, b));
        e.label_size = label.unwrap_or((0.0, 0.0));
        sc.routes.push(Route { edge: k, a: e.a, b: e.b, bows, vias, opts, label, paths: Vec::new() });
        if routing {
            // Every node has a single spot, so value 0 everywhere places them all.
            let i = sc.routes.len() - 1;
            let mut val = vec![0; sc.node.len() + i + 1];
            if link {
                // Bends that clear every box go first, so branch and bound finds a good
                // routing early even when its budget runs out.
                let r = &sc.routes[i];
                let hits: Vec<usize> = (0..r.bends())
                    .map(|k| {
                        val[sc.node.len() + i] = r.opts.iter().position(|o| o.0 == k).unwrap() as u32;
                        let path = sc.route_path(i, &val);
                        (0..n).filter(|&u| u != r.a && u != r.b && path.iter().any(|&p| inside_box(sc.at[sc.var[u]][0], sc.shapes[sc.var[u]], sc.size[sc.var[u]].0, sc.size[sc.var[u]].1, p))).count()
                    })
                    .collect();
                sc.routes[i].opts.sort_by_key(|o| hits[o.0]);
            }
            sc.routes[i].paths = (0..sc.routes[i].opts.len() as u32)
                .map(|o| {
                    val[sc.node.len() + i] = o;
                    sc.route_path(i, &val).into_owned()
                })
                .collect();
        }
    }
    // Scopes over nodes plus route variables; duplicates are dropped because a route's
    // endpoints may be the same nodes as the constraint's others.
    let base = sc.node.len();
    let push = |sc: &mut Scene, nodes: &[usize], routes: &[usize], kind: Kind, w: u64| {
        let mut scope: Vec<usize> = nodes.iter().map(|&u| sc.var[u]).collect();
        for &i in routes {
            scope.extend([sc.var[sc.routes[i].a], sc.var[sc.routes[i].b], base + i]);
        }
        // Keep first occurrences in order: structural kinds read their scope by position.
        let mut seen = Vec::new();
        scope.retain(|v| !seen.contains(v) && {
            seen.push(*v);
            true
        });
        sc.scopes.push(scope);
        sc.kinds.push(kind);
        sc.weights.push(w);
    };
    for (nodes, kind, w) in cons {
        if w == 0 || optimizing {
            push(&mut sc, &nodes, &[], kind, w);
        }
    }
    let links: Vec<usize> = (0..sc.routes.len()).filter(|&i| edges[sc.routes[i].edge].style == EdgeStyle::Link).collect();
    for i in 0..sc.routes.len() {
        if sc.routes[i].label.is_some() {
            push(&mut sc, &[], &[i], Kind::LabelPage(i), 0);
            // Labels must not touch boxes or each other; once nodes are fixed, they also try
            // to keep a margin.
            for u in 0..n {
                push(&mut sc, &[u], &[i], Kind::LabelClear(i, u, LABEL_CLEAR), 0);
                if routing {
                    push(&mut sc, &[u], &[i], Kind::LabelClear(i, u, CLEAR), 2);
                }
            }
            if routing {
                for (ci, c) in g.constraints.iter().enumerate() {
                    if let Relation::Group { items, .. } = &c.rel {
                        push(&mut sc, items, &[i], Kind::LabelFrame(i, ci), 2);
                    }
                }
            }
            for j in i + 1..sc.routes.len() {
                if sc.routes[j].label.is_some() {
                    push(&mut sc, &[], &[i, j], Kind::LabelApart(i, j, LABEL_CLEAR), 0);
                    if routing {
                        push(&mut sc, &[], &[i, j], Kind::LabelApart(i, j, CLEAR), 2);
                    }
                }
            }
        }
        if routing && links.contains(&i) {
            let (a, b2) = (sc.routes[i].a, sc.routes[i].b);
            // Weights follow what a reader notices most: a link through a box (W0201), two
            // links meeting (W0207), a link grazing a box, a link over a tree edge.
            for u in (0..n).filter(|&u| u != a && u != b2) {
                push(&mut sc, &[u], &[i], Kind::LinkClear(i, u, 0.0), 40);
                push(&mut sc, &[u], &[i], Kind::LinkClear(i, u, CLEAR), 2);
            }
            for &j in links.iter().filter(|&&j| j > i) {
                push(&mut sc, &[], &[i, j], Kind::LinkApart(i, j), 20);
            }
            for e in edges.iter().filter(|e| e.style != EdgeStyle::Link && ![a, b2].contains(&e.a) && ![a, b2].contains(&e.b)) {
                push(&mut sc, &[e.a, e.b], &[i], Kind::LinkEdge(i, e.a, e.b), 1);
            }
        }
    }
    if routing {
        sc.grids.clear();
        // Nodes are fixed, so whether two links meet depends only on their bends; the search
        // asks the same question many times, so answer each pair once.
        sc.apart = vec![Vec::new(); sc.kinds.len()];
        for c in 0..sc.kinds.len() {
            let Kind::LinkApart(i, j) = sc.kinds[c] else { continue };
            let (ni, nj) = (sc.routes[i].bends(), sc.routes[j].bends());
            let first = |r: &Route, k: usize| r.opts.iter().position(|o| o.0 == k).unwrap() as u32;
            let mut val = vec![0; sc.node.len() + sc.routes.len()];
            let mut table = Vec::with_capacity(ni * nj);
            for bi in 0..ni {
                for bj in 0..nj {
                    val[sc.node.len() + i] = first(&sc.routes[i], bi);
                    val[sc.node.len() + j] = first(&sc.routes[j], bj);
                    table.push(sc.holds(c, &val));
                }
            }
            sc.apart[c] = table;
        }
    }
    (sc, edges, (plan.kx, plan.ky, plan.tol))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Settings;


    /// scene builds `k` random candidates for each of `n` boxes, with every `Apart` pair, a
    /// path of edges that must clear every box, and crossing-free edge pairs.
    fn scene(g: &Graph, n: usize, k: usize, seed: u64) -> Scene<'_> {
        let mut x = seed;
        let mut rnd = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x % 10_000) as f64 / 10.0 - 500.0
        };
        let mut sc = Scene {
            g,
            page: Rect::centered((0.0, 0.0), 1000.0, 1000.0),
            body: 10.0,
            tol: 1.0,
            node: (0..n).collect(),
            var: (0..n).collect(),
            at: Vec::new(),
            r: Vec::new(),
            rects: Vec::new(),
            scopes: Vec::new(),
            kinds: Vec::new(),
            weights: Vec::new(),
            size: Vec::new(),
            shapes: Vec::new(),
            grids: Vec::new(),
            routes: Vec::new(),
            apart: Vec::new(),
            labels: RefCell::new(Vec::new()),
        };
        for _ in 0..n {
            let (w, h) = (300.0 + rnd().abs() / 3.0, 160.0 + rnd().abs() / 4.0);
            let at: Vec<P> = (0..k).map(|_| (rnd(), rnd())).collect();
            sc.rects.push(at.iter().map(|&p| Rect::centered(p, w, h)).collect());
            sc.r.push(vec![0.0; at.len()]);
            sc.at.push(at);
            sc.size.push((w, h));
            sc.shapes.push(Shape::Rect);
        }
        for i in 0..n {
            for j in i + 1..n {
                sc.scopes.push(vec![i, j]);
                sc.kinds.push(Kind::Apart(5.0));
                sc.weights.push(0);
            }
            if i + 1 < n {
                for u in (0..n).filter(|&u| u != i && u != i + 1) {
                    sc.scopes.push(vec![i, i + 1, u]);
                    sc.kinds.push(Kind::Clear(1.0));
                    sc.weights.push(0);
                }
            }
            // Edges 0-1, 2-3, 4-5: pairwise crossing-free.
            if i % 2 == 0 && i + 1 < n {
                for j in (i + 2..n - 1).step_by(2) {
                    sc.scopes.push(vec![i, i + 1, j, j + 1]);
                    sc.kinds.push(Kind::NoCross);
                    sc.weights.push(0);
                }
            }
        }
        sc
    }

    #[test]
    fn grid_filter_matches_plain_checks() {
        let g = Graph { title: None, root: None, nodes: vec![], edges: vec![], constraints: vec![], settings: Settings::default() };
        let (mut sat, mut unsat, mut jumps) = (0, 0, 0);
        // Many candidates: mostly satisfiable after some search. Few: mostly unsatisfiable.
        let cases = (1..12).map(|seed| (8 + seed as usize % 2, 100, seed)).chain((1..12).map(|seed| (10, 25, seed)));
        for (n, k, seed) in cases {
            let mut sc = scene(&g, n, k, seed * 7919);
            let (plain, ps) = csp::solve(&sc, u64::MAX, &AtomicBool::new(false));
            sc.grids = sc.at.iter().map(|a| Grid::new(a)).collect();
            let (fast, fs) = csp::solve(&sc, u64::MAX, &AtomicBool::new(false));
            assert_eq!((ps.nodes, ps.backjumps), (fs.nodes, fs.backjumps), "seed {seed}");
            match fast {
                Outcome::Sat(_) => sat += 1,
                _ => unsat += 1,
            }
            assert_eq!(plain, fast, "seed {seed}");
            jumps += fs.backjumps;
        }
        // The scenes must be hard enough to exercise pruning, wipeouts, and backjumps.
        assert!(sat > 0 && unsat > 0 && jumps > 0, "sat {sat} unsat {unsat} backjumps {jumps}");
    }
}
