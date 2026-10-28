//! Layout core shared by every strategy: text measurement, the scene CSP, and the outer
//! search. A strategy (radial, layered) proposes candidate spots per node plus its own
//! structural constraints; the user's `require`/`prefer` constraints are added on top. The
//! outer search finds the largest body font that is satisfiable, then spends a second budget
//! on soft preferences. When even the smallest font fails, it extracts a minimal set of
//! conflicting user constraints (QuickXplain).

use crate::csp::{self, Algo, Outcome, Problem};
use crate::geom::{P, Rect, segment_hits_rect, segments_cross};
pub use crate::graph::Shape;
use crate::graph::{Graph, LayoutKind, Relation, Side};
use crate::metrics::{Fonts, Run};
use crate::tree::Tree;
use crate::{layered, radial};

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
    pub algo: Algo,
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
    };
    for &v in &plan.order {
        let c = &plan.cands[v];
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
        csp::optimize(&sc, cfg.algo, cfg.opt_budget)
    } else {
        let (out, st) = csp::solve(&sc, cfg.algo, cfg.budget);
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

/// probe reports whether every hard constraint can hold at body font `b`.
pub fn probe(g: &Graph, t: &Tree, fonts: &Fonts, cfg: &Config, b: f64, stats: &mut Stats) -> bool {
    let hard: Vec<usize> = (0..g.constraints.len()).filter(|&i| g.constraints[i].hard).collect();
    attempt(g, t, fonts, cfg, b, Users::Hard(&hard), stats).is_some()
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
    let probe = |b: f64, set: &[usize], stats: &mut Stats| attempt(g, t, fonts, cfg, b, Users::Hard(set), stats);
    let Some(mut best) = probe(cfg.min_font, &hard, &mut stats) else {
        if hard.is_empty() || probe(cfg.min_font, &[], &mut stats).is_none() {
            return Err(Failure::NoRoom);
        }
        let mut feasible = |set: &[usize]| probe(cfg.min_font, set, &mut stats).is_some();
        let mut core = quickxplain(&mut feasible, &[], false, &hard);
        core.sort();
        return Err(Failure::Core(core));
    };
    let (mut lo, mut hi) = (cfg.min_font, cfg.max_font);
    if let Some(l) = probe(hi, &hard, &mut stats) {
        best = l;
        lo = hi;
    }
    while hi - lo > 0.05 {
        let mid = (lo + hi) / 2.0;
        match probe(mid, &hard, &mut stats) {
            Some(l) => {
                best = l;
                lo = mid;
            }
            None => hi = mid,
        }
    }
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
