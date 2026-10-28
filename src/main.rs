mod ast;
mod bench;
mod check;
mod csp;
mod diag;
mod emit;
mod geom;
mod graph;
mod layered;
mod layout;
mod lex;
mod metrics;
mod parse;
mod radial;
mod sema;
mod tree;

use diag::{Diag, Level, Source};
use graph::LayoutKind;
use layout::{Config, Failure};
use std::process::ExitCode;

const USAGE: &str = "usage:
  cmapc build <input.cmap> [-o out.svg] [--tikz out.tex] [--page a0..a4|WxH|WxHmm]
              [--portrait] [--margin PT] [--min-font PT] [--max-font PT] [--wrap EM] [--force]
              [--algo bt|cbj|fc|fc-cbj|mac|mac-cbj[+dom|+wdeg][+learn|+restart]] [--budget CHECKS]
  cmapc bench [--budget CHECKS]                 every algorithm on classic CSPs
  cmapc bench <input.cmap> --font PT [flags]    every algorithm on one diagram at one font
  cmapc explain <CODE>";

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
        Some("build") => build(&args[1..], false),
        Some("bench") if args.get(1).is_none_or(|a| a.starts_with('-')) => {
            let budget = match args.get(1).map(String::as_str) {
                Some("--budget") => args.get(2).and_then(|b| b.parse().ok()).ok_or("--budget needs a number")?,
                _ => 50_000_000,
            };
            print!("{}", bench::table(&bench::classic(budget)));
            Ok(ExitCode::SUCCESS)
        }
        Some("bench") => build(&args[1..], true),
        _ => Err(USAGE.into()),
    }
}

/// build compiles a diagram; with `bench` it instead times every algorithm at `--font`.
fn build(args: &[String], bench: bool) -> Result<ExitCode, String> {
    let mut input = None;
    let (mut svg_out, mut tikz_out) = (None, None);
    let (mut page_spec, mut portrait, mut force) = (None, false, false);
    let (mut margin, mut min_font, mut max_font, mut wrap) = (None, None, None, None);
    let (mut algo, mut budget, mut bench_font) = (csp::Algo::FC_CBJ, 20_000_000, None);
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
            "--margin" => margin = Some(num(val()?)?),
            "--min-font" => min_font = Some(num(val()?)?),
            "--max-font" => max_font = Some(num(val()?)?),
            "--wrap" => wrap = Some(num(val()?)?),
            "--algo" => {
                let v = val()?;
                algo = csp::Algo::parse(&v).ok_or(format!("unknown --algo `{v}` (bt, cbj, fc, fc-cbj, mac, mac-cbj, then optionally +dom or +wdeg, then +learn or +restart on a -cbj base)"))?;
            }
            "--budget" => budget = num(val()?)? as u64,
            "--font" if bench => bench_font = Some(num(val()?)?),
            s if !s.starts_with('-') && input.is_none() => input = Some(s.to_string()),
            s => return Err(format!("unknown argument `{s}`\n{USAGE}")),
        }
    }
    let input = input.ok_or(USAGE)?;
    let text = std::fs::read_to_string(&input).map_err(|e| format!("{input}: {e}"))?;
    let src = Source::new(&input, &text);
    let (stmts, mut diags) = parse::parse(&text);
    let (g, sema_diags) = sema::analyze(&stmts);
    diags.extend(sema_diags);
    diags.sort_by_key(Diag::pos);
    eprint!("{}", diag::render(&src, &diags));
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
        opt_budget: budget / 4,
        algo,
    };

    let t = tree::build(&g);
    if g.settings.layout.unwrap_or(LayoutKind::Radial) == LayoutKind::Radial {
        eprint!("{}", diag::render(&src, &t.notes));
    }
    let fonts = metrics::Fonts::load()?;
    if bench {
        let b = bench_font.ok_or("bench needs --font PT")?;
        let mut rows = Vec::new();
        for &name in bench::ALGOS {
            let cfg = Config { algo: csp::Algo::parse(name).unwrap(), ..cfg };
            let mut stats = layout::Stats::default();
            let t0 = std::time::Instant::now();
            let ok = layout::probe(&g, &t, &fonts, &cfg, b, &mut stats);
            let outcome = if ok { "sat" } else if stats.search.checks > cfg.budget { "budget" } else { "unsat" };
            rows.push(bench::Row { problem: format!("{input}@{b}pt"), algo: name.into(), outcome, stats: stats.search, ms: t0.elapsed().as_secs_f64() * 1e3 });
        }
        print!("{}", bench::table(&rows));
        return Ok(ExitCode::SUCCESS);
    }
    let lay = match layout::solve(&g, &t, &fonts, &cfg) {
        Ok(lay) => lay,
        Err(Failure::NoRoom) => {
            eprintln!(
                "error: no layout fits at --min-font {}pt on a {:.0}x{:.0}pt page, even ignoring `require` constraints\n  = help: use a larger --page, a smaller --min-font, or a narrower --wrap",
                cfg.min_font, cfg.page_w, cfg.page_h
            );
            return Ok(ExitCode::FAILURE);
        }
        Err(Failure::Core(core)) => {
            let what = if core.len() == 1 { "this constraint cannot hold".to_string() } else { format!("these {} constraints cannot all hold", core.len()) };
            let mut d = Diag::error("E0110", format!("{what} at {}pt on this page", cfg.min_font));
            for (k, &ci) in core.iter().enumerate() {
                let span = g.constraints[ci].span;
                d = if k == 0 { d.primary(span, "") } else { d.secondary(span, "") };
            }
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
            eprint!("{}", diag::render(&src, &[d.help("remove one, turn it into `prefer`, or use a larger page")]));
            return Ok(ExitCode::FAILURE);
        }
    };
    let diags = check::check(&g, &fonts, &cfg, &lay);
    eprint!("{}", diag::render(&src, &diags));
    let errors = diags.iter().filter(|d| d.level == Level::Error).count();
    let warnings = diags.len() - errors;
    let s = lay.stats;
    eprintln!(
        "body font {:.1}pt (titles {:.1}pt) on {:.0}x{:.0}pt; {} nodes, {} edges; {errors} errors, {warnings} warnings; solver {}: {} attempts, {} nodes, {} checks, {} backjumps{}",
        lay.body,
        1.2 * lay.body,
        cfg.page_w,
        cfg.page_h,
        g.nodes.len(),
        lay.edges.len(),
        cfg.algo.name(),
        s.attempts,
        s.search.nodes,
        s.search.checks,
        s.search.backjumps,
        if s.soft_total > 0 { format!("; soft cost {}/{}", s.soft_cost, s.soft_total) } else { String::new() }
    );
    if errors > 0 && !force {
        eprintln!("error: aborting due to {errors} previous errors; no output written (use --force to write anyway)");
        return Ok(ExitCode::FAILURE);
    }
    let stem = input.strip_suffix(".cmap").unwrap_or(&input).to_string();
    let svg_path = svg_out.unwrap_or(format!("{stem}.svg"));
    std::fs::write(&svg_path, emit::svg(&g, &t, &fonts, &lay, cfg.page_w, cfg.page_h)).map_err(|e| format!("{svg_path}: {e}"))?;
    eprintln!("wrote {svg_path}");
    if let Some(p) = tikz_out {
        std::fs::write(&p, emit::tikz(&g, &t, &fonts, &lay, cfg.page_w, cfg.page_h)).map_err(|e| format!("{p}: {e}"))?;
        eprintln!("wrote {p}");
    }
    Ok(ExitCode::SUCCESS)
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
