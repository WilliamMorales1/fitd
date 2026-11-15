use diag::{Diag, Level};
use error::FitdResult;
use fitd::{
    attrs, check, compile, diag, emit, error, fix, fmt, geom, import, layout, lsp, metrics, split, stdlib,
};
use geom::Rect;
use std::process::ExitCode;

const USAGE: &str = "usage:
  fitd <input[.fitd]|-> [-o out.svg|out.png|out.pdf] [--tikz out.tex] [--page a0..a4|WxH|WxHmm|W:H]
              [--portrait|--landscape] [--bands N] [--margin PT] [--min-font PT] [--max-font PT] [--wrap EM] [--force] [--draft]
              [--short] [--budget CHECKS] [--expand-budget STEPS] [--scale PX] [--theme paper|light|dark|mono] [--font-family NAME]
              [--stable LOCK] [--watch]
  fitd <a.fitd> <b.fitd> ... -o out.pdf   one PDF, a page per input, in order
  fitd explain <CODE>
  fitd lsp              language server on stdin/stdout
  fitd import <file.dot|file.mmd|file.d2> [-o out.fitd]   translate Graphviz, Mermaid, or D2
  fitd fix <file.fitd>...   rewrite the indentation syntax into braces, in place
  fitd fmt <file.fitd|->...  print in canonical layout, in place (- for stdin to stdout)
  fitd split <input.fitd> [-o DIR]   an overview page plus one page per top-level branch
  fitd guide            the whole language on one page, with examples
  fitd std [NAME]       list the bundled library, or print one module
  fitd docs attrs       every attribute: statements, key, values, meaning";

const GUIDE: &str = include_str!("guide.txt");
type PdfPage = (String, metrics::Fonts, (f64, f64));

