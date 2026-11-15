//! compile: the pipeline from source text to a checked layout, shared by the CLI, the LSP, and tests.

use crate::ast::Stmt;
use crate::diag::{Diag, Level, Source};
use crate::geom::Rect;
use crate::graph::{self, Graph, LayoutKind};
use crate::layout::{self, Config, Failure, Layout};
use crate::{check as checks, expand, include, key, metrics, parse, sema, theme, tree};

/// Options are the command-line overrides of source settings.
#[derive(Clone, Debug)]
pub struct Options {
    pub page: Option<String>,
    pub portrait: bool,
    pub landscape: bool,
    pub draft: bool,
    pub bands: Option<usize>,
    pub margin: Option<f64>,
    pub min_font: Option<f64>,
    pub max_font: Option<f64>,
    pub wrap: Option<f64>,
    pub budget: u64,
    pub expand_budget: u64,
    pub theme: Option<String>,
    pub family: Option<String>,
    /// Lock file read for an anchor; the caller writes it back.
    pub lock: Option<String>,
    pub images: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            page: None,
            portrait: false,
            landscape: false,
            draft: false,
            bands: None,
            margin: None,
            min_font: None,
            max_font: None,
            wrap: None,
            budget: 20_000_000,
            expand_budget: expand::DEFAULT_BUDGET,
            theme: None,
            family: None,
            lock: None,
            images: true,
        }
    }
}

/// Front is the result of the front end: expanded source, statements, and the graph.
pub struct Front {
    pub src: Source,
    pub stmts: Vec<Stmt>,
    pub graph: Graph,
    pub diags: Vec<Diag>,
}

/// check runs includes, macros, parse, and sema, with diagnostics sorted by position.
pub fn check(path: &str, text: &str, opts: &Options) -> Front {
    let expand::Loaded {
        src,
        toks,
        mut diags,
        ..
    } = expand::load(path, text, opts.expand_budget);
    let (stmts, parse_diags) = parse::parse_tokens(&src.text, toks, Vec::new());
    diags.extend(parse_diags);
    let (mut graph, sema_diags) = sema::analyze(&stmts);
    diags.extend(sema_diags);
    if opts.images {
        diags.extend(load_images(&mut graph, path));
    }
    diags.retain(|d| !include::unused_shared_style(&src, d));
    diags.sort_by_key(Diag::pos);
    // A loop calling one template writes the same error at the same call site once per pass.
    diags.dedup_by(|a, b| a.code == b.code && a.msg == b.msg && a.pos() == b.pos());
    Front {
        src,
        stmts,
        graph,
        diags,
    }
}

/// Compiled is a laid-out diagram ready to emit.
pub struct Compiled {
    pub src: Source,
    pub graph: Graph,
    pub tree: tree::Tree,
    pub fonts: metrics::Fonts,
    pub theme: theme::Theme,
    pub cfg: Config,
    pub layout: Layout,
    pub key: Option<key::KeyPlan>,
    /// Ratio pages keep their shape in bitmaps.
    pub shape: Option<(f64, f64)>,
    /// Post-layout checks; errors here still leave a layout.
    pub diags: Vec<Diag>,
}

impl Compiled {
    /// svg draws the whole page, or `view` when given.
    pub fn svg(&self, view: Option<Rect>) -> String {
        crate::emit::svg(
            &self.graph,
            &self.tree,
            &self.fonts,
            &self.layout,
            view.unwrap_or(self.page()),
            &self.theme,
            self.key.as_ref(),
        )
    }

    /// page is the page rectangle centered on the origin.
    pub fn page(&self) -> Rect {
        Rect::centered((0.0, 0.0), self.cfg.page_w, self.cfg.page_h)
    }

    /// png_view is the bitmap crop: the page for ratio pages, else the drawing.
    pub fn png_view(&self) -> Rect {
        if self.shape.is_some() {
            self.page()
        } else {
            crate::emit::png_view(
                &self.graph,
                &self.layout,
                self.key.as_ref(),
                self.cfg.margin,
            )
        }
    }

    /// tikz draws the page as a TikZ picture.
    pub fn tikz(&self) -> String {
        crate::emit::tikz(
            &self.graph,
            &self.tree,
            &self.fonts,
            &self.layout,
            self.cfg.page_w,
            self.cfg.page_h,
            &self.theme,
            self.key.as_ref(),
        )
    }
}

