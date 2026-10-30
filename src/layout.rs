//! Layout core shared by every strategy: text measurement, the scene CSP, and the outer
//! search. A strategy (radial, layered) proposes candidate spots per node plus its own
//! structural constraints; the user's `require`/`prefer` constraints are added on top. The
//! outer search finds the largest body font that is satisfiable, then spends a second budget
//! on soft preferences. When even the smallest font fails, it extracts a minimal set of
//! conflicting user constraints (QuickXplain).

use crate::csp::{self, Outcome, Problem};
use crate::geom::{P, Rect, segment_hits_rect, segments_cross};
pub use crate::graph::Shape;
use crate::graph::{Dir, Graph, LayoutKind, Relation, Side};
use crate::metrics::{Fonts, Run};
use crate::quality::{self, Score};
use crate::tree::Tree;
use crate::{layered, radial};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

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
    /// Smallest body font readable at the page's viewing distance; also the length unit of
    /// readability scores.
    pub legible: f64,
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
    /// Checks the winning plan of the latest attempt spent.
    pub probe_checks: u64,
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
    /// Bend of a link (see `curve`) and the label's position along the path, 0 to 1. The
    /// solver picks both.
    pub bow: f64,
    pub t: f64,
    /// Label box size in points, set once the font is known.
    pub label_size: (f64, f64),
}

impl Drawn {
    pub fn new(a: usize, b: usize, style: EdgeStyle, width: f64, label: Option<String>) -> Drawn {
        Drawn { a, b, style, width, label, bow: 0.0, t: 0.5, label_size: (0.0, 0.0) }
    }
}

/// Bends tried for dashed links, preferred first, and label positions along any edge.
const BOWS: [f64; 8] = [0.22, -0.22, 0.4, -0.4, 0.1, -0.1, 0.6, -0.6];
const SPOTS: [f64; 9] = [0.5, 0.42, 0.58, 0.34, 0.66, 0.26, 0.74, 0.18, 0.82];

pub fn label_size(fonts: &Fonts, text: &str, body: f64) -> (f64, f64) {
    (fonts.width(text, 0.8 * body, false) + 0.4 * body, 1.04 * body)
}

fn inside_box(c: P, shape: Shape, w: f64, h: f64, p: P) -> bool {
    match shape {
        Shape::Circle => (p.0 - c.0).hypot(p.1 - c.1) <= w / 2.0,
        Shape::Rect => Rect::centered(c, w, h).contains(p),
    }
}

/// curve is the path of an edge: a quadratic Bezier from `pa` to `pb` bent by `bow` times its
/// length away from the page center (0 is straight), trimmed to the part outside both boxes.
pub fn curve(pa: P, in_a: &dyn Fn(P) -> bool, pb: P, in_b: &dyn Fn(P) -> bool, bow: f64) -> Vec<P> {
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
    let s = pts.iter().position(|&p| !in_a(p)).unwrap_or(0);
    let e = pts.iter().rposition(|&p| !in_b(p)).unwrap_or(pts.len() - 1).max(s);
    pts[s..=e].to_vec()
}

/// label_rect is the box of a label centered at fraction `t` of a path.
pub fn label_rect(path: &[P], t: f64, size: (f64, f64)) -> Rect {
    let i = ((path.len() - 1) as f64 * t).round() as usize;
    Rect::centered(path[i], size.0, size.1)
}

/// paths_meet reports whether two paths cross or run within `d` of each other, ignoring the
/// parts inside `skip` (around shared endpoints, where links naturally meet).
pub fn paths_meet(p: &[P], q: &[P], d: f64, skip: &[Rect]) -> bool {
    let free = |x: P| skip.iter().all(|r| !r.contains(x));
    for i in 0..p.len() {
        if !free(p[i]) {
            continue;
        }
        for j in 0..q.len() {
            if !free(q[j]) {
                continue;
            }
            if (p[i].0 - q[j].0).hypot(p[i].1 - q[j].1) < d {
                return true;
            }
            if i + 1 < p.len() && j + 1 < q.len() && free(p[i + 1]) && free(q[j + 1]) && segments_cross(p[i], p[i + 1], q[j], q[j + 1]) {
                return true;
            }
        }
    }
    false
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
    /// Radial branch rotation in degrees, so later passes rebuild the same plan.
    pub rot: f64,
    /// Fonts tried by the final trade-off with each one's merit (lower is better).
    pub tradeoff: Vec<(f64, f64)>,
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
        inside_box(self.pos[v], self.boxes[v].shape, self.boxes[v].w, self.boxes[v].h, p)
    }

    /// path is an edge's drawn route with its chosen bend, trimmed to the node borders.
    pub fn path(&self, e: &Drawn) -> Vec<P> {
        curve(self.pos[e.a], &|p| self.inside(e.a, p), self.pos[e.b], &|p| self.inside(e.b, p), e.bow)
    }

    pub fn label_rect(&self, e: &Drawn) -> Option<Rect> {
        e.label.as_ref().map(|_| label_rect(&self.path(e), e.t, e.label_size))
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
    /// Route `0`'s label stays off node `1`.
    LabelClear(usize, usize),
    /// Route's label stays on the page.
    LabelPage(usize),
    /// Two routes' labels do not overlap.
    LabelApart(usize, usize),
    /// Two dashed links neither cross nor run close together (soft).
    LinkApart(usize, usize),
    /// A dashed link stays out of node `1` (soft).
    LinkClear(usize, usize),
}

