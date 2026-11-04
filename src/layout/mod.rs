
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
mod stable;

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
pub use stable::Anchor;

pub fn measure_groups(g: &mut Graph, fonts: &Fonts) {
    fn measure(rel: &mut Relation, fonts: &Fonts) {
        if let Relation::Group {
            title,
            title_em,
            inner,
            ..
        } = rel
        {
            if let Some(t) = title {
                *title_em = label_size(fonts, t, 1.0).0;
            }
            inner.iter_mut().for_each(|r| measure(r, fonts));
        }
    }
    for c in &mut g.constraints {
        measure(&mut c.rel, fonts);
    }
}

#[derive(Clone, Copy)]
pub struct Config {
    pub page_w: f64,
    pub page_h: f64,
    pub margin: f64,
    pub min_font: f64,
    pub max_font: f64,
    pub wrap_em: f64,
    pub budget: u64,
    pub opt_budget: u64,
    pub first_cap: u64,
    pub legible: f64,
    pub draft: bool,
    pub anchor: Option<&'static Anchor>,
}

impl Config {
    pub fn page(&self) -> Rect {
        Rect::centered(
            (0.0, 0.0),
            self.page_w - 2.0 * self.margin,
            self.page_h - 2.0 * self.margin,
        )
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
    pub soft_cost: u64,
    pub soft_total: u64,
    pub probe_checks: u64,
    pub cut_off: Option<u64>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EdgeStyle {
    Line,
    Arrow,
    Link,
}

pub struct Drawn {
    pub a: usize,
    pub b: usize,
    pub style: EdgeStyle,
    pub width: f64,
    pub label: Option<String>,
    pub bow: f64,
    pub via: Option<Vec<P>>,
    pub t: f64,
    pub label_size: (f64, f64),
    pub aim: P,
}

impl Drawn {
    pub fn new(a: usize, b: usize, style: EdgeStyle, width: f64, label: Option<String>) -> Drawn {
        Drawn {
            a,
            b,
            style,
            width,
            label,
            bow: 0.0,
            via: None,
            t: 0.5,
            label_size: (0.0, 0.0),
            aim: (0.0, 0.0),
        }
    }
}

pub struct Layout {
    pub kind: LayoutKind,
    pub body: f64,
    pub boxes: Vec<Boxed>,
    pub pos: Vec<P>,
    pub edges: Vec<Drawn>,
    pub kx: f64,
    pub ky: f64,
    pub tol: f64,
    pub rot: f64,
    pub tradeoff: Vec<(f64, f64)>,
    pub stats: Stats,
}

impl Layout {
    pub fn rect(&self, v: usize) -> Rect {
        Rect::centered(self.pos[v], self.boxes[v].w, self.boxes[v].h)
    }

    pub fn radius(&self, v: usize) -> f64 {
        (self.pos[v].0 / self.kx).hypot(self.pos[v].1 / self.ky)
    }

    pub fn inside(&self, v: usize, p: P) -> bool {
        inside_box(
            self.pos[v],
            self.boxes[v].shape,
            self.boxes[v].w,
            self.boxes[v].h,
            p,
        )
    }

    pub fn ends(&self, e: &Drawn) -> (P, P) {
        (
            self.pos[e.a],
            (self.pos[e.b].0 + e.aim.0, self.pos[e.b].1 + e.aim.1),
        )
    }

    pub fn path(&self, e: &Drawn) -> Vec<P> {
        let (pa, pb) = self.ends(e);
        match &e.via {
            Some(way) => trim(way, &|p| self.inside(e.a, p), &|p| self.inside(e.b, p)),
            None => curve(
                pa,
                &|p| self.inside(e.a, p),
                pb,
                &|p| self.inside(e.b, p),
                e.bow,
            ),
        }
    }

    pub fn label_rect(&self, e: &Drawn) -> Option<Rect> {
        e.label
            .as_ref()
            .map(|_| label_rect(&self.path(e), e.t, e.label_size))
    }
}

pub struct Cand {
    pub cost: f64,
    pub at: P,
    pub r: f64,
}

#[derive(Clone, Copy)]
pub enum Kind {
    Apart(f64),
    Outward(f64),
    Clear(f64),
    NoCross,
    Rel(usize),
    Aligned(Side),
    LabelClear(usize, usize, f64),
    LabelPage(usize),
    LabelApart(usize, usize, f64),
    LabelFrame(usize, usize),
    LinkApart(usize, usize),
    LinkEdge(usize, usize, usize),
    LinkClear(usize, usize, f64),
    LinkPage(usize),
    Outside(usize, usize),
    Frames(usize, usize),
}

pub struct Plan {
    pub order: Vec<usize>,
    pub cands: Vec<Vec<Cand>>,
    pub cons: Vec<(Vec<usize>, Kind, u64)>,
    pub edges: Vec<Drawn>,
    pub kx: f64,
    pub ky: f64,
    pub tol: f64,
    pub crowded: Option<(usize, f64, f64)>,
}

#[derive(Clone, Copy)]
enum Users<'a> {
    Hard(&'a [usize]),
    All(&'a Layout),
    Wanted,
    Routes(&'a Layout),
    Shrunk(&'a Layout),
}

const ROTATIONS: [f64; 4] = [0.0, 10.0, 30.0, 45.0];

fn attempt(
    g: &Graph,
    t: &Tree,
    fonts: &Fonts,
    cfg: &Config,
    b: f64,
    users: Users,
    stats: &mut Stats,
) -> Option<Layout> {
    stats.attempts += 1;
    let kind = g.settings.layout.unwrap_or(LayoutKind::Radial);
    let n = g.nodes.len();
    let boxes = measure_all(g, t, fonts, cfg, b);
    let rots = match (kind, users) {
        (_, Users::All(prev) | Users::Routes(prev) | Users::Shrunk(prev)) => vec![prev.rot],
        (LayoutKind::Radial, Users::Hard(_) | Users::Wanted) => match cfg.anchor {
            Some(a) => std::iter::once(a.rot)
                .chain(ROTATIONS.into_iter().filter(|&r| r != a.rot))
                .collect(),
            None => ROTATIONS.to_vec(),
        },
        (LayoutKind::Layered(_), Users::Hard(_) | Users::Wanted) => vec![0.0],
    };
    let optimizing = matches!(users, Users::All(_));
    let routing = matches!(users, Users::Routes(_));
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
                        stop[i + 1..]
                            .iter()
                            .for_each(|f| f.store(true, Ordering::Relaxed));
                    }
                    (built, run)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("solver thread"))
            .unzip()
    });
    let first = runs
        .iter()
        .position(|(out, ..)| matches!(out, Outcome::Sat(_)));
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
            eprintln!(
                "font {b:.2}{} rot {}: {what} after {st:?}",
                if optimizing { " (optimizing)" } else { "" },
                rots[i]
            );
        }
    }
    if first.is_none() && runs.iter().any(|(out, ..)| *out == Outcome::Unknown) {
        let budget = if optimizing || routing {
            cfg.opt_budget
        } else {
            cfg.budget
        };
        stats.cut_off = stats.cut_off.max(Some(budget));
    }
    let win = first?;
    let (Outcome::Sat(val), cost, st) = &runs[win] else {
        unreachable!()
    };
    let (sc, mut edges, plan) = built.into_iter().nth(win).unwrap();
    stats.probe_checks = st.checks;
    let mut val = val.clone();
    let mut cost = *cost;
    if optimizing {
        let ctx = Readable::new(t, kind, &edges, sc.page, cfg.legible);
        cost = polish(
            &sc,
            &ctx,
            &mut val,
            if cfg.draft {
                POLISH_MOVES / 10
            } else {
                POLISH_MOVES
            },
            cfg.anchor,
        );
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
    Some(Layout {
        kind,
        body: b,
        boxes,
        pos,
        edges,
        kx: plan.0,
        ky: plan.1,
        tol: plan.2,
        rot: rots[win],
        tradeoff: Vec::new(),
        stats: *stats,
    })
}