/// Stop ends a compile without a layout.
#[derive(Debug)]
pub enum Stop {
    /// Diagnostics already went to `report`; the text, if any, still needs printing.
    Failed(String),
    /// A setup error, such as a bad page or missing font.
    Err(String),
}

impl From<String> for Stop {
    fn from(e: String) -> Stop {
        Stop::Err(e)
    }
}

/// page_size reads a paper name or `WxH` size, turned portrait when asked.
pub fn page_size(spec: &str, portrait: bool) -> Result<(f64, f64), String> {
    let (w, h) = sema::page_size(&spec.to_ascii_lowercase(), Some("pt"))?;
    Ok(if portrait {
        (w.min(h), w.max(h))
    } else {
        (w, h)
    })
}

/// compile lays out `text`, passing notes and diagnostics to `report` as they arise.
pub fn compile(
    path: &str,
    text: &str,
    opts: &Options,
    report: &mut dyn FnMut(&Source, &[Diag]),
) -> Result<Compiled, Stop> {
    let Front {
        src,
        graph: mut g,
        diags,
        ..
    } = check(path, text, opts);
    let mut show = |d: &[Diag]| report(&src, d);
    show(&diags);
    if diags.iter().any(|d| d.level == Level::Error) {
        return Err(Stop::Failed(String::new()));
    }

    if opts.bands.is_some() {
        g.settings.bands = opts.bands;
    }
    let set = &g.settings;
    // Without `keysize`, the key follows the body font, which is known only after a solve.
    let key_auto = g.settings.key_size.is_none();
    let page_spec = &opts.page;
    let mut shape = match page_spec.as_deref().and_then(sema::ratio) {
        Some(r) => Some(r?),
        None if page_spec.is_some() => None,
        None => set.page_ratio,
    };
    let (mut page_w, mut page_h) = match (page_spec, shape) {
        (_, Some((w, h))) => (w, h),
        (Some(p), None) => page_size(p, false)?,
        (None, None) => set.page.unwrap_or(page_size("a1", false)?),
    };
    if opts.portrait {
        (page_w, page_h) = (page_w.min(page_h), page_w.max(page_h));
    }
    if opts.landscape {
        (page_w, page_h) = (page_w.max(page_h), page_w.min(page_h));
    }
    if shape.is_some() {
        shape = Some((page_w, page_h));
    }
    let page_fixed = opts.portrait
        || opts.landscape
        || shape.is_some()
        || page_spec.as_ref().map_or(set.page_fixed, |p| {
            sema::paper(&p.to_ascii_lowercase()).is_none()
        });
    let mut anchor = None;
    if let Some(path) = &opts.lock
        && let Ok(text) = std::fs::read_to_string(path)
    {
        let a = layout::Anchor::parse(&g, &text).map_err(|e| format!("{path}: {e}"))?;
        if !page_fixed && a.page == (page_h, page_w) {
            (page_w, page_h) = (page_h, page_w);
        }
        if shape.is_some_and(|(w, h)| ((a.page.0 / a.page.1) / (w / h) - 1.0).abs() < 0.005) {
            (page_w, page_h) = a.page;
        }
        if a.page == (page_w, page_h) {
            anchor = Some(a);
        } else {
            show(&[Diag::info(format!(
                "{path} was made for a {:.0}x{:.0}pt page; laying out afresh",
                a.page.0, a.page.1
            ))]);
        }
    }
    let font = set.font.unwrap_or((9.0, 28.0));
    let budget = opts.budget;
    let draft = opts.draft;
    let mut cfg = Config {
        page_w,
        page_h,
        margin: opts.margin.or(set.margin).unwrap_or(24.0),
        min_font: opts.min_font.unwrap_or(font.0),
        max_font: opts.max_font.unwrap_or(font.1),
        wrap_em: opts.wrap.or(set.wrap_em).unwrap_or(24.0),
        budget,
        opt_budget: budget / if draft { 32 } else { 16 },
        first_cap: budget / if draft { 20 } else { 4 },
        legible: layout::legible(page_w, page_h, opts.min_font.unwrap_or(font.0)),
        draft,
        key_h: 0.0,
        pack_frames: false,
        anchor: anchor.map(|a| &*Box::leak(Box::new(a))),
    };

    let t = tree::build(&g);
    if g.settings.layout.unwrap_or(LayoutKind::Radial) == LayoutKind::Radial {
        show(&t.notes);
    }
    let fonts = metrics::Fonts::load(opts.family.as_deref().or(set.family.as_deref()))?;
    show(
        &fonts
            .notes
            .iter()
            .map(|n| Diag::info(n.clone()))
            .collect::<Vec<_>>(),
    );
    let name = opts
        .theme
        .as_deref()
        .or(set.theme.as_deref())
        .unwrap_or(theme::PRESETS[0]);
    let th = theme::Theme::preset(name)
        .ok_or(format!(
            "unknown theme `{name}` (expected one of: {})",
            theme::PRESETS.join(", ")
        ))?
        .with(&set.theme_over);
    layout::measure_groups(&mut g, &fonts);
    if let Some(r) = shape
        && cfg.anchor.is_none()
    {
        (cfg.page_w, cfg.page_h) = layout::fit_ratio(&mut g, &t, &fonts, cfg, r);
        cfg.legible = layout::legible(cfg.page_w, cfg.page_h, cfg.min_font);
        let (w, h) = (cfg.page_w, cfg.page_h);
        show(&[Diag::info(format!(
            "page {}:{} fits on {w:.0}x{h:.0}pt ({:.0}x{:.0}mm)",
            layout::num_text(r.0),
            layout::num_text(r.1),
            w / 72.0 * 25.4,
            h / 72.0 * 25.4
        ))]);
    }
    let plan_key = |size: f64| {
        key::KeyPlan::new(
            &g.key,
            &fonts,
            cfg.page_w - 2.0 * cfg.margin,
            cfg.margin,
            size,
        )
    };
    let mut key = plan_key(
        g.settings
            .key_size
            .unwrap_or(layout::KEY_RATIO * cfg.min_font),
    );
    cfg.key_h = key.as_ref().map_or(0.0, key::KeyPlan::height);
    let mut lay = match layout::solve(&g, &t, &fonts, &cfg) {
        Ok(lay) if key_auto && key.is_some() && lay.body > 1.02 * cfg.min_font => {
            let wider = plan_key(layout::KEY_RATIO * lay.body);
            let mut c2 = cfg;
            c2.key_h = wider.as_ref().map_or(0.0, key::KeyPlan::height);
            match layout::solve(&g, &t, &fonts, &c2) {
                Ok(l2) => {
                    (key, cfg) = (wider, c2);
                    l2
                }
                Err(_) => lay,
            }
        }
        Ok(lay) => lay,
        Err(Failure::NoRoom { crowded, cut_off }) => {
            return Err(Stop::Failed(no_room(
                &g, &t, &fonts, &cfg, crowded, cut_off,
            )));
        }
        Err(Failure::Core { core, cut_off }) => {
            show(&[unsat(&g, &cfg, opts, &core, cut_off)]);
            return Err(Stop::Failed(String::new()));
        }
    };
    let mut diags = checks::check(&g, &fonts, &cfg, &lay);
    if !diags.is_empty() && !draft && cfg.anchor.is_none() {
        let (cfg2, notes) = layout::retry(&g, &t, &fonts, cfg, page_fixed, &mut lay, &mut diags);
        cfg = cfg2;
        show(&notes);
    }
    if let Some(&(top, _)) = lay.tradeoff.iter().max_by(|a, b| a.0.total_cmp(&b.0))
        && top > lay.body + 0.05
    {
        diags.push(Diag::info(format!("chose {:.1}pt over {top:.1}pt, which fits but reads worse (crossings, spacing); `--min-font {top:.1}` keeps the larger", lay.body)));
    }
    Ok(Compiled {
        src,
        graph: g,
        tree: t,
        fonts,
        theme: th,
        cfg,
        layout: lay,
        key,
        shape,
        diags,
    })
}

