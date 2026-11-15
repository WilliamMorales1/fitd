//! search: the layout retries after failed checks and the smallest-page search for `W:H` ratio pages.

use super::Config;
use crate::diag::{Diag, Level};
use crate::graph::{self, LayoutKind};
use crate::layout;
use crate::{check, key, metrics, radial, tree};

/// KEY_RATIO is the key text size relative to the body font when `keysize` is not set.
pub const KEY_RATIO: f64 = 0.85;

/// legible is the font size readable from arm's length on a page this big.
pub fn legible(page_w: f64, page_h: f64, min_font: f64) -> f64 {
    let diagonal_m = page_w.hypot(page_h) / 72.0 * 0.0254;
    (16.0 * diagonal_m).max(8.0).max(min_font)
}

pub fn retry(
    g: &graph::Graph,
    t: &tree::Tree,
    fonts: &metrics::Fonts,
    cfg: Config,
    page_fixed: bool,
    lay: &mut layout::Layout,
    diags: &mut Vec<Diag>,
) -> (Config, Vec<Diag>) {
    let arranged = if lay.kind == LayoutKind::Radial {
        radial::arrange(g, t, &cfg)
    } else {
        None
    };
    let quick = Config {
        first_cap: cfg.budget / 20,
        ..cfg
    };
    let turned = Config {
        page_w: cfg.page_h,
        page_h: cfg.page_w,
        ..quick
    };
    let mut tries: Vec<(&tree::Tree, Config, String)> = Vec::new();
    if let Some(a) = &arranged {
        let moved: Vec<String> = (0..t.children.len())
            .filter(|&v| a.children[v] != t.children[v])
            .map(|v| format!("`{}`", g.nodes[v].id))
            .collect();
        tries.push((
            a,
            quick,
            format!("reordered the branches under {}", moved.join(", ")),
        ));
    }
    if !page_fixed && cfg.page_w != cfg.page_h {
        tries.push((
            t,
            turned,
            format!(
                "turned the page to {}",
                if turned.page_w < turned.page_h {
                    "portrait"
                } else {
                    "landscape"
                }
            ),
        ));
    }
    let runs: Vec<_> = std::thread::scope(|s| {
        let handles: Vec<_> = tries
            .iter()
            .map(|(tr, c, _)| s.spawn(move || layout::solve(g, tr, fonts, c).ok()))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("retry thread"))
            .collect()
    });
    let before = diags.len();
    let mut best = (before, layout::merit(g, t, &cfg, lay));
    let (mut win, mut stats) = (None, lay.stats);
    for (k, run) in runs.into_iter().enumerate() {
        let Some(l) = run else { continue };
        stats.attempts += l.stats.attempts;
        stats.search += l.stats.search;
        let c = tries[k].1;
        let d = check::check(g, fonts, &c, &l);
        let key = (d.len(), layout::merit(g, t, &c, &l));
        if key.0 < best.0 || (key.0 == best.0 && key.1 < best.1) {
            best = key;
            win = Some((k, d.len()));
            *lay = l;
            *diags = d;
        }
    }
    let soft = (lay.stats.soft_cost, lay.stats.soft_total);
    lay.stats = layout::Stats {
        soft_cost: soft.0,
        soft_total: soft.1,
        ..stats
    };
    match win {
        Some((k, now)) => {
            let note = Diag::info(format!("{} (warnings: {before} -> {now})", tries[k].2));
            (tries[k].1, vec![note])
        }
        None => (cfg, Vec::new()),
    }
}