pub enum Failure {
    NoRoom {
        crowded: Option<(usize, f64, f64)>,
        cut_off: Option<u64>,
    },
    Core {
        core: Vec<usize>,
        cut_off: Option<u64>,
    },
}

pub fn largest_branch(
    g: &Graph,
    t: &Tree,
    fonts: &Fonts,
    cfg: &Config,
) -> Option<(usize, usize, f64)> {
    if t.children[t.root].len() < 2 {
        return None;
    }
    let boxes = measure_all(g, t, fonts, cfg, cfg.min_font);
    let area = |v: usize| boxes[v].w * boxes[v].h;
    let total: f64 = (0..boxes.len()).map(area).sum();
    let mut per: Vec<(usize, usize, f64)> =
        t.children[t.root].iter().map(|&b| (b, 0, 0.0)).collect();
    for v in 0..boxes.len() {
        if let Some(e) = t.branch(v).and_then(|b| per.iter_mut().find(|e| e.0 == b)) {
            e.1 += 1;
            e.2 += area(v);
        }
    }
    per.into_iter()
        .max_by(|a, b| a.2.total_cmp(&b.2).then(b.0.cmp(&a.0)))
        .map(|(b, n, a)| (b, n, a / total))
}

const TRADE: [f64; 3] = [1.0, 0.95, 0.9];

