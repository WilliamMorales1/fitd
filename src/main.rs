mod ast;
mod check;
mod csp;
mod diag;
mod emit;
#[cfg(test)]
mod fuzz;
mod geom;
mod import;
mod include;
mod graph;
mod layered;
mod layout;
mod lex;
mod lsp;
mod metrics;
mod parse;
mod quality;
mod radial;
mod sema;
mod split;
mod theme;
mod tree;

use diag::{Diag, Level};
use geom::Rect;
use graph::LayoutKind;
use layout::{Config, Failure};
use std::process::ExitCode;

const USAGE: &str = "usage:
  fitd <input[.fitd]|-> [-o out.svg|out.png] [--tikz out.tex] [--page a0..a4|WxH|WxHmm]
              [--portrait|--landscape] [--bands N] [--margin PT] [--min-font PT] [--max-font PT] [--wrap EM] [--force] [--draft]
              [--short] [--budget CHECKS] [--scale PX] [--theme light|dark|mono] [--font-family NAME]
              [--stable LOCK] [--watch]
  fitd explain <CODE>
  fitd lsp              language server on stdin/stdout
  fitd import <file.dot|file.mmd> [-o out.fitd]   translate Graphviz or Mermaid
  fitd split <input.fitd> [-o DIR]   an overview page plus one page per top-level branch
  fitd guide            the whole language on one page, with examples";

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
        Some("lsp") => Ok(lsp::serve()),
        Some("import") => {
            let path = args.get(1).ok_or(USAGE)?;
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            let (fitd, dropped) = import::import(path, &text)?;
            if !dropped.is_empty() {
                eprintln!("warning: no fitd equivalent, dropped: {}", dropped.join("; "));
            }
            match args.iter().position(|a| a == "-o").and_then(|i| args.get(i + 1)) {
                Some(out) => {
                    write(out, fitd.as_bytes())?;
                    eprintln!("wrote {out}");
                }
                None => print!("{fitd}"),
            }
            Ok(ExitCode::SUCCESS)
        }
        Some("split") => {
            let path = args.get(1).filter(|a| !a.starts_with('-')).ok_or(USAGE)?;
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            let stem = std::path::Path::new(path).file_stem().map_or("map".into(), |s| s.to_string_lossy().into_owned());
            let dir = match args.iter().position(|a| a == "-o").and_then(|i| args.get(i + 1)) {
                Some(d) => std::path::PathBuf::from(d),
                None => std::path::Path::new(path).with_file_name(format!("{stem}-pages")),
            };
            let out = split::split(path, &text, &stem)?;
            for d in &out.dropped {
                eprintln!("warning: spans pages, left out: {d}");
            }
            for (name, text) in &out.files {
                let p = dir.join(name);
                write(&p.to_string_lossy(), text.as_bytes())?;
                eprintln!("wrote {}", p.display());
            }
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
        Some(_) if args.iter().any(|a| a == "--watch") => watch(&args.iter().filter(|a| *a != "--watch").cloned().collect::<Vec<_>>()),
        Some(_) => build(args),
        None => Err(USAGE.into()),
    }
}

/// VALUED lists the build flags that take a value, so `watch` can find the input among them.
const VALUED: &[&str] = &["-o", "--tikz", "--page", "--bands", "--margin", "--min-font", "--max-font", "--wrap", "--budget", "--scale", "--theme", "--font-family", "--stable"];

/// How often `watch` looks at the input, and how long the input must stay unchanged before
/// the full build follows the draft.
const POLL: std::time::Duration = std::time::Duration::from_millis(200);
const IDLE: std::time::Duration = std::time::Duration::from_millis(1500);