pub fn num_text(x: f64) -> String {
    format!("{x:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .into()
}

/// fit_ratio finds the smallest page of shape `r` that fits at the font floor and meets as many checks and preferences as an unbounded page.
type Score = (usize, usize);

/// fit_ratio sizes a `W:H` page for the content; layered flows without `bands=` also try 2 and 3 bands and keep the count that gives the largest font.
pub fn fit_ratio(
    g: &mut graph::Graph,
    t: &tree::Tree,
    fonts: &metrics::Fonts,
    cfg: Config,
    r: (f64, f64),
) -> (f64, f64) {
    let layered = matches!(g.settings.layout, Some(LayoutKind::Layered(_))) && !g.settings.lanes;
    let folds: &[Option<usize>] = if layered && g.settings.bands.is_none() {
        &[None, Some(2), Some(3)]
    } else {
        &[None]
    };
    let mut best: Option<(Option<usize>, Score, f64)> = None;
    for &fold in folds {
        g.settings.fold = fold;
        // A fold that does not fit on the best page so far cannot win, so it gets one solve there.
        let Some((goal, s)) = ratio_probe(g, t, fonts, cfg, r, best.map(|b| b.2)) else {
            if best.is_some() {
                break;
            }
            continue;
        };
        // Probes are rough, so another band count must win clearly.
        if best.is_none_or(|b| goal < b.1 || (goal == b.1 && s < 0.95 * b.2)) {
            best = Some((fold, goal, s));
        } else if fold.is_some() {
            break;
        }
    }
    let Some((fold, goal, s)) = best else {
        g.settings.fold = None;
        let (aw, ah) = (r.0 / r.0.max(r.1), r.1 / r.0.max(r.1));
        return ((aw * 16000.0).round(), (ah * 16000.0).round());
    };
    g.settings.fold = fold;
    ratio_finish(g, t, fonts, cfg, r, goal, s)
}

fn ratio_solve(
    g: &graph::Graph,
    t: &tree::Tree,
    fonts: &metrics::Fonts,
    cfg: Config,
    r: (f64, f64),
) -> impl Fn(f64, f64, bool) -> Option<(Score, f64)> {
    let (aw, ah) = (r.0 / r.0.max(r.1), r.1 / r.0.max(r.1));
    let dims = move |s: f64| ((aw * s).round(), (ah * s).round());
    let radial = g.settings.layout.unwrap_or(LayoutKind::Radial) == LayoutKind::Radial;
    move |s: f64, max_font: f64, quick: bool| {
        let (page_w, page_h) = dims(s);
        let mut c = Config {
            page_w,
            page_h,
            max_font,
            legible: legible(page_w, page_h, cfg.min_font),
            budget: if quick { cfg.budget / 4 } else { cfg.budget },
            first_cap: if quick {
                cfg.budget / 20
            } else {
                cfg.first_cap
            },
            ..cfg
        };
        let size = g.settings.key_size.unwrap_or(KEY_RATIO * c.min_font);
        c.key_h = key::KeyPlan::new(&g.key, fonts, page_w - 2.0 * c.margin, c.margin, size)
            .as_ref()
            .map_or(0.0, key::KeyPlan::height);
        // Includes the branch reordering that `retry` tries on the final page.
        let arranged = if radial {
            radial::arrange(g, t, &c)
        } else {
            None
        };
        std::iter::once(t)
            .chain(arranged.as_ref())
            .filter_map(|tr| {
                let l = layout::solve(g, tr, fonts, &c).ok()?;
                let d = check::check(g, fonts, &c, &l);
                let n = |lv| d.iter().filter(|x| x.level == lv).count();
                let top = l.tradeoff.iter().map(|x| x.0).fold(l.body, f64::max);
                Some(((n(Level::Error), n(Level::Warning)), top))
            })
            .min_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)))
    }
}

/// ratio_probe finds the page scale where the content just fits at the font floor, and the score there.
fn ratio_probe(
    g: &graph::Graph,
    t: &tree::Tree,
    fonts: &metrics::Fonts,
    cfg: Config,
    r: (f64, f64),
    start: Option<f64>,
) -> Option<(Score, f64)> {
    let (aw, ah) = (r.0 / r.0.max(r.1), r.1 / r.0.max(r.1));
    let m = cfg.margin;
    let solve = ratio_solve(g, t, fonts, cfg, r);
    // Layouts scale with the font, so the largest font on one page predicts the size at the floor.
    let unbounded = 1000.0 * cfg.min_font;
    // A fixed font (`font 11pt`) can outgrow the first guess, so double until something fits.
    let mut s = start.unwrap_or(1000.0);
    let (goal, mut top) = loop {
        match solve(s, unbounded, true) {
            Some(r) => break r,
            None if s < 16000.0 && start.is_none() => s *= 2.0,
            None => return None,
        }
    };
    for _ in 0..3 {
        // Only the area inside the margin scales with the font.
        let next =
            (s * aw.min(ah) - 2.0 * m) * cfg.min_font / top / aw.min(ah) + 2.0 * m / aw.min(ah);
        if (next / s - 1.0).abs() < 0.02 {
            break;
        }
        match solve(next, unbounded, true) {
            Some((_, f)) => (s, top) = (next, f),
            None => {
                s = next * 1.04;
                break;
            }
        }
    }
    Some((goal, s))
}

fn ratio_finish(
    g: &graph::Graph,
    t: &tree::Tree,
    fonts: &metrics::Fonts,
    cfg: Config,
    r: (f64, f64),
    goal: Score,
    s: f64,
) -> (f64, f64) {
    let (aw, ah) = (r.0 / r.0.max(r.1), r.1 / r.0.max(r.1));
    let dims = |s: f64| ((aw * s).round(), (ah * s).round());
    let solve = &ratio_solve(g, t, fonts, cfg, r);
    // Preferences met depend on search luck, so try a few sizes up and keep the smallest best one.
    let sizes: Vec<f64> = (0..6).map(|k| s * 1.04_f64.powi(k)).collect();
    // The first size usually meets the goal, so it runs alone before the rest fan out.
    let first = solve(sizes[0], cfg.max_font, false).map(|r| r.0);
    if first.is_some_and(|sc| sc <= goal) {
        return dims(sizes[0]);
    }
    let scores: Vec<_> = std::iter::once(first)
        .chain(std::thread::scope(|sc| {
            let hs: Vec<_> = sizes[1..]
                .iter()
                .map(|&x| sc.spawn(move || solve(x, cfg.max_font, false).map(|r| r.0)))
                .collect();
            hs.into_iter()
                .map(|h| h.join().expect("ratio probe"))
                .collect::<Vec<_>>()
        }))
        .collect();
    let pick = scores
        .iter()
        .position(|sc| sc.is_some_and(|sc| sc <= goal))
        .or_else(|| {
            let best = scores.iter().flatten().min()?;
            scores.iter().position(|sc| sc.as_ref() == Some(best))
        });
    dims(pick.map_or(sizes[5], |k| sizes[k]))
}
