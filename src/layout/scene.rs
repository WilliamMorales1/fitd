//! The scene CSP: a strategy's plan plus user constraints, as a `csp::Problem`.

use super::*;

/// Route is an edge whose bend or label position the solver chooses. Its variable comes after
/// every node's, so all positions are known when its values are checked.
pub(super) struct Route {
    pub(super) edge: usize,
    pub(super) a: usize,
    pub(super) b: usize,
    /// (bow, t) pairs in preference order.
    pub(super) opts: Vec<(f64, f64)>,
    pub(super) label: Option<(f64, f64)>,
    /// Path per option, filled when both endpoints are fixed; curves dominate route checks.
    pub(super) paths: Vec<Vec<P>>,
}

/// Grid buckets one variable's candidate centers. Every candidate of a node has the same box,
/// so "its box meets region R" is "its center lies in R grown by half the box": a range query.
pub(super) struct Grid {
    pub(super) cell: f64,
    // Ordered, so candidates come out in the same order every run and `polish` is reproducible.
    pub(super) buckets: BTreeMap<(i64, i64), Vec<u32>>,
}

impl Grid {
    pub(super) fn new(at: &[P]) -> Grid {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in at {
            (x0, y0, x1, y1) = (x0.min(p.0), y0.min(p.1), x1.max(p.0), y1.max(p.1));
        }
        // About sixteen candidates per bucket.
        let cell = (((x1 - x0) * (y1 - y0)).max(1.0) * 16.0 / at.len().max(1) as f64).sqrt().max(1.0);
        let mut buckets: BTreeMap<(i64, i64), Vec<u32>> = BTreeMap::new();
        for (i, p) in at.iter().enumerate() {
            buckets.entry(((p.0 / cell).floor() as i64, (p.1 / cell).floor() as i64)).or_default().push(i as u32);
        }
        Grid { cell, buckets }
    }

