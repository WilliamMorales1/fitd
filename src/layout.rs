//! Layout core shared by every strategy: text measurement, the scene CSP, and the outer
//! search. A strategy (radial, layered) proposes candidate spots per node plus its own
//! structural constraints; the user's `require`/`prefer` constraints are added on top. The
//! outer search finds the largest body font that is satisfiable, then spends a second budget
//! on soft preferences. When even the smallest font fails, it extracts a minimal set of
//! conflicting user constraints (QuickXplain).

use crate::csp::{self, Outcome, Problem};
use crate::geom::{P, Rect, segment_hits_rect, segments_cross};
pub use crate::graph::Shape;
use crate::graph::{Graph, LayoutKind, Relation, Side};
use crate::metrics::{Fonts, Run};
use crate::tree::Tree;
use crate::{layered, radial};
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub struct Config {
    pub page_w: f64,
    pub page_h: f64,
    pub margin: f64,
    pub min_font: f64,
    pub max_font: f64,
    /// Leaf wrap width in ems of the body font.
    pub wrap_em: f64,
    /// Constraint checks allowed per CSP attempt before declaring it infeasible.
    pub budget: u64,
    /// Constraint checks spent improving soft preferences once the font is fixed.
    pub opt_budget: u64,
}

impl Config {
    pub fn page(&self) -> Rect {
        Rect::centered((0.0, 0.0), self.page_w - 2.0 * self.margin, self.page_h - 2.0 * self.margin)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Role {
    Root,
    Hub,
    Leaf,
}

pub struct TextLine {
    pub runs: Vec<Run>,
    pub size: f64,
    pub bold: bool,
    pub width: f64,
    /// Offsets from the box's top-left corner (y grows downward inside the box).
    pub x: f64,
    pub baseline: f64,
}

pub struct Boxed {
    pub shape: Shape,
    pub role: Role,
    pub w: f64,
    pub h: f64,
    pub pad: f64,
    pub lines: Vec<TextLine>,
}

#[derive(Default, Clone, Copy)]
pub struct Stats {
    pub attempts: usize,
    pub search: csp::Stats,
    /// Total weight of soft constraints left unmet, and of all soft constraints.
    pub soft_cost: u64,
    pub soft_total: u64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EdgeStyle {
    /// A straight hierarchy line (radial).
    Line,
    /// A straight directed arrow (layered).
    Arrow,
    /// A dashed curved cross link.
    Link,
}

pub struct Drawn {
    pub a: usize,
    pub b: usize,
    pub style: EdgeStyle,
    /// Stroke width in ems of the body font.
    pub width: f64,
    pub label: Option<String>,
}

pub struct Layout {
    pub kind: LayoutKind,
    pub body: f64,
    pub boxes: Vec<Boxed>,
    /// Node centers; origin at page center, y up.
    pub pos: Vec<P>,
    pub edges: Vec<Drawn>,
    pub kx: f64,
    pub ky: f64,
    /// Slack allowed when checking alignments, since candidate spots are discrete.
    pub tol: f64,
    pub stats: Stats,
}

impl Layout {
    pub fn rect(&self, v: usize) -> Rect {
        Rect::centered(self.pos[v], self.boxes[v].w, self.boxes[v].h)
    }

    /// radius is the stretched polar radius used for the "general at center" ordering.
    pub fn radius(&self, v: usize) -> f64 {
        (self.pos[v].0 / self.kx).hypot(self.pos[v].1 / self.ky)
    }

    pub fn inside(&self, v: usize, p: P) -> bool {
        let b = &self.boxes[v];
        match b.shape {
            Shape::Circle => (p.0 - self.pos[v].0).hypot(p.1 - self.pos[v].1) <= b.w / 2.0,
            Shape::Rect => self.rect(v).contains(p),
        }
    }

    /// link_path routes a cross link: the first bow (away from center first) that clears all nodes.
    pub fn link_path(&self, a: usize, b: usize) -> Vec<P> {
        let clear = |pts: &[P]| (0..self.pos.len()).all(|u| u == a || u == b || pts.iter().all(|&p| !self.inside(u, p)));
        let mut first = None;
        for bow in [0.22, -0.22, 0.4, -0.4, 0.1, -0.1, 0.6, -0.6] {
            let pts = self.curve(a, b, bow);
            if clear(&pts) {
                return pts;
            }
            first.get_or_insert(pts);
        }
        first.unwrap()
    }

    fn curve(&self, a: usize, b: usize, bow: f64) -> Vec<P> {
        let (pa, pb) = (self.pos[a], self.pos[b]);
        let m = ((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0);
        let d = (pb.0 - pa.0).hypot(pb.1 - pa.1).max(1e-9);
        let mut n = (-(pb.1 - pa.1) / d, (pb.0 - pa.0) / d);
        if n.0 * m.0 + n.1 * m.1 < 0.0 {
            n = (-n.0, -n.1);
        }
        let c = (m.0 + bow * d * n.0, m.1 + bow * d * n.1);
        let pts: Vec<P> = (0..=60)
            .map(|i| {
                let t = i as f64 / 60.0;
                let (u, w) = ((1.0 - t) * (1.0 - t), 2.0 * (1.0 - t) * t);
                (u * pa.0 + w * c.0 + t * t * pb.0, u * pa.1 + w * c.1 + t * t * pb.1)
            })
            .collect();
        let s = pts.iter().position(|&p| !self.inside(a, p)).unwrap_or(0);
        let e = pts.iter().rposition(|&p| !self.inside(b, p)).unwrap_or(pts.len() - 1).max(s);
        pts[s..=e].to_vec()
    }
}

fn words(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth, mut prev) = (Vec::new(), String::new(), 0, ' ');
    for c in s.chars() {
        if c == '{' && (prev == '^' || prev == '_') {
            depth += 1;
        } else if c == '}' && depth > 0 {
            depth -= 1;
        }
        if c == ' ' && depth == 0 {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
        prev = c;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn wrap(fonts: &Fonts, text: &str, size: f64, bold: bool, maxw: f64) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for w in words(text) {
        let cand = if line.is_empty() { w.clone() } else { format!("{line} {w}") };
        if line.is_empty() || fonts.width(&cand, size, bold) <= maxw {
            line = cand;
        } else {
            out.push(std::mem::replace(&mut line, w));
        }
    }
    if !line.is_empty() || out.is_empty() {
        out.push(line);
    }
    out
}

pub fn line_height(size: f64) -> f64 {
    1.22 * size
}

pub fn measure(fonts: &Fonts, g: &Graph, v: usize, role: Role, b: f64, cfg: &Config) -> Boxed {
    let (ts, bs, wrap_w, shape, pad) = match role {
        Role::Root => (1.73 * b, 0.95 * b, 16.0 * b, Shape::Circle, 0.35 * b),
        Role::Hub => (1.44 * b, 0.9 * b, 11.0 * b, Shape::Circle, 0.35 * b),
        Role::Leaf => (1.2 * b, b, cfg.wrap_em * b, Shape::Rect, 0.5 * b),
    };
    let node = &g.nodes[v];
    let shape = node.shape.unwrap_or(shape);
    let mut raw: Vec<(String, f64, bool)> = Vec::new();
    for part in node.title.split('\n') {
        for l in wrap(fonts, part, ts, true, wrap_w) {
            raw.push((l, ts, true));
        }
    }
    let ntitle = raw.len();
    for part in &node.body {
        for l in wrap(fonts, part, bs, false, wrap_w) {
            raw.push((l, bs, false));
        }
    }
    let title_gap = if raw.len() > ntitle { 0.25 * b } else { 0.0 };
    let mut lines = Vec::new();
    let mut top = 0.0;
    for (i, (text, size, bold)) in raw.into_iter().enumerate() {
        if i == ntitle {
            top += title_gap;
        }
        let runs = fonts.runs(&text, bold);
        let width = fonts.runs_width(&runs, size, bold);
        lines.push(TextLine { runs, size, bold, width, x: 0.0, baseline: top + 0.96 * size });
        top += line_height(size);
    }
    let text_h = top;
    match shape {
        Shape::Rect => {
            let w = lines.iter().map(|l| l.width).fold(0.0, f64::max) + 2.0 * pad;
            for l in &mut lines {
                l.x = pad;
                l.baseline += pad;
            }
            Boxed { shape, role, w, h: text_h + 2.0 * pad, pad, lines }
        }
        Shape::Circle => {
            // Radius from the farthest line corner, so text always fits inside the circle.
            let mut r: f64 = 0.0;
            for l in &lines {
                let top_y = l.baseline - 0.96 * l.size - text_h / 2.0;
                let far = top_y.abs().max((top_y + line_height(l.size)).abs());
                r = r.max((l.width / 2.0).hypot(far));
            }
            let r = r + pad;
            for l in &mut lines {
                l.x = r - l.width / 2.0;
                l.baseline += r - text_h / 2.0;
            }
            Boxed { shape, role, w: 2.0 * r, h: 2.0 * r, pad, lines }
        }
    }
}

/// Cand is one spot a node may take: its preference cost (lower is better), its center, and
/// a strategy-specific coordinate (the radial step index for `Outward`).
pub struct Cand {
    pub cost: f64,
    pub at: P,
    pub r: f64,
}

#[derive(Clone, Copy)]
pub enum Kind {
    /// Two regions stay this far apart.
    Apart(f64),
    /// Scope 1 sits at least this much farther out (in `Cand::r`) than scope 0.
    Outward(f64),
    /// The segment between scopes 0 and 1 misses scope 2's region grown by this much.
    Clear(f64),
    /// Segments 0-1 and 2-3 do not cross.
    NoCross,
    /// A user constraint, by index into `Graph::constraints`, over all its nodes.
    Rel(usize),
    /// Two nodes of a hard alignment; pairs propagate better than one wide constraint.
    Aligned(Side),
}

/// Plan is what a strategy hands to the core: candidates per node and constraints over nodes.
pub struct Plan {
    /// Variable order: static search assigns nodes in this order.
    pub order: Vec<usize>,
    pub cands: Vec<Vec<Cand>>,
    /// (nodes, kind, weight); weight 0 is hard.
    pub cons: Vec<(Vec<usize>, Kind, u64)>,
    pub edges: Vec<Drawn>,
    pub kx: f64,
    pub ky: f64,
    pub tol: f64,
}

fn aligned(side: Side, a: &Rect, b: &Rect, tol: f64) -> bool {
    let d = match side {
        Side::Top => a.y1 - b.y1,
        Side::Bottom => a.y0 - b.y0,
        Side::Left => a.x0 - b.x0,
        Side::Right => a.x1 - b.x1,
        Side::Row => (a.y0 + a.y1 - b.y0 - b.y1) / 2.0,
        Side::Column => (a.x0 + a.x1 - b.x0 - b.x1) / 2.0,
    };
    d.abs() <= tol + 1e-9
}

/// relation_holds is the meaning of a user constraint, shared by the solver and the checker.
/// `rect` gives a node's region; y grows upward.
pub fn relation_holds(rel: &Relation, rect: &dyn Fn(usize) -> Rect, body: f64, page: &Rect, tol: f64) -> bool {
    match rel {
        Relation::LeftOf { a, b, gap } => rect(*a).x1 + gap.pt(body) <= rect(*b).x0 + 1e-9,
        Relation::Above { a, b, gap } => rect(*a).y0 >= rect(*b).y1 + gap.pt(body) - 1e-9,
        Relation::Near { a, b, within } => {
            let (ra, rb) = (rect(*a), rect(*b));
            let dx = (ra.x0 - rb.x1).max(rb.x0 - ra.x1).max(0.0);
            let dy = (ra.y0 - rb.y1).max(rb.y0 - ra.y1).max(0.0);
            dx.hypot(dy) <= within.pt(body) + 1e-9
        }
        Relation::Align { side, items } => items.windows(2).all(|w| aligned(*side, &rect(w[0]), &rect(w[1]), tol)),
        Relation::At { a, place } => {
            let r = rect(*a);
            let (cx, cy) = ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
            let (w, h) = ((page.x1 - page.x0) / 3.0, (page.y1 - page.y0) / 3.0);
            let (x0, y1) = (page.x0 + w * place.0 as f64, page.y1 - h * place.1 as f64);
            cx >= x0 - 1e-9 && cx <= x0 + w + 1e-9 && cy <= y1 + 1e-9 && cy >= y1 - h - 1e-9
        }
    }
}

/// Grid buckets one variable's candidate centers. Every candidate of a node has the same box,
/// so "its box meets region R" is "its center lies in R grown by half the box": a range query.
struct Grid {
    cell: f64,
    buckets: HashMap<(i64, i64), Vec<u32>>,
}

impl Grid {
    fn new(at: &[P]) -> Grid {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in at {
            (x0, y0, x1, y1) = (x0.min(p.0), y0.min(p.1), x1.max(p.0), y1.max(p.1));
        }
        // About sixteen candidates per bucket.
        let cell = (((x1 - x0) * (y1 - y0)).max(1.0) * 16.0 / at.len().max(1) as f64).sqrt().max(1.0);
        let mut buckets: HashMap<(i64, i64), Vec<u32>> = HashMap::new();
        for (i, p) in at.iter().enumerate() {
            buckets.entry(((p.0 / cell).floor() as i64, (p.1 / cell).floor() as i64)).or_default().push(i as u32);
        }
        Grid { cell, buckets }
    }

    /// near calls `f` with every candidate whose center may lie in `r` (and some that do not).
    fn near(&self, r: &Rect, f: &mut dyn FnMut(u32)) -> u64 {
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
struct Scene<'a> {
    g: &'a Graph,
    page: Rect,
    body: f64,
    tol: f64,
    /// Node of each variable, and the variable of each node.
    node: Vec<usize>,
    var: Vec<usize>,
    at: Vec<Vec<P>>,
    r: Vec<Vec<f64>>,
    rects: Vec<Vec<Rect>>,
    scopes: Vec<Vec<usize>>,
    kinds: Vec<Kind>,
    weights: Vec<u64>,
    /// Candidate box size per variable, and a spatial index when the fast path is on.
    size: Vec<(f64, f64)>,
    grids: Vec<Grid>,
}

impl Problem for Scene<'_> {
    fn num_vars(&self) -> usize {
        self.node.len()
    }

    fn domain_size(&self, v: usize) -> usize {
        self.at[v].len()
    }

    fn scopes(&self) -> &[Vec<usize>] {
        &self.scopes
    }

    fn weight(&self, c: usize) -> u64 {
        self.weights[c]
    }

    fn filter(&self, c: usize, val: &[u32], y: usize, bad: &mut dyn FnMut(usize)) -> Option<u64> {
        if self.grids.is_empty() {
            return None;
        }
        let s = &self.scopes[c];
        let (w, h) = self.size[y];
        let mut test = |region: Rect, hit: &dyn Fn(usize) -> bool| {
            let grown = Rect { x0: region.x0 - w / 2.0, y0: region.y0 - h / 2.0, x1: region.x1 + w / 2.0, y1: region.y1 + h / 2.0 };
            self.grids[y].near(&grown, &mut |a| {
                if hit(a as usize) {
                    bad(a as usize);
                }
            })
        };
        match self.kinds[c] {
            Kind::Apart(gap) => {
                let other = if s[0] == y { s[1] } else { s[0] };
                let r = self.rects[other][val[other] as usize].inflate(gap);
                Some(test(r, &|a| r.intersects(&self.rects[y][a])))
            }
            Kind::Clear(thin) if s[2] == y => {
                let (p, q) = (self.at[s[0]][val[s[0]] as usize], self.at[s[1]][val[s[1]] as usize]);
                let span = Rect { x0: p.0.min(q.0) - thin, y0: p.1.min(q.1) - thin, x1: p.0.max(q.0) + thin, y1: p.1.max(q.1) + thin };
                Some(test(span, &|a| segment_hits_rect(p, q, &self.rects[y][a].inflate(thin))))
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
        }
    }
}

/// Which user constraints an attempt includes.
#[derive(Clone, Copy)]
enum Users<'a> {
    /// These hard constraints only (indices into `Graph::constraints`).
    Hard(&'a [usize]),
    /// Every hard constraint plus every soft one.
    All,
}

fn attempt(g: &Graph, t: &Tree, fonts: &Fonts, cfg: &Config, b: f64, users: Users, stats: &mut Stats) -> Option<Layout> {
    stats.attempts += 1;
    let kind = g.settings.layout.unwrap_or(LayoutKind::Radial);
    let n = g.nodes.len();
    let boxes: Vec<Boxed> = match kind {
        LayoutKind::Radial => (0..n).map(|v| measure(fonts, g, v, radial::role(t, v), b, cfg)).collect(),
        LayoutKind::Layered(_) => (0..n).map(|v| measure(fonts, g, v, Role::Leaf, b, cfg)).collect(),
    };
    let plan = match kind {
        LayoutKind::Radial => radial::plan(t, &boxes, cfg, b),
        LayoutKind::Layered(dir) => layered::plan(g, &boxes, cfg, b, dir),
    };
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
        grids: Vec::new(),
    };
    for &v in &plan.order {
        let c = &plan.cands[v];
        sc.size.push((boxes[v].w, boxes[v].h));
        sc.at.push(c.iter().map(|c| c.at).collect());
        sc.r.push(c.iter().map(|c| c.r).collect());
        sc.rects.push(c.iter().map(|c| Rect::centered(c.at, boxes[v].w, boxes[v].h)).collect());
    }
    let mut cons = plan.cons;
    for (ci, c) in g.constraints.iter().enumerate() {
        let on = match users {
            Users::Hard(set) => c.hard && set.contains(&ci),
            Users::All => true,
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
    let optimizing = matches!(users, Users::All);
    for (nodes, kind, w) in cons {
        if w > 0 && !optimizing {
            continue;
        }
        sc.scopes.push(nodes.iter().map(|&u| sc.var[u]).collect());
        sc.kinds.push(kind);
        sc.weights.push(w);
    }
    let (out, cost, st) = if optimizing {
        csp::optimize(&sc, cfg.opt_budget)
    } else {
        let (out, st) = csp::solve(&sc, cfg.budget);
        (out, 0, st)
    };
    stats.search += st;
    if std::env::var_os("CMAPC_DEBUG").is_some() {
        let what = match out {
            Outcome::Sat(_) => "sat",
            Outcome::Unsat => "unsat",
            Outcome::Unknown => "budget",
        };
        eprintln!("font {b:.2}{}: {what} after {st:?}", if optimizing { " (optimizing)" } else { "" });
    }
    let Outcome::Sat(val) = out else { return None };
    if optimizing {
        stats.soft_cost = cost;
        stats.soft_total = sc.weights.iter().sum();
    }
    let mut pos = vec![(0.0, 0.0); n];
    for (i, &v) in sc.node.iter().enumerate() {
        pos[v] = sc.at[i][val[i] as usize];
    }
    Some(Layout { kind, body: b, boxes, pos, edges: plan.edges, kx: plan.kx, ky: plan.ky, tol: plan.tol, stats: *stats })
}

pub enum Failure {
    /// Nothing fits even without user constraints.
    NoRoom,
    /// These hard user constraints cannot all hold at the minimum font; removing any one of
    /// them makes the rest satisfiable (within the search budget).
    Core(Vec<usize>),
}

/// quickxplain returns a minimal subset of `cs` that is infeasible together with `base`
/// (Junker 2004). It needs `base` feasible and `base + cs` infeasible.
fn quickxplain(feasible: &mut dyn FnMut(&[usize]) -> bool, base: &[usize], added: bool, cs: &[usize]) -> Vec<usize> {
    if added && !feasible(base) {
        return Vec::new();
    }
    if cs.len() == 1 {
        return cs.to_vec();
    }
    let (c1, c2) = cs.split_at(cs.len() / 2);
    let d2 = quickxplain(feasible, &[base, c1].concat(), !c1.is_empty(), c2);
    let d1 = quickxplain(feasible, &[base, &d2].concat(), !d2.is_empty(), c1);
    [d1, d2].concat()
}

/// solve binary-searches the largest body font for which the hard constraints are
/// satisfiable, then optimizes soft constraints at that font.
pub fn solve(g: &Graph, t: &Tree, fonts: &Fonts, cfg: &Config) -> Result<Layout, Failure> {
    let mut stats = Stats::default();
    let hard: Vec<usize> = (0..g.constraints.len()).filter(|&i| g.constraints[i].hard).collect();
    let probe = |b: f64, set: &[usize], budget: u64, stats: &mut Stats| attempt(g, t, fonts, &Config { budget, ..*cfg }, b, Users::Hard(set), stats);

    // Near the largest font that fits, a probe either succeeds quickly or runs until the
    // budget is gone. So once a probe succeeds, later probes get a few times its cost.
    let (mut lo, mut hi) = (cfg.min_font, cfg.max_font);
    let (mut found, mut next, mut cap) = (None, hi, cfg.budget / 4);
    loop {
        let before = stats.search.checks;
        match probe(next, &hard, cap, &mut stats) {
            Some(l) => {
                cap = ((stats.search.checks - before) * 4).clamp(1_000_000, cfg.budget);
                lo = next;
                found = Some(l);
            }
            None => hi = next,
        }
        // One percent of the font size is below what anyone can see.
        if hi - lo <= 0.01 * lo {
            break;
        }
        next = (lo + hi) / 2.0;
    }
    let mut best = match found.or_else(|| probe(cfg.min_font, &hard, cfg.budget, &mut stats)) {
        Some(l) => l,
        None => {
            if hard.is_empty() || probe(cfg.min_font, &[], cfg.budget, &mut stats).is_none() {
                return Err(Failure::NoRoom);
            }
            let mut feasible = |set: &[usize]| probe(cfg.min_font, set, cfg.budget, &mut stats).is_some();
            let mut core = quickxplain(&mut feasible, &[], false, &hard);
            core.sort();
            return Err(Failure::Core(core));
        }
    };
    let has_soft = g.constraints.iter().any(|c| !c.hard) || matches!(best.kind, LayoutKind::Layered(_));
    if has_soft {
        if let Some(l) = attempt(g, t, fonts, cfg, best.body, Users::All, &mut stats) {
            best = l;
        }
    }
    best.stats = stats;
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Settings;

    /// scene builds random candidates for `n` boxes with every `Apart` pair and some `Clear`s.
    fn scene(g: &Graph, n: usize, seed: u64) -> Scene<'_> {
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
            grids: Vec::new(),
        };
        for _ in 0..n {
            let (w, h) = (60.0 + rnd().abs() / 5.0, 30.0 + rnd().abs() / 10.0);
            let at: Vec<P> = (0..300).map(|_| (rnd(), rnd())).collect();
            sc.rects.push(at.iter().map(|&p| Rect::centered(p, w, h)).collect());
            sc.r.push(vec![0.0; at.len()]);
            sc.at.push(at);
            sc.size.push((w, h));
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
        }
        sc
    }

    #[test]
    fn grid_filter_matches_plain_checks() {
        let g = Graph { title: None, root: None, nodes: vec![], edges: vec![], constraints: vec![], settings: Settings::default() };
        for seed in 1..40 {
            let mut sc = scene(&g, 7, seed * 7919);
            let (plain, ps) = csp::solve(&sc, u64::MAX);
            sc.grids = sc.at.iter().map(|a| Grid::new(a)).collect();
            let (fast, fs) = csp::solve(&sc, u64::MAX);
            assert_eq!(plain, fast, "seed {seed}");
            assert_eq!((ps.nodes, ps.backjumps), (fs.nodes, fs.backjumps), "seed {seed}");
        }
    }

    #[test]
    fn quickxplain_finds_minimal_core() {
        // Infeasible exactly when the set contains both 2 and 5, or contains 7.
        let mut calls = 0;
        let mut feasible = |s: &[usize]| {
            calls += 1;
            !(s.contains(&2) && s.contains(&5)) && !s.contains(&7)
        };
        let mut core = quickxplain(&mut feasible, &[], false, &[0, 1, 2, 3, 4, 5, 6]);
        core.sort();
        assert_eq!(core, vec![2, 5]);
        assert!(calls < 20, "{calls} oracle calls");
    }
}
