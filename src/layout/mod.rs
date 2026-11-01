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
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};

mod explain;
mod measure;
mod path;
mod polish;
mod relations;
mod scene;

use explain::quickxplain;
use measure::measure_all;
use path::{BOWS, SPOTS, detours, inside_box, trim};
use polish::{POLISH_MOVES, SOFT_PENALTY, polish};
use relations::{FRAME_CLEAR, aligned};
use scene::{Scene, build};

pub use measure::{label_size, line_height};
pub use path::{curve, label_rect, paths_meet};
pub use polish::Readable;
pub use relations::{FRAME_PAD, TITLE_BAND, frame, frame_clear, relation_holds};

/// measure_groups sets each group's title width, which frames need, from the loaded fonts.
pub fn measure_groups(g: &mut Graph, fonts: &Fonts) {
    for c in &mut g.constraints {
        if let Relation::Group { title: Some(t), title_em, .. } = &mut c.rel {
            *title_em = label_size(fonts, t, 1.0).0;
        }
    }
}

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
    /// Constraint checks for font probes until one fits; later probes get twice the last
    /// fit's cost.
    pub first_cap: u64,
    /// Smallest body font readable at the page's viewing distance; also the length unit of
    /// readability scores.
    pub legible: f64,
    /// Trade quality for speed while editing: coarser font search, one font, short polish.
    pub draft: bool,
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
    /// Bend of a link (see `curve`), or instead a detour around boxes (untrimmed), and the
    /// label's position along the path, 0 to 1. The solver picks them.
    pub bow: f64,
    pub via: Option<Vec<P>>,
    pub t: f64,
    /// Label box size in points, set once the font is known.
    pub label_size: (f64, f64),
    /// Where an arrow ends, as an offset from the target's center (see `spread_arrivals`).
    pub aim: P,
}

impl Drawn {
    pub fn new(a: usize, b: usize, style: EdgeStyle, width: f64, label: Option<String>) -> Drawn {
        Drawn { a, b, style, width, label, bow: 0.0, via: None, t: 0.5, label_size: (0.0, 0.0), aim: (0.0, 0.0) }
    }
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

    /// ends are the points an edge runs between before trimming: the centers of its nodes,
    /// with the target's moved by `aim`.
    pub fn ends(&self, e: &Drawn) -> (P, P) {
        (self.pos[e.a], (self.pos[e.b].0 + e.aim.0, self.pos[e.b].1 + e.aim.1))
    }

    /// path is an edge's drawn route with its chosen bend, trimmed to the node borders.
    pub fn path(&self, e: &Drawn) -> Vec<P> {
        let (pa, pb) = self.ends(e);
        match &e.via {
            Some(way) => trim(way, &|p| self.inside(e.a, p), &|p| self.inside(e.b, p)),
            None => curve(pa, &|p| self.inside(e.a, p), pb, &|p| self.inside(e.b, p), e.bow),
        }
    }