/// Route is an edge whose bend or label position the solver chooses. Its variable comes after
/// every node's, so all positions are known when its values are checked.
struct Route {
    edge: usize,
    a: usize,
    b: usize,
    /// (bow, t) pairs in preference order.
    opts: Vec<(f64, f64)>,
    label: Option<(f64, f64)>,
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
    // Ordered, so candidates come out in the same order every run and `polish` is reproducible.
    buckets: BTreeMap<(i64, i64), Vec<u32>>,
}

impl Grid {
    fn new(at: &[P]) -> Grid {
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
    fn shadow(&self, apex: P, pts: &[P], f: &mut dyn FnMut(u32)) -> Option<u64> {
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
    /// Candidate box size and shape per node variable, and a spatial index over its candidates.
    size: Vec<(f64, f64)>,
    shapes: Vec<Shape>,
    grids: Vec<Grid>,
    routes: Vec<Route>,
}

impl Scene<'_> {
    fn center(&self, u: usize, val: &[u32]) -> P {
        self.at[self.var[u]][val[self.var[u]] as usize]
    }

    fn node_rect(&self, u: usize, val: &[u32]) -> Rect {
        self.rects[self.var[u]][val[self.var[u]] as usize]
    }

    fn route_path(&self, i: usize, val: &[u32]) -> Vec<P> {
        let r = &self.routes[i];
        let (bow, _) = r.opts[val[self.node.len() + i] as usize];
        let inside = |u: usize, p: P| {
            let (w, h) = self.size[self.var[u]];
            inside_box(self.center(u, val), self.shapes[self.var[u]], w, h, p)
        };
        curve(self.center(r.a, val), &|p| inside(r.a, p), self.center(r.b, val), &|p| inside(r.b, p), bow)
    }

    fn route_label(&self, i: usize, val: &[u32]) -> Rect {
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

/// Which user constraints an attempt includes.
#[derive(Clone, Copy)]
enum Users<'a> {
    /// These hard constraints only (indices into `Graph::constraints`).
    Hard(&'a [usize]),
    /// Every hard constraint plus `prefer`s and layered crossings, on this layout's plan and
    /// trying its spots first.
    All(&'a Layout),
    /// Nodes fixed where this layout put them; only link routes are optimized.
    Routes(&'a Layout),
    /// Every hard constraint at a smaller font than this layout's, trying its spots first:
    /// boxes shrink around the same centers, so they usually still fit.
    Shrunk(&'a Layout),
}

/// ROTATIONS turns the radial branches, in degrees. Near the largest font that fits, which
/// rotation solves is close to chance, so each probe tries all of them in parallel.
const ROTATIONS: [f64; 4] = [0.0, 10.0, 30.0, 45.0];

/// attempt places every node at body font `b`. A radial probe tries each of ROTATIONS and
/// keeps the first in list order that solves, so the result does not depend on timing.
fn attempt(g: &Graph, t: &Tree, fonts: &Fonts, cfg: &Config, b: f64, users: Users, stats: &mut Stats) -> Option<Layout> {
    stats.attempts += 1;
    let kind = g.settings.layout.unwrap_or(LayoutKind::Radial);
    let n = g.nodes.len();
    let boxes: Vec<Boxed> = match kind {
        LayoutKind::Radial => (0..n).map(|v| measure(fonts, g, v, radial::role(t, v), b, cfg)).collect(),
        LayoutKind::Layered(_) => (0..n).map(|v| measure(fonts, g, v, Role::Leaf, b, cfg)).collect(),
    };
    let rots = match (kind, users) {
        (_, Users::All(prev) | Users::Routes(prev) | Users::Shrunk(prev)) => vec![prev.rot],
        (LayoutKind::Radial, Users::Hard(_)) => ROTATIONS.to_vec(),
        (LayoutKind::Layered(_), Users::Hard(_)) => vec![0.0],
    };
    let built: Vec<_> = rots.iter().map(|&rot| build(g, t, fonts, cfg, b, users, kind, &boxes, rot)).collect();
    let optimizing = matches!(users, Users::All(_));
    let routing = matches!(users, Users::Routes(_));
    // Once a plan solves, the plans after it in list order cannot win, so they stop.
    let stop: Vec<AtomicBool> = built.iter().map(|_| AtomicBool::new(false)).collect();
    let runs: Vec<(Outcome, u64, csp::Stats)> = std::thread::scope(|s| {
        let stop = &stop;
        let handles: Vec<_> = built
            .iter()
            .enumerate()
            .map(|(i, (sc, ..))| {
                s.spawn(move || {
                    let run = if optimizing || routing {
                        csp::optimize(sc, cfg.opt_budget, &stop[i])
                    } else {
                        let (out, st) = csp::solve(sc, cfg.budget, &stop[i]);
                        (out, 0, st)
                    };
                    if matches!(run.0, Outcome::Sat(_)) {
                        stop[i + 1..].iter().for_each(|f| f.store(true, Ordering::Relaxed));
                    }
                    run
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("solver thread")).collect()
    });
    for (i, (out, _, st)) in runs.iter().enumerate() {
        stats.search += *st;
        if std::env::var_os("CMAPC_DEBUG").is_some() {
            let what = match out {
                Outcome::Sat(_) => "sat",
                Outcome::Unsat => "unsat",
                Outcome::Unknown => "budget",
            };
            eprintln!("font {b:.2}{} rot {}: {what} after {st:?}", if optimizing { " (optimizing)" } else { "" }, rots[i]);
        }
    }
    let win = runs.iter().position(|(out, ..)| matches!(out, Outcome::Sat(_)))?;
    let (Outcome::Sat(val), cost, st) = &runs[win] else { unreachable!() };
    let (sc, mut edges, plan) = built.into_iter().nth(win).unwrap();
    stats.probe_checks = st.checks;
    let mut val = val.clone();
    let mut cost = *cost;
    if optimizing {
        let ctx = Readable::new(t, kind, &edges, sc.page, cfg.legible);
        cost = polish(&sc, &ctx, &mut val, POLISH_MOVES);
    }
    if optimizing || routing {
        stats.soft_cost += cost;
        stats.soft_total += sc.weights.iter().sum::<u64>();
    }
    let mut pos = vec![(0.0, 0.0); n];
    for (i, &v) in sc.node.iter().enumerate() {
        pos[v] = sc.at[i][val[i] as usize];
    }
    let base = sc.node.len();
    for (i, r) in sc.routes.iter().enumerate() {
        (edges[r.edge].bow, edges[r.edge].t) = r.opts[val[base + i] as usize];
    }
    Some(Layout { kind, body: b, boxes, pos, edges, kx: plan.0, ky: plan.1, tol: plan.2, rot: rots[win], tradeoff: Vec::new(), stats: *stats })
}

/// build turns one plan into a CSP scene, plus the plan's edges and its (kx, ky, tol).
#[allow(clippy::too_many_arguments)]
fn build<'a>(g: &'a Graph, t: &Tree, fonts: &Fonts, cfg: &Config, b: f64, users: Users, kind: LayoutKind, boxes: &[Boxed], rot: f64) -> (Scene<'a>, Vec<Drawn>, (f64, f64, f64)) {
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
        sc.routes.push(Route { edge: k, a: e.a, b: e.b, opts, label });
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
    }
    (sc, edges, (plan.kx, plan.ky, plan.tol))
}

/// Readable holds what `quality::score` needs that stays fixed while nodes move.
pub struct Readable {
    pub tree: Vec<(usize, usize)>,
    pub links: Vec<(usize, usize)>,
    pub branch: Vec<Option<usize>>,
    pub flow: Option<bool>,
    pub page: Rect,
    pub unit: f64,
}

impl Readable {
    pub fn new(t: &Tree, kind: LayoutKind, edges: &[Drawn], page: Rect, unit: f64) -> Readable {
        let pairs = |link: bool| edges.iter().filter(|e| (e.style == EdgeStyle::Link) == link).map(|e| (e.a, e.b)).collect();
        // Branch grouping only means something when branches own directions, as in radial.
        let branch = (0..t.parent.len()).map(|v| if kind == LayoutKind::Radial { t.branch(v) } else { None }).collect();
        let flow = match kind {
            LayoutKind::Radial => None,
            LayoutKind::Layered(dir) => Some(dir == Dir::Down),
        };
        Readable { tree: pairs(false), links: pairs(true), branch, flow, page, unit }
    }

    pub fn score(&self, rects: &[Rect]) -> Score {
        quality::score(&quality::Input { rects, tree: &self.tree, links: &self.links, branch: &self.branch, flow: self.flow, page: self.page, unit: self.unit })
    }
}

/// Moves tried by `polish`, and how far one move may go, in ems.
const POLISH_MOVES: usize = 150_000;
const POLISH_REACH: f64 = 10.0;
/// Readability penalty per unit of unmet `prefer` weight; high, since a `prefer` is the
/// author's stated intent and the aesthetics are only general guidance.
const SOFT_PENALTY: f64 = 1000.0;

/// polish moves one node at a time to a nearby candidate spot, keeping every hard constraint,
/// and anneals on readability plus unmet `prefer` weight. The walk is seeded, so the result is
/// deterministic. Returns the final soft cost.
fn polish(sc: &Scene, ctx: &Readable, val: &mut [u32], moves: usize) -> u64 {
    let nv = sc.node.len();
    let mut hard = vec![Vec::new(); nv];
    let mut soft = vec![Vec::new(); nv];
    for (c, scope) in sc.scopes.iter().enumerate() {
        for &v in scope.iter().filter(|&&v| v < nv) {
            if sc.weights[c] == 0 { hard[v].push(c) } else { soft[v].push(c) }
        }
    }
    let unmet = |cs: &[usize], val: &[u32]| cs.iter().filter(|&&c| !sc.holds(c, val)).map(|&c| sc.weights[c]).sum::<u64>();
    let mut rects: Vec<Rect> = (0..nv).map(|v| sc.node_rect(v, val)).collect();
    let mut soft_cost: u64 = (0..sc.scopes.len()).filter(|&c| sc.weights[c] > 0 && !sc.holds(c, val)).map(|c| sc.weights[c]).sum();
    let energy = |rects: &[Rect], soft: u64| ctx.score(rects).total() + SOFT_PENALTY * soft as f64;
    let mut cur = energy(&rects, soft_cost);
    let (mut best, mut best_val, mut best_soft) = (cur, val.to_vec(), soft_cost);
    let movable: Vec<usize> = (0..nv).filter(|&i| sc.at[i].len() > 1).collect();
    if movable.is_empty() {
        return soft_cost;
    }
    let mut rng: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = |k: usize| {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng % k as u64) as usize
    };
    // Temperature falls geometrically from about one em of edge to almost nothing.
    let (t0, t1): (f64, f64) = (2.0, 0.01);
    let mut near = Vec::new();
    for k in 0..moves {
        let temp = t0 * (t1 / t0).powf(k as f64 / moves as f64);
        let i = movable[next(movable.len())];
        let u = sc.node[i];
        let reach = POLISH_REACH * sc.body;
        near.clear();
        sc.grids[i].near(&Rect::centered(sc.at[i][val[i] as usize], 2.0 * reach, 2.0 * reach), &mut |a| near.push(a));
        if near.is_empty() {
            continue;
        }
        let old = val[i];
        let a = near[next(near.len())];
        if a == old {
            continue;
        }
        let before = unmet(&soft[i], val);
        val[i] = a;
        if !hard[i].iter().all(|&c| sc.holds(c, val)) {
            val[i] = old;
            continue;
        }
        let new_soft = soft_cost - before + unmet(&soft[i], val);
        let old_rect = rects[u];
        rects[u] = sc.rects[i][a as usize];
        let e = energy(&rects, new_soft);
        let x = (next(1 << 20) as f64 + 0.5) / (1 << 20) as f64;
        if e <= cur || x < ((cur - e) / temp).exp() {
            cur = e;
            soft_cost = new_soft;
            if cur < best {
                (best, best_soft) = (cur, soft_cost);
                best_val.copy_from_slice(&val[..]);
            }
        } else {
            val[i] = old;
            rects[u] = old_rect;
        }
    }
    val.copy_from_slice(&best_val);
    best_soft
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

/// Fractions of the largest feasible font that the final trade-off polishes and compares.
const TRADE: [f64; 3] = [1.0, 0.95, 0.9];
/// Merit per unit of log font gain above the legibility floor: 5% more font is worth about
/// one crossing.
const W_FONT: f64 = 800.0;

/// merit scores a finished layout for the font trade-off, lower is better: its readability
/// penalty plus unmet `prefer` weight, minus a font gain with diminishing returns.
pub fn merit(g: &Graph, t: &Tree, cfg: &Config, lay: &Layout) -> f64 {
    let rects: Vec<Rect> = (0..lay.pos.len()).map(|v| lay.rect(v)).collect();
    let q = Readable::new(t, lay.kind, &lay.edges, cfg.page(), cfg.legible).score(&rects);
    let rect = |v: usize| lay.rect(v);
    let unmet: u64 = g.constraints.iter().filter(|c| !c.hard && !relation_holds(&c.rel, &rect, lay.body, &cfg.page(), lay.tol)).map(|c| c.weight as u64).sum();
    q.total() + SOFT_PENALTY * unmet as f64 - W_FONT * (lay.body / cfg.legible).ln()
}

/// solve binary-searches the largest body font for which the hard constraints are
/// satisfiable, then polishes that font and slightly smaller ones and keeps the best `merit`.
pub fn solve(g: &Graph, t: &Tree, fonts: &Fonts, cfg: &Config) -> Result<Layout, Failure> {
    let mut stats = Stats::default();
    let hard: Vec<usize> = (0..g.constraints.len()).filter(|&i| g.constraints[i].hard).collect();
    let probe = |b: f64, set: &[usize], budget: u64, stats: &mut Stats| attempt(g, t, fonts, &Config { budget, ..*cfg }, b, Users::Hard(set), stats);

    // Near the largest font that fits, a probe either succeeds quickly or runs until the
    // budget is gone. So once a probe succeeds, later probes get twice its cost.
    let (mut lo, mut hi) = (cfg.min_font, cfg.max_font);
    let (mut found, mut next, mut cap) = (None, hi, cfg.budget / 4);
    loop {
        match probe(next, &hard, cap, &mut stats) {
            Some(l) => {
                cap = (stats.probe_checks * 2).clamp(1_000_000, cfg.budget);
                lo = next;
                found = Some(l);
            }
            None => hi = next,
        }
        // The trade-off below steps down 5% at a time, so finer precision here is wasted.
        if hi - lo <= 0.03 * lo {
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
    // Then trade font against readability. Above the legibility floor a larger font helps
    // less and less, so a slightly smaller font may buy a clearer arrangement: polish a few
    // fonts and keep the best merit. Below the floor nothing smaller is tried.
    let top = best.body;
    let fonts_tried: Vec<f64> = TRADE.iter().map(|k| k * top).filter(|&f| f == top || (f >= cfg.legible && f >= cfg.min_font)).collect();
    // Each font is independent, so they run in parallel, each with its own stats.
    let runs: Vec<(f64, Option<(f64, Layout)>, Stats)> = std::thread::scope(|s| {
        let best = &best;
        let handles: Vec<_> = fonts_tried
            .iter()
            .map(|&f| {
                s.spawn(move || {
                    let mut st = Stats::default();
                    let probed;
                    let base = if f == top {
                        best
                    } else {
                        match attempt(g, t, fonts, &Config { budget: cap, ..*cfg }, f, Users::Shrunk(best), &mut st) {
                            Some(l) => {
                                probed = l;
                                &probed
                            }
                            None => return (f, None, st),
                        }
                    };
                    let polished = attempt(g, t, fonts, cfg, f, Users::All(base), &mut st).map(|l| (merit(g, t, cfg, &l), l));
                    (f, polished, st)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("trade-off thread")).collect()
    });
    let mut tradeoff = Vec::new();
    let mut picked: Option<(f64, Layout, Stats)> = None;
    for (f, polished, st) in runs {
        stats.attempts += st.attempts;
        stats.search += st.search;
        let Some((m, l)) = polished else { continue };
        tradeoff.push((f, m));
        if picked.as_ref().is_none_or(|p| m < p.0) {
            picked = Some((m, l, st));
        }
    }
    if let Some((_, l, st)) = picked {
        best = l;
        (stats.soft_cost, stats.soft_total) = (st.soft_cost, st.soft_total);
    }
    best.tradeoff = tradeoff;
    // Then, with nodes fixed, pick link routes: few variables, so the search is exhaustive.
    if best.edges.iter().any(|e| e.style == EdgeStyle::Link) {
        if let Some(mut l) = attempt(g, t, fonts, cfg, best.body, Users::Routes(&best), &mut stats) {
            l.tradeoff = std::mem::take(&mut best.tradeoff);
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