fn no_room(
    g: &Graph,
    t: &tree::Tree,
    fonts: &metrics::Fonts,
    cfg: &Config,
    crowded: Option<(usize, f64, f64)>,
    cut_off: Option<u64>,
) -> String {
    let mut out = format!(
        "error: no layout fits at the {}pt font floor on a {:.0}x{:.0}pt page, even ignoring `require` constraints\n",
        cfg.min_font, cfg.page_w, cfg.page_h
    );
    match cut_off {
        Some(n) => {
            out += &format!(
                "  = note: the search was cut off after {n} checks, so a layout may still exist; try `--budget {}`\n",
                cfg.budget * 4
            )
        }
        None => out += "  = note: proved for every candidate position this layout considers\n",
    }
    match (crowded, g.settings.layout) {
        (Some((layers, need, have)), Some(LayoutKind::Layered(dir))) => {
            let (along, other) = if dir == graph::Dir::Down {
                ("tall", "right")
            } else {
                ("wide", "down")
            };
            out += &format!(
                "  = note: the longest path has {layers} layers, which need {need:.0}pt along the flow; the page gives {have:.0}pt\n"
            );
            out += &format!(
                "  = help: use `layout layered [dir={other}]`, a page that is more {along}, or fewer steps on the longest path\n"
            );
        }
        _ => {
            out += "  = help: use a larger page, a lower font floor (`font 6pt..30pt`), or a narrower `wrap`\n"
        }
    }
    if let Some((b, n, share)) = layout::largest_branch(g, t, fonts, cfg) {
        out += &format!(
            "  = help: or split the map: the branch under `{}` ({n} nodes, {:.0}% of the box area at {}pt) could become its own diagram, leaving one stub node here; `fitd split` makes one page per branch\n",
            g.nodes[b].id,
            100.0 * share,
            cfg.min_font
        );
    }
    out
}

