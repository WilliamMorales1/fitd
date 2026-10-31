mod ast;
mod check;
mod csp;
mod diag;
mod emit;
#[cfg(test)]
mod fuzz;
mod geom;
mod graph;
mod layered;
mod layout;
mod lex;
mod metrics;
mod parse;
mod quality;
mod radial;
mod sema;
mod theme;
mod tree;

use diag::{Diag, Level, Source};
use geom::Rect;
use graph::LayoutKind;
use layout::{Config, Failure};
use std::process::ExitCode;

const USAGE: &str = "usage:
  cmapc <input[.cmap]|-> [-o out.svg|out.png] [--tikz out.tex] [--page a0..a4|WxH|WxHmm]
              [--portrait] [--margin PT] [--min-font PT] [--max-font PT] [--wrap EM] [--force] [--draft]
              [--short] [--budget CHECKS] [--scale PX] [--theme light|dark|mono] [--font-family NAME]
  cmapc explain <CODE>
  cmapc guide            the whole language on one page, with examples";

/// GUIDE is the language reference sized for an agent's context: everything needed to write a
/// diagram, in a few hundred tokens.
const GUIDE: &str = include_str!("guide.txt");

fn page(spec: &str, portrait: bool) -> Result<(f64, f64), String> {
    let (w, h) = sema::page_size(&spec.to_ascii_lowercase(), Some("pt"))?;
    Ok(if portrait { (w.min(h), w.max(h)) } else { (w, h) })
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    match args.first().map(String::as_str) {
        Some("explain") => {
            let code = args.get(1).ok_or(USAGE)?.to_uppercase();
            let (_, text) = check::EXPLAIN.iter().find(|(c, _)| *c == code).ok_or(format!("unknown code {code}"))?;
            println!("{code}: {text}");
            Ok(ExitCode::SUCCESS)
        }
        Some("guide") => {
            print!("{GUIDE}");
            Ok(ExitCode::SUCCESS)
        }
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        Some(_) => build(args),
        None => Err(USAGE.into()),
    }
}