/// watch rebuilds whenever the input changes: a draft at once, then a full build once the
/// file has been idle for IDLE. Each build runs as a child process, so a change kills the build
/// in progress instead of waiting out a slow full layout. It runs until interrupted.
fn watch(args: &[String]) -> Result<ExitCode, String> {
    let mut it = args.iter();
    let mut input = None;
    let mut outs = Vec::new();
    while let Some(a) = it.next() {
        if VALUED.contains(&a.as_str()) {
            let v = it.next();
            if matches!(a.as_str(), "-o" | "--tikz" | "--stable") {
                outs.extend(v.cloned());
            }
        } else if a == "-" {
            return Err("--watch needs a file, not stdin".into());
        } else if !a.starts_with('-') {
            input = Some(a.clone());
        }
    }
    let mut input = input.ok_or(USAGE)?;
    if !std::path::Path::new(&input).exists() {
        input.push_str(".fitd");
    }
    let exe = std::env::current_exe().map_err(|e| format!("cannot find fitd itself: {e}"))?;
    let full = args.to_vec();
    let mut draft = full.clone();
    draft.push("--draft".into());
    let stamp = || std::fs::metadata(&input).and_then(|m| m.modified()).ok();
    let (mut seen, mut changed, mut full_done) = (None, std::time::Instant::now(), true);
    let mut running: Option<std::process::Child> = None;
    loop {
        if let Some(c) = &mut running
            && !matches!(c.try_wait(), Ok(None))
        {
            running = None;
        }
        let now = stamp();
        let pass = if now != seen {
            (seen, changed, full_done) = (now, std::time::Instant::now(), false);
            if let Some(mut c) = running.take() {
                let _ = c.kill();
                let _ = c.wait();
                // A build killed between writing and renaming leaves its temporary file.
                for o in &outs {
                    let _ = std::fs::remove_file(format!("{o}.fitd-tmp{}", c.id()));
                }
                eprintln!("--- cancelled, {input} changed");
            }
            Some((&draft, "draft"))
        } else if !full_done && running.is_none() && changed.elapsed() >= IDLE {
            full_done = true;
            Some((&full, "full"))
        } else {
            None
        };
        if let Some((a, what)) = pass
            && now.is_some()
        {
            eprintln!("--- {what} build of {input}");
            match std::process::Command::new(&exe).args(a).spawn() {
                Ok(c) => running = Some(c),
                Err(e) => eprintln!("cannot start build: {e}"),
            }
        }
        std::thread::sleep(POLL);
    }
}