fn unsat(g: &Graph, cfg: &Config, opts: &Options, core: &[usize], cut_off: Option<u64>) -> Diag {
    let what = if core.len() == 1 {
        "this constraint cannot hold".to_string()
    } else {
        format!("these {} constraints cannot all hold", core.len())
    };
    let mut d = Diag::error(
        "E0110",
        format!("{what} at {}pt on this page", cfg.min_font),
    );
    for (k, &ci) in core.iter().enumerate() {
        let span = g.constraints[ci].span;
        d = if k == 0 {
            d.primary(span, "")
        } else {
            d.secondary(span, "")
        };
    }
    let d = match (opts.min_font, g.settings.font_span) {
        (Some(_), _) => d.note("the font floor is set by `--min-font`"),
        (None, Some(span)) => d.secondary(span, "the font floor is set here"),
        (None, None) => d.note(format!(
            "{}pt is the default font floor; a `font` statement can lower it",
            cfg.min_font
        )),
    };
    let d = if core.len() == 1 {
        d.note("it fails even with every other `require` removed")
    } else {
        d.note("drop any one of them and the rest can be satisfied")
    };
    let d = match cut_off {
        Some(n) => d.note(format!("the search was cut off after {n} checks, so this may be an artifact of the budget; try `--budget {}`", cfg.budget * 4)),
        None => d.note("proved for every candidate position this layout considers"),
    };
    let d = if g.settings.layout.unwrap_or(LayoutKind::Radial) == LayoutKind::Radial {
        d.note("radial layout keeps each branch within 30 degrees of its wedge; try `layout layered` for free placement")
    } else {
        d
    };
    d.help("remove one, turn it into `prefer`, or use a larger page")
}

/// load_images reads each `~ image` file, relative to the input file's folder.
fn load_images(g: &mut Graph, input: &str) -> Vec<Diag> {
    let base = match input {
        "<stdin>" => std::path::Path::new(""),
        _ => std::path::Path::new(input)
            .parent()
            .unwrap_or(std::path::Path::new("")),
    };
    let mut diags = Vec::new();
    for c in g
        .nodes
        .iter_mut()
        .flat_map(|n| n.figure.iter_mut().flatten())
    {
        let Some(im) = c.image.as_mut().and_then(std::sync::Arc::get_mut) else {
            continue;
        };
        let file = base.join(&im.path);
        let err = match std::fs::read(&file) {
            Ok(data) => im.decode(data).err().map(String::from),
            Err(e) => Some(format!("cannot read `{}`: {e}", file.display())),
        };
        if let Some(e) = err {
            diags.push(
                Diag::error("E0014", e)
                    .primary(im.span, "image not loaded")
                    .help("paths are relative to the main file; use a PNG or JPEG"),
            );
        }
    }
    diags
}