    /// shadow calls `f` with every candidate whose center may lie in the wedge seen from
    /// `apex` across the convex hull of `pts` (and some that do not). None when the wedge is
    /// 180 degrees or wider, which happens when `apex` touches the hull.
    pub(super) fn shadow(&self, apex: P, pts: &[P], f: &mut dyn FnMut(u32)) -> Option<u64> {
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
        for (&(cx, cy), b) in &self.buckets {
            let (x0, y0) = (cx as f64 * self.cell - apex.0, cy as f64 * self.cell - apex.1);
            let (x1, y1) = (x0 + self.cell, y0 + self.cell);
            let corners = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)];
            // The wedge is left of `dlo` and right of `dhi`; skip cells wholly outside either side.
            if corners.iter().all(|&c| cross(dlo, c) < 0.0) || corners.iter().all(|&c| cross(dhi, c) > 0.0) {
                continue;
            }
            n += b.len() as u64;
            b.iter().for_each(|&i| f(i));
        }
        Some(n)
    }

    /// near calls `f` with every candidate whose center may lie in `r` (and some that do not).
    pub(super) fn near(&self, r: &Rect, f: &mut dyn FnMut(u32)) -> u64 {
        let (cx0, cx1) = ((r.x0 / self.cell).floor() as i64, (r.x1 / self.cell).floor() as i64);
        let (cy0, cy1) = ((r.y0 / self.cell).floor() as i64, (r.y1 / self.cell).floor() as i64);
        let mut n = 0;
        if (cx1 - cx0 + 1) * (cy1 - cy0 + 1) > self.buckets.len() as i64 {
            for b in self.buckets.values() {
                n += b.len() as u64;
                b.iter().for_each(|&i| f(i));
            }
            return n;
        }
        for cx in cx0..=cx1 {
            for cy in cy0..=cy1 {
                if let Some(b) = self.buckets.get(&(cx, cy)) {
                    n += b.len() as u64;
                    b.iter().for_each(|&i| f(i));
                }
            }
        }
        n
    }
}

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
        let (bow, _) = r.opts[k];
        let inside = |u: usize, p: P| {
            let (w, h) = self.size[self.var[u]];
            inside_box(self.center(u, val), self.shapes[self.var[u]], w, h, p)
        };
        Cow::Owned(curve(self.center(r.a, val), &|p| inside(r.a, p), self.center(r.b, val), &|p| inside(r.b, p), bow))
    }

    pub(super) fn route_label(&self, i: usize, val: &[u32]) -> Rect {
        let r = &self.routes[i];
        let (_, t) = r.opts[val[self.node.len() + i] as usize];
        label_rect(&self.route_path(i, val), t, r.label.unwrap())
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

    fn filter(&self, c: usize, val: &[u32], y: usize, alive: &dyn Fn(usize) -> bool, bad: &mut dyn FnMut(usize)) -> Option<u64> {
        if self.grids.is_empty() || y >= self.node.len() {
            return None;
        }
        let s = &self.scopes[c];
        let (w, h) = self.size[y];
        let mut tests = 0;
        let mut visit = |a: u32, hit: &dyn Fn(usize) -> bool| {
            let a = a as usize;
            if alive(a) {
                tests += 1;
                if hit(a) {
                    bad(a);
                }
            }
        };
        match self.kinds[c] {
            Kind::Apart(gap) => {
                let other = if s[0] == y { s[1] } else { s[0] };
                let r = self.rects[other][val[other] as usize].inflate(gap);
                let grown = Rect { x0: r.x0 - w / 2.0, y0: r.y0 - h / 2.0, x1: r.x1 + w / 2.0, y1: r.y1 + h / 2.0 };
                self.grids[y].near(&grown, &mut |a| visit(a, &|a| r.intersects(&self.rects[y][a])));
                Some(tests)
            }
            Kind::Clear(thin) if s[2] == y => {
                let (p, q) = (self.at[s[0]][val[s[0]] as usize], self.at[s[1]][val[s[1]] as usize]);
                let grown = Rect { x0: p.0.min(q.0) - thin - w / 2.0, y0: p.1.min(q.1) - thin - h / 2.0, x1: p.0.max(q.0) + thin + w / 2.0, y1: p.1.max(q.1) + thin + h / 2.0 };
                self.grids[y].near(&grown, &mut |a| visit(a, &|a| segment_hits_rect(p, q, &self.rects[y][a].inflate(thin))));
                Some(tests)
            }
            Kind::Clear(thin) => {
                // An endpoint is open: the edge hits the box only from inside the box's shadow.
                let (apex, open) = if s[0] == y { (self.at[s[1]][val[s[1]] as usize], 0) } else { (self.at[s[0]][val[s[0]] as usize], 1) };
                let r = self.rects[s[2]][val[s[2]] as usize].inflate(thin);
                let corners = [(r.x0, r.y0), (r.x1, r.y0), (r.x0, r.y1), (r.x1, r.y1)];
                let ends = |a: usize| if open == 0 { (self.at[y][a], apex) } else { (apex, self.at[y][a]) };
                self.grids[y].shadow(apex, &corners, &mut |a| {
                    visit(a, &|a| {
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
                self.grids[y].shadow(apex, &other, &mut |a| {
                    visit(a, &|a| {
                        let mut seg = [(0.0, 0.0); 4];
                        for (i, p) in seg.iter_mut().enumerate() {
                            *p = if i == k { self.at[y][a] } else { pos(i) };
                        }
                        segments_cross(seg[0], seg[1], seg[2], seg[3])
                    })
                })?;
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
            Kind::LabelClear(i, u) => !self.route_label(i, val).intersects(&self.node_rect(u, val).inflate(0.15 * self.body)),
            Kind::LabelPage(i) => self.route_label(i, val).within(&self.page),
            Kind::LabelApart(i, j) => !self.route_label(i, val).inflate(0.15 * self.body).intersects(&self.route_label(j, val)),
            Kind::LinkApart(i, j) if self.apart.get(c).is_some_and(|t| !t.is_empty()) => {
                let bend = |i: usize| val[self.node.len() + i] as usize / (self.routes[i].opts.len() / BOWS.len());
                self.apart[c][bend(i) * BOWS.len() + bend(j)]
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
            Kind::LinkClear(i, u) => {
                let (w, h) = self.size[self.var[u]];
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
        LayoutKind::Radial => radial::plan(t, boxes, cfg, b, rot),
        LayoutKind::Layered(dir) => layered::plan(g, boxes, cfg, b, dir),
    };
    let mut plan = plan;
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
            Users::All(_) => true,
            Users::Routes(_) | Users::Shrunk(_) => c.hard,
        };
        if !on {
            continue;
        }
        let w = if c.hard { 0 } else { c.weight as u64 };
        match &c.rel {
            Relation::Align { side, items } if c.hard => {
                for (i, &a) in items.iter().enumerate() {
                    for &b in &items[i + 1..] {
                        cons.push((vec![a, b], Kind::Aligned(*side), 0));
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
        let bows: &[f64] = if link { &BOWS } else { &[0.0] };
        let spots: &[f64] = if e.label.is_some() { &SPOTS } else { &[0.5] };
        let opts = bows.iter().flat_map(|&bow| spots.iter().map(move |&t| (bow, t))).collect();
        let label = e.label.as_ref().map(|l| label_size(fonts, l, b));
        e.label_size = label.unwrap_or((0.0, 0.0));
        sc.routes.push(Route { edge: k, a: e.a, b: e.b, opts, label, paths: Vec::new() });
        if routing {
            // Every node has a single spot, so value 0 everywhere places them all.
            let i = sc.routes.len() - 1;
            let mut val = vec![0; sc.node.len() + i + 1];
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
            for u in 0..n {
                push(&mut sc, &[u], &[i], Kind::LabelClear(i, u), 0);
            }
            for j in i + 1..sc.routes.len() {
                if sc.routes[j].label.is_some() {
                    push(&mut sc, &[], &[i, j], Kind::LabelApart(i, j), 0);
                }
            }
        }
        if routing && links.contains(&i) {
            let (a, b2) = (sc.routes[i].a, sc.routes[i].b);
            for u in (0..n).filter(|&u| u != a && u != b2) {
                push(&mut sc, &[u], &[i], Kind::LinkClear(i, u), 2);
            }
            for &j in links.iter().filter(|&&j| j > i) {
                push(&mut sc, &[], &[i, j], Kind::LinkApart(i, j), 1);
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
            let (si, sj) = (sc.routes[i].opts.len() / BOWS.len(), sc.routes[j].opts.len() / BOWS.len());
            let mut val = vec![0; sc.node.len() + sc.routes.len()];
            let mut table = Vec::with_capacity(BOWS.len() * BOWS.len());
            for bi in 0..BOWS.len() {
                for bj in 0..BOWS.len() {
                    val[sc.node.len() + i] = (bi * si) as u32;
                    val[sc.node.len() + j] = (bj * sj) as u32;
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