fn build(args: &[String]) -> Result<ExitCode, String> {
    let mut input = None;
    let (mut svg_out, mut tikz_out) = (None, None);
    let (mut page_spec, mut portrait, mut landscape, mut force, mut draft, mut short) = (None, false, false, false, false, false);
    let mut bands = None;
    let (mut margin, mut min_font, mut max_font, mut wrap) = (None, None, None, None);
    let mut budget = 20_000_000;
    let mut scale = 2.0;
    let (mut theme_name, mut family, mut lock) = (None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or(format!("{a} needs a value"));
        let num = |s: String| s.parse::<f64>().map_err(|_| format!("bad number `{s}`"));
        match a.as_str() {
            "-o" => svg_out = Some(val()?),
            "--tikz" => tikz_out = Some(val()?),
            "--page" => page_spec = Some(val()?),
            "--portrait" => portrait = true,
            "--landscape" => landscape = true,
            "--bands" => bands = Some(val()?.parse::<usize>().ok().filter(|&k| k > 0).ok_or("--bands needs a whole number, 1 or more")?),
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
            "--stable" => lock = Some(val()?),
            s if (s == "-" || !s.starts_with('-')) && input.is_none() => input = Some(s.to_string()),
            s => return Err(format!("unknown argument `{s}`\n{USAGE}")),
        }
    }
    let mut input = input.ok_or(USAGE)?;
    if !std::path::Path::new(&input).exists() && std::path::Path::new(&format!("{input}.fitd")).exists() {
        input.push_str(".fitd");
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
    let (src, mut diags) = include::expand(&input, &text);
    let show = |diags: &[Diag]| eprint!("{}", if short { diag::render_short(&src, diags) } else { diag::render(&src, diags) });
    let (stmts, parse_diags) = parse::parse(&src.text);
    diags.extend(parse_diags);
    let (mut g, sema_diags) = sema::analyze(&stmts);
    diags.extend(sema_diags);
    diags.retain(|d| !include::unused_shared_style(&src, d));
    diags.sort_by_key(Diag::pos);
    show(&diags);
    if diags.iter().any(|d| d.level == Level::Error) {
        return Ok(ExitCode::FAILURE);
    }

    // Command-line flags override settings written in the file.
    if bands.is_some() {
        g.settings.bands = bands;
    }
    let set = &g.settings;
    let (mut page_w, mut page_h) = match &page_spec {
        Some(p) => page(p, false)?,
        None => set.page.unwrap_or(page("a1", false)?),
    };
    if portrait {
        (page_w, page_h) = (page_w.min(page_h), page_w.max(page_h));
    }
    if landscape {
        (page_w, page_h) = (page_w.max(page_h), page_w.min(page_h));
    }
    let page_fixed = portrait || landscape || page_spec.as_ref().map_or(set.page_fixed, |p| sema::paper(&p.to_ascii_lowercase()).is_none());
    let mut anchor = None;
    if let Some(path) = &lock
        && let Ok(text) = std::fs::read_to_string(path)
    {
        let a = layout::Anchor::parse(&g, &text).map_err(|e| format!("{path}: {e}"))?;
        // An open orientation follows the lock, so a page turned last time stays turned.
        if !page_fixed && a.page == (page_h, page_w) {
            (page_w, page_h) = (page_h, page_w);
        }
        if a.page == (page_w, page_h) {
            anchor = Some(a);
        } else {
            show(&[Diag::info(format!("{path} was made for a {:.0}x{:.0}pt page; laying out afresh", a.page.0, a.page.1))]);
        }
    }
    let font = set.font.unwrap_or((7.0, 48.0));
    let mut cfg = Config {
        page_w,
        page_h,
        margin: margin.or(set.margin).unwrap_or(24.0),
        min_font: min_font.unwrap_or(font.0),
        max_font: max_font.unwrap_or(font.1),
        wrap_em: wrap.or(set.wrap_em).unwrap_or(24.0),
        budget,
        opt_budget: budget / if draft { 32 } else { 16 },
        // A draft gives up sooner on the first, usually too large, fonts.
        first_cap: budget / if draft { 20 } else { 4 },
        legible: legible(page_w, page_h, min_font.unwrap_or(font.0)),
        draft,
        // Leaked: the anchor lives for the whole run, and a static keeps Config Copy.
        anchor: anchor.map(|a| &*Box::leak(Box::new(a))),
    };

    let t = tree::build(&g);
    if g.settings.layout.unwrap_or(LayoutKind::Radial) == LayoutKind::Radial {
        show(&t.notes);
    }
    let fonts = metrics::Fonts::load(family.as_deref().or(set.family.as_deref()))?;
    show(&fonts.notes.iter().map(|n| Diag::info(n.clone())).collect::<Vec<_>>());
    let name = theme_name.as_deref().or(set.theme.as_deref()).unwrap_or("light");
    let th = theme::Theme::preset(name).ok_or(format!("unknown theme `{name}` (expected one of: {})", theme::PRESETS.join(", ")))?.with(&set.theme_over);
    layout::measure_groups(&mut g, &fonts);
    let mut lay = match layout::solve(&g, &t, &fonts, &cfg) {
        Ok(lay) => lay,
        Err(Failure::NoRoom { crowded, cut_off }) => {
            eprintln!("error: no layout fits at the {}pt font floor on a {:.0}x{:.0}pt page, even ignoring `require` constraints", cfg.min_font, cfg.page_w, cfg.page_h);
            match cut_off {
                Some(n) => eprintln!("  = note: the search was cut off after {n} checks, so a layout may still exist; try `--budget {}`", budget * 4),
                None => eprintln!("  = note: proved for every candidate position this layout considers"),
            }
            match (crowded, g.settings.layout) {
                (Some((layers, need, have)), Some(LayoutKind::Layered(dir))) => {
                    let (along, other) = if dir == graph::Dir::Down { ("tall", "right") } else { ("wide", "down") };
                    eprintln!("  = note: the longest path has {layers} layers, which need {need:.0}pt along the flow; the page gives {have:.0}pt");
                    eprintln!("  = help: use `layout layered dir={other}`, a page that is more {along}, or fewer steps on the longest path");
                }
                _ => eprintln!("  = help: use a larger page, a lower font floor (`font 6pt..30pt`), or a narrower `wrap`"),
            }
            if let Some((b, n, share)) = layout::largest_branch(&g, &t, &fonts, &cfg) {
                eprintln!(
                    "  = help: or split the map: the branch under `{}` ({n} nodes, {:.0}% of the box area at {}pt) could become its own diagram, leaving one stub node here; `fitd split` makes one page per branch",
                    g.nodes[b].id,
                    100.0 * share,
                    cfg.min_font
                );
            }
            return Ok(ExitCode::FAILURE);
        }
        Err(Failure::Core { core, cut_off }) => {
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
                d.note("drop any one of them and the rest can be satisfied")
            };
            let d = match cut_off {
                Some(n) => d.note(format!("the search was cut off after {n} checks, so this may be an artifact of the budget; try `--budget {}`", budget * 4)),
                None => d.note("proved for every candidate position this layout considers"),
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
    let mut diags = check::check(&g, &fonts, &cfg, &lay);
    // Retry variants reorder branches or turn the page, which would undo the lock.
    if !diags.is_empty() && !draft && cfg.anchor.is_none() {
        let (cfg2, notes) = retry(&g, &t, &fonts, cfg, page_fixed, &mut lay, &mut diags);
        cfg = cfg2;
        show(&notes);
    }
    // The largest font that fits is not always kept; say so, since the log line is easy to miss.
    if let Some(&(top, _)) = lay.tradeoff.iter().max_by(|a, b| a.0.total_cmp(&b.0))
        && top > lay.body + 0.05
    {
        diags.push(Diag::info(format!("chose {:.1}pt over {top:.1}pt, which fits but reads worse (crossings, spacing); `--min-font {top:.1}` keeps the larger", lay.body)));
    }
    show(&diags);
    let errors = diags.iter().filter(|d| d.level == Level::Error).count();
    let warnings = diags.iter().filter(|d| d.level == Level::Warning).count();
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
    let stem = input.strip_suffix(".fitd").unwrap_or(&input).to_string();
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
    if let Some(p) = lock {
        write(&p, layout::Anchor::render(&g, &lay, &cfg).as_bytes())?;
        if let Some(a) = cfg.anchor {
            let kept = (0..g.nodes.len()).filter(|&v| a.pos[v].is_some_and(|q| (q.0 - lay.pos[v].0).hypot(q.1 - lay.pos[v].1) <= 0.01)).count();
            eprintln!("wrote {p} ({kept} of {} locked nodes kept their spots)", a.pos.iter().flatten().count());
        } else {
            eprintln!("wrote {p}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// retry rebuilds a layout that has warnings with the radial branches reordered toward their
/// links and, when the file leaves the orientation open, on the turned page. The variants run
/// in parallel; the one with the fewest diagnostics wins, then the best merit. Returns the
/// winner's config and a note saying what changed.
fn retry(g: &graph::Graph, t: &tree::Tree, fonts: &metrics::Fonts, cfg: Config, page_fixed: bool, lay: &mut layout::Layout, diags: &mut Vec<Diag>) -> (Config, Vec<Diag>) {
    let arranged = if lay.kind == LayoutKind::Radial { radial::arrange(g, t, &cfg) } else { None };
    // The first build already found about where the font lands, so the variants give up on
    // fonts far above it as soon as a draft would.
    let quick = Config { first_cap: cfg.budget / 20, ..cfg };
    let turned = Config { page_w: cfg.page_h, page_h: cfg.page_w, ..quick };
    let mut tries: Vec<(&tree::Tree, Config, String)> = Vec::new();
    if let Some(a) = &arranged {
        let moved: Vec<String> = (0..t.children.len()).filter(|&v| a.children[v] != t.children[v]).map(|v| format!("`{}`", g.nodes[v].id)).collect();
        tries.push((a, quick, format!("reordered the branches under {}", moved.join(", "))));
    }
    if !page_fixed && cfg.page_w != cfg.page_h {
        tries.push((t, turned, format!("turned the page to {}", if turned.page_w < turned.page_h { "portrait" } else { "landscape" })));
    }
    let runs: Vec<_> = std::thread::scope(|s| {
        let handles: Vec<_> = tries.iter().map(|(tr, c, _)| s.spawn(move || layout::solve(g, tr, fonts, c).ok())).collect();
        handles.into_iter().map(|h| h.join().expect("retry thread")).collect()
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
    lay.stats = layout::Stats { soft_cost: soft.0, soft_total: soft.1, ..stats };
    match win {
        Some((k, now)) => {
            let note = Diag::info(format!("{} (warnings: {before} -> {now})", tries[k].2));
            (tries[k].1, vec![note])
        }
        None => (cfg, Vec::new()),
    }
}

/// write saves an output file, creating its directory if needed.
fn write(path: &str, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = std::path::Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    // Through a temporary file and a rename, so a viewer never reads half an image.
    let tmp = format!("{path}.fitd-tmp{}", std::process::id());
    let renamed = std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, path));
    // Windows refuses to replace a file another program holds open, as viewers often do.
    #[cfg(windows)]
    let renamed = renamed.or_else(|_| std::fs::write(path, bytes).and_then(|_| std::fs::remove_file(&tmp)));
    renamed.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{path}: {e}")
    })
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