fn build(args: &[String]) -> Result<ExitCode, String> {
    let mut input = None;
    let (mut svg_out, mut tikz_out) = (None, None);
    let (mut page_spec, mut portrait, mut force, mut draft, mut short) = (None, false, false, false, false);
    let (mut margin, mut min_font, mut max_font, mut wrap) = (None, None, None, None);
    let mut budget = 20_000_000;
    let mut scale = 2.0;
    let (mut theme_name, mut family) = (None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or(format!("{a} needs a value"));
        let num = |s: String| s.parse::<f64>().map_err(|_| format!("bad number `{s}`"));
        match a.as_str() {
            "-o" => svg_out = Some(val()?),
            "--tikz" => tikz_out = Some(val()?),
            "--page" => page_spec = Some(val()?),
            "--portrait" => portrait = true,
            "--force" => force = true,
            "--draft" => draft = true,
            "--short" => short = true,
            "--margin" => margin = Some(num(val()?)?),
            "--min-font" => min_font = Some(num(val()?)?),
            "--max-font" => max_font = Some(num(val()?)?),
            "--wrap" => wrap = Some(num(val()?)?),
            "--budget" => budget = num(val()?)? as u64,
            "--scale" => scale = num(val()?)?,
            "--theme" => theme_name = Some(val()?),
            "--font-family" => family = Some(val()?),
            s if (s == "-" || !s.starts_with('-')) && input.is_none() => input = Some(s.to_string()),
            s => return Err(format!("unknown argument `{s}`\n{USAGE}")),
        }
    }
    let mut input = input.ok_or(USAGE)?;
    if !std::path::Path::new(&input).exists() && std::path::Path::new(&format!("{input}.cmap")).exists() {
        input.push_str(".cmap");
    }
    let text = if input == "-" {
        if svg_out.is_none() {
            return Err("reading the diagram from stdin needs -o".into());
        }
        input = "<stdin>".into();
        std::io::read_to_string(std::io::stdin()).map_err(|e| format!("stdin: {e}"))?
    } else {
        std::fs::read_to_string(&input).map_err(|e| format!("{input}: {e}"))?
    };
    let src = Source::new(&input, &text);
    let show = |diags: &[Diag]| eprint!("{}", if short { diag::render_short(&src, diags) } else { diag::render(&src, diags) });
    let (stmts, mut diags) = parse::parse(&text);
    let (mut g, sema_diags) = sema::analyze(&stmts);
    diags.extend(sema_diags);
    diags.sort_by_key(Diag::pos);
    show(&diags);
    if diags.iter().any(|d| d.level == Level::Error) {
        return Ok(ExitCode::FAILURE);
    }

    // Command-line flags override settings written in the file.
    let set = &g.settings;
    let (mut page_w, mut page_h) = match &page_spec {
        Some(p) => page(p, false)?,
        None => set.page.unwrap_or(page("a1", false)?),
    };
    if portrait {
        (page_w, page_h) = (page_w.min(page_h), page_w.max(page_h));
    }
    let font = set.font.unwrap_or((7.0, 48.0));
    let cfg = Config {
        page_w,
        page_h,
        margin: margin.or(set.margin).unwrap_or(24.0),
        min_font: min_font.unwrap_or(font.0),
        max_font: max_font.unwrap_or(font.1),
        wrap_em: wrap.or(set.wrap_em).unwrap_or(24.0),
        budget,
        opt_budget: budget / if draft { 32 } else { 16 },
        legible: legible(page_w, page_h, min_font.unwrap_or(font.0)),
        draft,
    };

    let t = tree::build(&g);
    if g.settings.layout.unwrap_or(LayoutKind::Radial) == LayoutKind::Radial {
        show(&t.notes);
    }
    let fonts = metrics::Fonts::load(family.as_deref().or(set.family.as_deref()))?;
    let name = theme_name.as_deref().or(set.theme.as_deref()).unwrap_or("light");
    let th = theme::Theme::preset(name).ok_or(format!("unknown theme `{name}` (expected one of: {})", theme::PRESETS.join(", ")))?.with(&set.theme_over);
    layout::measure_groups(&mut g, &fonts);
    let lay = match layout::solve(&g, &t, &fonts, &cfg) {
        Ok(lay) => lay,
        Err(Failure::NoRoom(crowded)) => {
            eprintln!("error: no layout fits at the {}pt font floor on a {:.0}x{:.0}pt page, even ignoring `require` constraints", cfg.min_font, cfg.page_w, cfg.page_h);
            match (crowded, g.settings.layout) {
                (Some((layers, need, have)), Some(LayoutKind::Layered(dir))) => {
                    let (along, other) = if dir == graph::Dir::Down { ("tall", "right") } else { ("wide", "down") };
                    eprintln!("  = note: the longest path has {layers} layers, which need {need:.0}pt along the flow; the page gives {have:.0}pt");
                    eprintln!("  = help: use `layout layered dir={other}`, a page that is more {along}, or fewer steps on the longest path");
                }
                _ => eprintln!("  = help: use a larger page, a lower font floor (`font 6pt..30pt`), or a narrower `wrap`"),
            }
            return Ok(ExitCode::FAILURE);
        }
        Err(Failure::Core(core)) => {
            let what = if core.len() == 1 { "this constraint cannot hold".to_string() } else { format!("these {} constraints cannot all hold", core.len()) };
            let mut d = Diag::error("E0110", format!("{what} at {}pt on this page", cfg.min_font));
            for (k, &ci) in core.iter().enumerate() {
                let span = g.constraints[ci].span;
                d = if k == 0 { d.primary(span, "") } else { d.secondary(span, "") };
            }
            // Say where the floor came from, since a smaller font is often the easiest fix.
            let d = match (min_font, g.settings.font_span) {
                (Some(_), _) => d.note("the font floor is set by `--min-font`"),
                (None, Some(span)) => d.secondary(span, "the font floor is set here"),
                (None, None) => d.note(format!("{}pt is the default font floor; a `font` statement can lower it", cfg.min_font)),
            };
            let d = if core.len() == 1 {
                d.note("it fails even with every other `require` removed")
            } else {
                d.note("drop any one of them and the rest can be satisfied (within the search budget)")
            };
            let d = if g.settings.layout.unwrap_or(LayoutKind::Radial) == LayoutKind::Radial {
                d.note("radial layout keeps each branch within 30 degrees of its wedge; try `layout layered` for free placement")
            } else {
                d
            };
            show(&[d.help("remove one, turn it into `prefer`, or use a larger page")]);
            return Ok(ExitCode::FAILURE);
        }
    };
    let diags = check::check(&g, &fonts, &cfg, &lay);
    show(&diags);
    let errors = diags.iter().filter(|d| d.level == Level::Error).count();
    let warnings = diags.len() - errors;
    let s = lay.stats;
    if short {
        eprintln!("body font {:.1}pt on {:.0}x{:.0}pt; {errors} errors, {warnings} warnings", lay.body, cfg.page_w, cfg.page_h);
    } else {
        eprintln!(
            "body font {:.1}pt (titles {:.1}pt) on {:.0}x{:.0}pt; {} nodes, {} edges; {errors} errors, {warnings} warnings; solver: {} attempts, {} nodes, {} checks, {} backjumps{}",
            lay.body,
            1.2 * lay.body,
            cfg.page_w,
            cfg.page_h,
            g.nodes.len(),
            lay.edges.len(),
            s.attempts,
            s.search.nodes,
            s.search.checks,
            s.search.backjumps,
            if s.soft_total > 0 { format!("; soft cost {}/{}", s.soft_cost, s.soft_total) } else { String::new() }
        );
        let rects: Vec<Rect> = (0..g.nodes.len()).map(|v| lay.rect(v)).collect();
        let q = layout::Readable::new(&t, lay.kind, &lay.edges, cfg.page(), cfg.legible).score(&rects);
        eprintln!(
            "readability penalty {:.0}: crossings {:.0}, proximity {:.0}, grouping {:.0}, angles {:.0}, alignment {:.0}, spread {:.0}",
            q.total(),
            q.crossings,
            q.proximity,
            q.grouping,
            q.angles,
            q.alignment,
            q.spread
        );
        if lay.tradeoff.len() > 1 {
            let tried: Vec<String> = lay.tradeoff.iter().map(|(f, m)| format!("{f:.1}pt {m:.0}{}", if *f == lay.body { " (chosen)" } else { "" })).collect();
            eprintln!("font trade-off, merit (lower is better; legible from {:.1}pt): {}", cfg.legible, tried.join(", "));
        }
    }
    if errors > 0 && !force {
        eprintln!("error: aborting due to {errors} previous errors; no output written (use --force to write anyway)");
        return Ok(ExitCode::FAILURE);
    }
    let stem = input.strip_suffix(".cmap").unwrap_or(&input).to_string();
    let out = svg_out.unwrap_or(format!("{stem}.svg"));
    let bytes = if out.to_ascii_lowercase().ends_with(".png") {
        // A PNG is cropped to the drawing, with the margin as padding, not sized to the page.
        let svg = emit::svg(&g, &t, &fonts, &lay, emit::bounds(&g, &lay).inflate(cfg.margin), &th);
        emit::png(&svg, &fonts, scale)?
    } else {
        emit::svg(&g, &t, &fonts, &lay, Rect::centered((0.0, 0.0), cfg.page_w, cfg.page_h), &th).into_bytes()
    };
    write(&out, &bytes)?;
    eprintln!("wrote {out}");
    if let Some(p) = tikz_out {
        write(&p, emit::tikz(&g, &t, &fonts, &lay, cfg.page_w, cfg.page_h, &th).as_bytes())?;
        eprintln!("wrote {p}");
    }
    Ok(ExitCode::SUCCESS)
}

/// write saves an output file, creating its directory if needed.
fn write(path: &str, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = std::path::Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("{path}: {e}"))
}

/// legible is the smallest comfortable body font for a page read from about its diagonal:
/// 16pt per meter of viewing distance (poster guides give about 24pt at 1.5m), at least 8pt,
/// and never below the user's own minimum.
fn legible(page_w: f64, page_h: f64, min_font: f64) -> f64 {
    let diagonal_m = page_w.hypot(page_h) / 72.0 * 0.0254;
    (16.0 * diagonal_m).max(8.0).max(min_font)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