const W_FONT: f64 = 800.0;

pub fn merit(g: &Graph, t: &Tree, cfg: &Config, lay: &Layout) -> f64 {
    let rects: Vec<Rect> = (0..lay.pos.len()).map(|v| lay.rect(v)).collect();
    let q = Readable::new(t, lay.kind, &lay.edges, cfg.page(), cfg.legible).score(&rects);
    let rect = |v: usize| lay.rect(v);
    let unmet: u64 = g
        .constraints
        .iter()
        .filter(|c| !c.hard && !relation_holds(&c.rel, &rect, lay.body, &cfg.page(), lay.tol))
        .map(|c| c.weight as u64)
        .sum();
    let drift = cfg.anchor.map_or(0.0, |a| a.drift(lay));
    q.total() + SOFT_PENALTY * unmet as f64 + drift - W_FONT * (lay.body / cfg.legible).ln()
}

pub fn solve(g: &Graph, t: &Tree, fonts: &Fonts, cfg: &Config) -> Result<Layout, Failure> {
    let mut stats = Stats::default();
    let hard: Vec<usize> = (0..g.constraints.len())
        .filter(|&i| g.constraints[i].hard)
        .collect();
    let probe = |b: f64, set: &[usize], budget: u64, stats: &mut Stats| {
        attempt(
            g,
            t,
            fonts,
            &Config { budget, ..*cfg },
            b,
            Users::Hard(set),
            stats,
        )
    };

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
                if found.is_none() {
                    if next > lo || cap < cfg.budget {
                        stats.cut_off = None;
                        found = probe(lo, &hard, cfg.budget, &mut stats);
                    }
                    if found.is_none() {
                        break;
                    }
                }
            }
        }
        if hi - lo <= if cfg.draft { 0.1 } else { 0.03 } * lo {
            break;
        }
        next = (lo + hi) / 2.0;
    }
    let mut best = match found {
        Some(l) => l,
        None => {
            let full_cut = stats.cut_off.take();
            if hard.is_empty() || probe(cfg.min_font, &[], cfg.budget, &mut stats).is_none() {
                let crowded = match g.settings.layout {
                    Some(LayoutKind::Layered(dir)) => {
                        layered::plan(
                            g,
                            fonts,
                            &measure_all(g, t, fonts, cfg, cfg.min_font),
                            cfg,
                            cfg.min_font,
                            dir,
                        )
                        .crowded
                    }
                    _ => None,
                };
                let cut_off = if hard.is_empty() {
                    full_cut
                } else {
                    stats.cut_off
                };
                return Err(Failure::NoRoom { crowded, cut_off });
            }
            let mut feasible =
                |set: &[usize]| probe(cfg.min_font, set, cfg.budget / 4, &mut stats).is_some();
            let mut core = quickxplain(&mut feasible, &[], false, &hard);
            core.sort();
            return Err(Failure::Core {
                core,
                cut_off: full_cut.max(stats.cut_off),
            });
        }
    };
    let (mut kept, mut locked) = (false, None);
    if let Some(a) = cfg.anchor
        && a.body >= cfg.min_font
        && a.body <= cfg.max_font
    {
        if a.body == best.body {
            kept = true;
        } else if let Some(l) = probe(a.body, &hard, cap, &mut stats) {
            if a.body > best.body {
                (best, kept) = (l, true);
            } else {
                locked = Some(l);
            }
        }
    }
    let top = best.body;
    let fonts_tried: Vec<f64> = match &locked {
        Some(l) => vec![top, l.body],
        None if cfg.draft || kept => vec![top],
        None => TRADE
            .iter()
            .map(|k| k * top)
            .filter(|&f| f == top || (f >= cfg.legible && f >= cfg.min_font))
            .collect(),
    };
    let locked = locked.as_ref();
    type Trial = (f64, Option<(f64, Layout, (u64, u64))>, Stats);
    let round = |best: &Layout, wanted: bool| -> Vec<Trial> {
        std::thread::scope(|s| {
            let handles: Vec<_> = fonts_tried
                .iter()
                .map(|&f| {
                    s.spawn(move || {
                        let mut st = Stats::default();
                        let base = locked.filter(|l| l.body == f);
                        let start = match (wanted, f == top || base.is_some()) {
                            (true, _) => attempt(
                                g,
                                t,
                                fonts,
                                &Config {
                                    budget: cap,
                                    ..*cfg
                                },
                                f,
                                Users::Wanted,
                                &mut st,
                            ),
                            (false, false) => attempt(
                                g,
                                t,
                                fonts,
                                &Config {
                                    budget: cap,
                                    ..*cfg
                                },
                                f,
                                Users::Shrunk(best),
                                &mut st,
                            ),
                            (false, true) => None,
                        };
                        if start.is_none() && (wanted || (f != top && base.is_none())) {
                            return (f, None, st);
                        }
                        let mut one = Stats::default();
                        let polished = attempt(
                            g,
                            t,
                            fonts,
                            cfg,
                            f,
                            Users::All(start.as_ref().or(base).unwrap_or(best)),
                            &mut one,
                        );
                        st.attempts += one.attempts;
                        st.search += one.search;
                        (
                            f,
                            polished.map(|l| {
                                (merit(g, t, cfg, &l), l, (one.soft_cost, one.soft_total))
                            }),
                            st,
                        )
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("trade-off thread"))
                .collect()
        })
    };
    let mut tradeoff: Vec<(f64, f64)> = Vec::new();
    let mut picked: Option<(f64, Layout, (u64, u64))> = None;
    for wanted in [false, true] {
        if wanted {
            let Some(p) = &picked else { break };
            let rect = |v: usize| p.1.rect(v);
            if g.constraints
                .iter()
                .all(|c| c.hard || relation_holds(&c.rel, &rect, p.1.body, &cfg.page(), p.1.tol))
            {
                break;
            }
        }
        for (f, polished, st) in round(&best, wanted) {
            stats.attempts += st.attempts;
            stats.search += st.search;
            let Some((m, l, soft)) = polished else {
                continue;
            };
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
    if best
        .edges
        .iter()
        .any(|e| e.style == EdgeStyle::Link || e.label.is_some())
        && let Some(mut l) = attempt(
            g,
            t,
            fonts,
            cfg,
            best.body,
            Users::Routes(&best),
            &mut stats,
        )
    {
        l.tradeoff = std::mem::take(&mut best.tradeoff);
        best = l;
    }
    spread_arrivals(&mut best, &cfg.page());
    best.stats = stats;
    Ok(best)
}

fn spread_arrivals(lay: &mut Layout, page: &Rect) {
    let LayoutKind::Layered(dir) = lay.kind else {
        return;
    };
    let down = dir == Dir::Down;
    let along = |p: P| if down { -p.1 } else { p.0 };
    let across = |p: P| if down { p.0 } else { p.1 };
    let n = lay.pos.len();
    let straight = |e: &Drawn| e.style == EdgeStyle::Arrow;
    for v in 0..n {
        let mut ins: Vec<usize> = (0..lay.edges.len())
            .filter(|&k| {
                straight(&lay.edges[k])
                    && lay.edges[k].b == v
                    && along(lay.pos[lay.edges[k].a]) < along(lay.pos[v])
            })
            .collect();
        if ins.len() < 2 {
            continue;
        }
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

fn arrival_clear(lay: &Layout, k: usize, page: &Rect) -> bool {
    let e = &lay.edges[k];
    let (p, q) = lay.ends(e);
    let n = lay.pos.len();
    if (0..n).any(|u| u != e.a && u != e.b && segment_hits_rect(p, q, &lay.rect(u))) {
        return false;
    }
    let others = lay
        .edges
        .iter()
        .enumerate()
        .filter(|&(j, f)| j != k && f.style != EdgeStyle::Link);
    if others.clone().any(|(_, f)| {
        let (r, s) = lay.ends(f);
        segments_cross(p, q, r, s)
    }) {
        return false;
    }
    let Some(l) = lay.label_rect(e) else {
        return true;
    };
    let clear = l.inflate(0.15 * lay.body);
    l.within(page)
        && (0..n).all(|u| !clear.intersects(&lay.rect(u)))
        && lay
            .edges
            .iter()
            .enumerate()
            .all(|(j, f)| j == k || lay.label_rect(f).is_none_or(|r| !clear.intersects(&r)))
}