fn run(args: &[String]) -> FitdResult<ExitCode> {
    match args.first().map(String::as_str) {
        Some("explain") => {
            let code = args.get(1).ok_or(USAGE)?.to_uppercase();
            let (_, text) = check::EXPLAIN
                .iter()
                .find(|(c, _)| *c == code)
                .ok_or(format!("unknown code {code}"))?;
            println!("{code}: {text}");
            Ok(ExitCode::SUCCESS)
        }
        Some("lsp") => Ok(lsp::serve()),
        Some(cmd @ ("fix" | "fmt")) => {
            let files = &args[1..];
            if files.is_empty() {
                return Err(USAGE.into());
            }
            for path in files {
                let stdin = path == "-";
                let text = if stdin {
                    std::io::read_to_string(std::io::stdin()).map_err(|e| format!("stdin: {e}"))?
                } else {
                    std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?
                };
                let out = if cmd == "fix" {
                    if fmt::braced(&text) {
                        eprintln!("{path}: already brace syntax, skipped");
                        continue;
                    }
                    fix::fix(path, &text)
                } else if fmt::old(&text) {
                    return Err(format!("{path}: indentation syntax; run `fitd fix {path}` first").into());
                } else {
                    fmt::fmt(&text)
                };
                if stdin {
                    print!("{out}");
                } else if out != text {
                    write(path, out.as_bytes())?;
                    eprintln!("wrote {path}");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Some("import") => {
            let path = args.get(1).ok_or(USAGE)?;
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            let (fitd, dropped) = import::import(path, &text)?;
            if !dropped.is_empty() {
                eprintln!(
                    "warning: no fitd equivalent, dropped: {}",
                    dropped.join("; ")
                );
            }
            match args
                .iter()
                .position(|a| a == "-o")
                .and_then(|i| args.get(i + 1))
            {
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
            let stem = std::path::Path::new(path)
                .file_stem()
                .map_or("map".into(), |s| s.to_string_lossy().into_owned());
            let dir = match args
                .iter()
                .position(|a| a == "-o")
                .and_then(|i| args.get(i + 1))
            {
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
        Some("docs") if args.get(1).map(String::as_str) == Some("attrs") => {
            print!("{}", attrs::table());
            Ok(ExitCode::SUCCESS)
        }
        Some("guide") => {
            print!("{GUIDE}");
            Ok(ExitCode::SUCCESS)
        }
        Some("std") => {
            match args.get(1) {
                Some(name) => print!(
                    "{}",
                    stdlib::get(name).ok_or(format!(
                        "no library module {name}; the library has: {}",
                        stdlib::names().join(", ")
                    ))?
                ),
                None => {
                    for (name, text) in stdlib::MODULES {
                        let what = text.lines().next().unwrap_or("").trim_start_matches("// ");
                        println!("{name:8} {what}");
                    }
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        Some(_) if args.iter().any(|a| a == "--watch") => watch(
            &args
                .iter()
                .filter(|a| *a != "--watch")
                .cloned()
                .collect::<Vec<_>>(),
        )
        .map_err(Into::into),
        Some(_) => build(args).map_err(Into::into),
        None => Err(USAGE.into()),
    }
}

const VALUED: &[&str] = &[
    "--grid",
    "-o",
    "--tikz",
    "--page",
    "--bands",
    "--margin",
    "--min-font",
    "--max-font",
    "--wrap",
    "--budget",
    "--expand-budget",
    "--scale",
    "--theme",
    "--font-family",
    "--stable",
];

const POLL: std::time::Duration = std::time::Duration::from_millis(200);
const IDLE: std::time::Duration = std::time::Duration::from_millis(1500);

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
            if input.is_some() {
                return Err("--watch takes one input".into());
            }
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
    let mut inputs = Vec::new();
    let mut grid = None;
    let mut it = args.iter().enumerate();
    while let Some((i, a)) = it.next() {
        if a == "--grid" {
            let v = it
                .next()
                .map(|(_, v)| v.as_str())
                .ok_or("--grid needs a value")?;
            let cr = v
                .split_once('x')
                .and_then(|(c, r)| Some((c.parse::<usize>().ok()?, r.parse::<usize>().ok()?)));
            grid = Some(
                cr.filter(|&(c, r)| c > 0 && r > 0)
                    .ok_or(format!("--grid needs COLSxROWS, such as 2x1, not `{v}`"))?,
            );
        } else if VALUED.contains(&a.as_str()) {
            it.next();
        } else if a == "-" || !a.starts_with('-') {
            inputs.push(i);
        }
    }
    if inputs.len() < 2 && grid.is_none() {
        return build_one(args, None);
    }
    let out = args
        .iter()
        .position(|a| a == "-o")
        .and_then(|i| args.get(i + 1));
    let out = out
        .filter(|o| o.to_ascii_lowercase().ends_with(".pdf"))
        .ok_or("several inputs, or --grid, need -o FILE.pdf")?;
    if let Some(a) = args
        .iter()
        .find(|a| matches!(a.as_str(), "--tikz" | "--stable" | "-"))
    {
        return Err(format!("`{a}` takes one input, not several"));
    }
    let sheet = match grid {
        Some(_) => {
            let spec = args
                .iter()
                .position(|a| a == "--page")
                .and_then(|i| args.get(i + 1))
                .map_or("a1", String::as_str);
            let (w, h) = compile::page_size(spec, false)?;
            Some(if args.iter().any(|a| a == "--portrait") {
                (w.min(h), w.max(h))
            } else if args.iter().any(|a| a == "--landscape") {
                (w.max(h), w.min(h))
            } else {
                (w, h)
            })
        }
        None => None,
    };
    let (cols, rows) = grid.unwrap_or((1, 1));
    let mut pages = Vec::new();
    let mut failed = 0;
    for (n, &i) in inputs.iter().enumerate() {
        eprintln!("--- {} of {}: {}", n + 1, inputs.len(), args[i]);
        let mut one = Vec::new();
        let mut skip = false;
        for (j, a) in args.iter().enumerate() {
            if std::mem::take(&mut skip) || (j != i && inputs.contains(&j)) {
                continue;
            }
            if sheet.is_some()
                && matches!(
                    a.as_str(),
                    "--page" | "--grid" | "--portrait" | "--landscape"
                )
            {
                skip = a == "--page" || a == "--grid";
                continue;
            }
            one.push(a.clone());
        }
        if let Some((w, h)) = sheet {
            one.extend([
                "--page".into(),
                format!("{:.3}x{:.3}pt", w / cols as f64, h / rows as f64),
            ]);
        }
        if build_one(&one, Some(&mut pages))? != ExitCode::SUCCESS {
            failed += 1;
        }
    }
    if failed > 0 {
        eprintln!(
            "error: {failed} of {} diagrams failed; no output written",
            inputs.len()
        );
        return Ok(ExitCode::FAILURE);
    }
    let per = cols * rows;
    let sheets: Vec<emit::Sheet> = pages
        .chunks(per)
        .map(|chunk| emit::Sheet {
            size: sheet,
            cells: chunk
                .iter()
                .enumerate()
                .map(|(k, (svg, fonts, (w, h)))| {
                    (
                        svg.as_str(),
                        fonts,
                        (k % cols) as f64 * w,
                        (k / cols) as f64 * h,
                    )
                })
                .collect(),
        })
        .collect();
    write(out, &emit::pdf(&sheets)?)?;
    let plural = |n: usize, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "s" });
    eprintln!(
        "wrote {out} ({} on {})",
        plural(pages.len(), "diagram"),
        plural(sheets.len(), "page")
    );
    Ok(ExitCode::SUCCESS)
}

fn build_one(args: &[String], pages: Option<&mut Vec<PdfPage>>) -> Result<ExitCode, String> {
    let mut input = None;
    let (mut svg_out, mut tikz_out) = (None, None);
    let mut short = false;
    let mut scale = 2.0;
    let mut force = false;
    let mut o = compile::Options::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or(format!("{a} needs a value"));
        let num = |s: String| s.parse::<f64>().map_err(|_| format!("bad number `{s}`"));
        match a.as_str() {
            "-o" => svg_out = Some(val()?),
            "--tikz" => tikz_out = Some(val()?),
            "--page" => o.page = Some(val()?),
            "--portrait" => o.portrait = true,
            "--landscape" => o.landscape = true,
            "--bands" => {
                o.bands = Some(
                    val()?
                        .parse::<usize>()
                        .ok()
                        .filter(|&k| k > 0)
                        .ok_or("--bands needs a whole number, 1 or more")?,
                )
            }
            "--force" => force = true,
            "--draft" => o.draft = true,
            "--short" => short = true,
            "--margin" => o.margin = Some(num(val()?)?),
            "--min-font" => o.min_font = Some(num(val()?)?),
            "--max-font" => o.max_font = Some(num(val()?)?),
            "--wrap" => o.wrap = Some(num(val()?)?),
            "--budget" => o.budget = num(val()?)? as u64,
            "--expand-budget" => {
                o.expand_budget = val()?
                    .parse::<u64>()
                    .map_err(|_| "--expand-budget needs a whole number")?
            }
            "--scale" => scale = num(val()?)?,
            "--theme" => o.theme = Some(val()?),
            "--font-family" => o.family = Some(val()?),
            "--stable" => o.lock = Some(val()?),
            s if (s == "-" || !s.starts_with('-')) && input.is_none() => {
                input = Some(s.to_string())
            }
            s => return Err(format!("unknown argument `{s}`\n{USAGE}")),
        }
    }
    let mut input = input.ok_or(USAGE)?;
    if !std::path::Path::new(&input).exists()
        && std::path::Path::new(&format!("{input}.fitd")).exists()
    {
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
    let render = |src: &diag::Source, diags: &[Diag]| {
        if short {
            diag::render_short(src, diags)
        } else {
            diag::render(src, diags)
        }
    };
    let c = match compile::compile(&input, &text, &o, &mut |src, d| {
        eprint!("{}", render(src, d))
    }) {
        Ok(c) => c,
        Err(compile::Stop::Failed(text)) => {
            eprint!("{text}");
            return Ok(ExitCode::FAILURE);
        }
        Err(compile::Stop::Err(e)) => return Err(e),
    };
    let (g, lay, cfg) = (&c.graph, &c.layout, &c.cfg);
    eprint!("{}", render(&c.src, &c.diags));
    let errors = c.diags.iter().filter(|d| d.level == Level::Error).count();
    let warnings = c.diags.iter().filter(|d| d.level == Level::Warning).count();
    let s = lay.stats;
    if short {
        eprintln!(
            "body font {:.1}pt on {:.0}x{:.0}pt; {errors} errors, {warnings} warnings",
            lay.body, cfg.page_w, cfg.page_h
        );
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
            if s.soft_total > 0 {
                format!("; soft cost {}/{}", s.soft_cost, s.soft_total)
            } else {
                String::new()
            }
        );
        let rects: Vec<Rect> = (0..g.nodes.len()).map(|v| lay.rect(v)).collect();
        let q = layout::Readable::new(&c.tree, lay.kind, &lay.edges, cfg.page(), cfg.legible)
            .score(&rects);
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
            let tried: Vec<String> = lay
                .tradeoff
                .iter()
                .map(|(f, m)| {
                    format!(
                        "{f:.1}pt {m:.0}{}",
                        if *f == lay.body { " (chosen)" } else { "" }
                    )
                })
                .collect();
            eprintln!(
                "font trade-off, merit (lower is better; legible from {:.1}pt): {}",
                cfg.legible,
                tried.join(", ")
            );
        }
    }
    if errors > 0 && !force {
        eprintln!(
            "error: aborting due to {errors} previous errors; no output written (use --force to write anyway)"
        );
        return Ok(ExitCode::FAILURE);
    }
    if let Some(pages) = pages {
        let svg = c.svg(None);
        let size = (cfg.page_w, cfg.page_h);
        pages.push((svg, c.fonts, size));
        return Ok(ExitCode::SUCCESS);
    }
    let stem = input.strip_suffix(".fitd").unwrap_or(&input).to_string();
    let out = svg_out.unwrap_or(format!("{stem}.svg"));
    let bytes = if out.to_ascii_lowercase().ends_with(".png") {
        emit::png(&c.svg(Some(c.png_view())), &c.fonts, scale)?
    } else if out.to_ascii_lowercase().ends_with(".pdf") {
        emit::pdf(&[emit::Sheet {
            size: None,
            cells: vec![(&c.svg(None), &c.fonts, 0.0, 0.0)],
        }])?
    } else {
        c.svg(None).into_bytes()
    };
    write(&out, &bytes)?;
    eprintln!("wrote {out}");
    if let Some(p) = tikz_out {
        write(&p, c.tikz().as_bytes())?;
        eprintln!("wrote {p}");
    }
    if let Some(p) = &o.lock {
        write(p, layout::Anchor::render(g, lay, cfg).as_bytes())?;
        if let Some(a) = cfg.anchor {
            let kept = (0..g.nodes.len())
                .filter(|&v| {
                    a.pos[v].is_some_and(|q| (q.0 - lay.pos[v].0).hypot(q.1 - lay.pos[v].1) <= 0.01)
                })
                .count();
            eprintln!(
                "wrote {p} ({kept} of {} locked nodes kept their spots)",
                a.pos.iter().flatten().count()
            );
        } else {
            eprintln!("wrote {p}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn write(path: &str, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = std::path::Path::new(path)
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
    {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = format!("{path}.fitd-tmp{}", std::process::id());
    let renamed = std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, path));
    #[cfg(windows)]
    let renamed =
        renamed.or_else(|_| std::fs::write(path, bytes).and_then(|_| std::fs::remove_file(&tmp)));
    renamed.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{path}: {e}")
    })
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