    pub fn label_rect(&self, e: &Drawn) -> Option<Rect> {
        e.label.as_ref().map(|_| label_rect(&self.path(e), e.t, e.label_size))
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
    LabelClear(usize, usize, f64),
    /// Route's label stays on the page.
    LabelPage(usize),
    /// Two routes' labels do not overlap.
    LabelApart(usize, usize, f64),
    /// Route `0`'s label sits wholly inside or wholly outside group constraint `1`'s frame,
    /// clear of its border and title (soft).
    LabelFrame(usize, usize),
    /// Two dashed links neither cross nor run close together (soft).
    LinkApart(usize, usize),
    /// A dashed link does not cross the straight edge between nodes `1` and `2` (soft).
    LinkEdge(usize, usize, usize),
    /// A dashed link stays out of node `1` grown by this many ems (soft).
    LinkClear(usize, usize, f64),
    /// Group constraint `0`'s frame keeps clear of node `1`, which is not a member.
    Outside(usize, usize),
    /// The frames of two group constraints keep clear of each other.
    Frames(usize, usize),
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
    /// Layered only, when the layers do not fit along the flow: (layers, length they need,
    /// length the page gives), in points.
    pub crowded: Option<(usize, f64, f64)>,
}

/// Which user constraints an attempt includes.
#[derive(Clone, Copy)]
enum Users<'a> {
    /// These hard constraints only (indices into `Graph::constraints`).
    Hard(&'a [usize]),
    /// Every hard constraint plus `prefer`s and layered crossings, on this layout's plan and
    /// trying its spots first.
    All(&'a Layout),
    /// Every hard constraint and every `prefer`, all as hard: a layout that meets what the
    /// author asked for, to weigh against the largest font.
    Wanted,
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
    let boxes = measure_all(g, t, fonts, cfg, b);
    let rots = match (kind, users) {
        (_, Users::All(prev) | Users::Routes(prev) | Users::Shrunk(prev)) => vec![prev.rot],
        (LayoutKind::Radial, Users::Hard(_) | Users::Wanted) => ROTATIONS.to_vec(),
        (LayoutKind::Layered(_), Users::Hard(_) | Users::Wanted) => vec![0.0],
    };
    let optimizing = matches!(users, Users::All(_));
    let routing = matches!(users, Users::Routes(_));
    // Once a plan solves, the plans after it in list order cannot win, so they stop.
    let stop: Vec<AtomicBool> = rots.iter().map(|_| AtomicBool::new(false)).collect();
    let (built, runs): (Vec<_>, Vec<(Outcome, u64, csp::Stats)>) = std::thread::scope(|s| {
        let (stop, boxes) = (&stop, &boxes);
        let handles: Vec<_> = rots
            .iter()
            .enumerate()
            .map(|(i, &rot)| {
                s.spawn(move || {
                    let built = build(g, t, fonts, cfg, b, users, kind, boxes, rot);
                    let run = if optimizing || routing {
                        csp::optimize(&built.0, cfg.opt_budget, &stop[i])
                    } else {
                        let (out, st) = csp::solve(&built.0, cfg.budget, &stop[i]);
                        (out, 0, st)
                    };
                    if matches!(run.0, Outcome::Sat(_)) {
                        stop[i + 1..].iter().for_each(|f| f.store(true, Ordering::Relaxed));
                    }
                    (built, run)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("solver thread")).unzip()
    });
    // Plans after the first that solves were cancelled at a timing-dependent point, so their
    // work is left out of the stats to keep the build log reproducible.
    let first = runs.iter().position(|(out, ..)| matches!(out, Outcome::Sat(_)));
    for (i, (out, _, st)) in runs.iter().enumerate() {
        if first.is_none_or(|w| i <= w) {
            stats.search += *st;
        }
        if std::env::var_os("FITD_DEBUG").is_some() {
            let what = match out {
                Outcome::Sat(_) => "sat",
                Outcome::Unsat => "unsat",
                Outcome::Unknown => "budget",
            };
            eprintln!("font {b:.2}{} rot {}: {what} after {st:?}", if optimizing { " (optimizing)" } else { "" }, rots[i]);
        }
    }
    let win = first?;
    let (Outcome::Sat(val), cost, st) = &runs[win] else { unreachable!() };
    let (sc, mut edges, plan) = built.into_iter().nth(win).unwrap();
    stats.probe_checks = st.checks;
    let mut val = val.clone();
    let mut cost = *cost;
    if optimizing {
        let ctx = Readable::new(t, kind, &edges, sc.page, cfg.legible);
        cost = polish(&sc, &ctx, &mut val, if cfg.draft { POLISH_MOVES / 10 } else { POLISH_MOVES });
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
        let (k, t) = r.opts[val[base + i] as usize];
        let e = &mut edges[r.edge];
        e.t = t;
        match k.checked_sub(r.bows.len()) {
            Some(j) => e.via = Some(r.vias[j].clone()),
            None => (e.bow, e.via) = (r.bows[k], None),
        }
    }
    Some(Layout { kind, body: b, boxes, pos, edges, kx: plan.0, ky: plan.1, tol: plan.2, rot: rots[win], tradeoff: Vec::new(), stats: *stats })
}

pub enum Failure {
    /// Nothing fits even without user constraints; for a layered layout whose layers are too
    /// many for the page, how crowded the flow is (see `Plan::crowded`).
    NoRoom(Option<(usize, f64, f64)>),
    /// These hard user constraints cannot all hold at the minimum font; removing any one of
    /// them makes the rest satisfiable (within the search budget).
    Core(Vec<usize>),
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
    let (mut found, mut next, mut cap) = (None, hi, cfg.first_cap);
    loop {
        match probe(next, &hard, cap, &mut stats) {
            Some(l) => {
                cap = (stats.probe_checks * 2).clamp(1_000_000, cfg.budget);
                lo = next;
                found = Some(l);
            }
            None => {
                hi = next;
                // Settle whether anything fits before bisecting further: on an unsatisfiable
                // input every probe down to the floor would otherwise run its whole cap.
                if found.is_none() {
                    found = if next > lo || cap < cfg.budget { probe(lo, &hard, cfg.budget, &mut stats) } else { None };
                    if found.is_none() {
                        break;
                    }
                }
            }
        }
        // The trade-off below steps down 5% at a time, so finer precision here is wasted.
        if hi - lo <= if cfg.draft { 0.1 } else { 0.03 } * lo {
            break;
        }
        next = (lo + hi) / 2.0;
    }
    let mut best = match found {
        Some(l) => l,
        None => {
            if hard.is_empty() || probe(cfg.min_font, &[], cfg.budget, &mut stats).is_none() {
                let crowded = match g.settings.layout {
                    Some(LayoutKind::Layered(dir)) => layered::plan(g, fonts, &measure_all(g, t, fonts, cfg, cfg.min_font), cfg, cfg.min_font, dir).crowded,
                    _ => None,
                };
                return Err(Failure::NoRoom(crowded));
            }
            // The full set already failed on the whole budget. Subsets that fit usually solve
            // fast, so core probes get the first probe's cap; a core is minimal within it.
            let mut feasible = |set: &[usize]| probe(cfg.min_font, set, cfg.budget / 4, &mut stats).is_some();
            let mut core = quickxplain(&mut feasible, &[], false, &hard);
            core.sort();
            return Err(Failure::Core(core));
        }
    };
    // Then trade font against readability. Above the legibility floor a larger font helps
    // less and less, so a slightly smaller font may buy a clearer arrangement: polish a few
    // fonts and keep the best merit. Below the floor nothing smaller is tried.
    let top = best.body;
    let trade: &[f64] = if cfg.draft { &TRADE[..1] } else { &TRADE };
    let fonts_tried: Vec<f64> = trade.iter().map(|k| k * top).filter(|&f| f == top || (f >= cfg.legible && f >= cfg.min_font)).collect();
    // Each font is independent, so they run in parallel, each with its own stats; the soft
    // cost is the polished winner's. The first round starts from the largest font's layout.
    // When its winner still misses a `prefer`, a second round starts each font from a layout
    // that meets every one, since a preference outweighs a few percent of font (see `merit`).
    type Trial = (f64, Option<(f64, Layout, (u64, u64))>, Stats);
    let round = |best: &Layout, wanted: bool| -> Vec<Trial> {
        std::thread::scope(|s| {
            let handles: Vec<_> = fonts_tried
                .iter()
                .map(|&f| {
                    s.spawn(move || {
                        let mut st = Stats::default();
                        let start = match (wanted, f == top) {
                            (true, _) => attempt(g, t, fonts, &Config { budget: cap, ..*cfg }, f, Users::Wanted, &mut st),
                            (false, false) => attempt(g, t, fonts, &Config { budget: cap, ..*cfg }, f, Users::Shrunk(best), &mut st),
                            (false, true) => None,
                        };
                        if start.is_none() && (wanted || f != top) {
                            return (f, None, st);
                        }
                        let mut one = Stats::default();
                        let polished = attempt(g, t, fonts, cfg, f, Users::All(start.as_ref().unwrap_or(best)), &mut one);
                        st.attempts += one.attempts;
                        st.search += one.search;
                        (f, polished.map(|l| (merit(g, t, cfg, &l), l, (one.soft_cost, one.soft_total))), st)
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().expect("trade-off thread")).collect()
        })
    };
    let mut tradeoff: Vec<(f64, f64)> = Vec::new();
    let mut picked: Option<(f64, Layout, (u64, u64))> = None;
    for wanted in [false, true] {
        if wanted {
            let Some(p) = &picked else { break };
            let rect = |v: usize| p.1.rect(v);
            if g.constraints.iter().all(|c| c.hard || relation_holds(&c.rel, &rect, p.1.body, &cfg.page(), p.1.tol)) {
                break;
            }
        }
        for (f, polished, st) in round(&best, wanted) {
            stats.attempts += st.attempts;
            stats.search += st.search;
            let Some((m, l, soft)) = polished else { continue };
            match tradeoff.iter_mut().find(|x| x.0 == f) {
                Some(x) => x.1 = x.1.min(m),
                None => tradeoff.push((f, m)),
            }
            if picked.as_ref().is_none_or(|p| m < p.0) {
                picked = Some((m, l, soft));
            }
        }
    }
    if let Some((_, l, soft)) = picked {
        best = l;
        (stats.soft_cost, stats.soft_total) = soft;
    }
    best.tradeoff = tradeoff;
    // Then, with nodes fixed, pick link routes and label spots: few variables, so the search
    // is exhaustive.
    if best.edges.iter().any(|e| e.style == EdgeStyle::Link || e.label.is_some())
        && let Some(mut l) = attempt(g, t, fonts, cfg, best.body, Users::Routes(&best), &mut stats)
    {
        l.tradeoff = std::mem::take(&mut best.tradeoff);
        best = l;
    }
    spread_arrivals(&mut best, &cfg.page());
    best.stats = stats;
    Ok(best)
}

/// spread_arrivals spaces the ends of arrows that enter one node from earlier layers along its
/// entry side, in the order of their sources, so they do not meet in one point. An arrow keeps
/// its center end when the new one would touch another box, a label, or another edge.
fn spread_arrivals(lay: &mut Layout, page: &Rect) {
    let LayoutKind::Layered(dir) = lay.kind else { return };
    let down = dir == Dir::Down;
    // Position along the flow and across it.
    let along = |p: P| if down { -p.1 } else { p.0 };
    let across = |p: P| if down { p.0 } else { p.1 };
    let n = lay.pos.len();
    let straight = |e: &Drawn| e.style == EdgeStyle::Arrow;
    for v in 0..n {
        let mut ins: Vec<usize> = (0..lay.edges.len()).filter(|&k| straight(&lay.edges[k]) && lay.edges[k].b == v && along(lay.pos[lay.edges[k].a]) < along(lay.pos[v])).collect();
        if ins.len() < 2 {
            continue;
        }
        // By the slope they arrive at, so the spread ends keep the arrows apart.
        let slope = |k: usize| {
            let (a, c) = (lay.pos[lay.edges[k].a], lay.pos[v]);
            (across(a) - across(c)) / (along(c) - along(a))
        };
        ins.sort_by(|&i, &j| slope(i).total_cmp(&slope(j)));
        let w = if down { lay.boxes[v].w } else { lay.boxes[v].h };
        for (i, &k) in ins.iter().enumerate() {
            let off = ((i + 1) as f64 / (ins.len() + 1) as f64 - 0.5) * 0.6 * w;
            lay.edges[k].aim = if down { (off, 0.0) } else { (0.0, off) };
            if !arrival_clear(lay, k, page) {
                lay.edges[k].aim = (0.0, 0.0);
            }
        }
    }
}

/// arrival_clear reports whether edge `k` misses every box but its own, every edge it does not
/// share a node with, and, with its label, every label and box.
fn arrival_clear(lay: &Layout, k: usize, page: &Rect) -> bool {
    let e = &lay.edges[k];
    let (p, q) = lay.ends(e);
    let n = lay.pos.len();
    if (0..n).any(|u| u != e.a && u != e.b && segment_hits_rect(p, q, &lay.rect(u))) {
        return false;
    }
    // With the end moved, even an edge sharing a node can cross; the checker tests every pair.
    let others = lay.edges.iter().enumerate().filter(|&(j, f)| j != k && f.style != EdgeStyle::Link);
    if others.clone().any(|(_, f)| {
        let (r, s) = lay.ends(f);
        segments_cross(p, q, r, s)
    }) {
        return false;
    }
    let Some(l) = lay.label_rect(e) else { return true };
    let clear = l.inflate(0.15 * lay.body);
    l.within(page) && (0..n).all(|u| !clear.intersects(&lay.rect(u))) && lay.edges.iter().enumerate().all(|(j, f)| j == k || lay.label_rect(f).is_none_or(|r| !clear.intersects(&r)))
}
