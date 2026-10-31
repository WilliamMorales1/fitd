use std::path::PathBuf;
use std::process::Command;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cmapc-test-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

fn cmapc(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_cmapc")).args(args).output().unwrap();
    (out.status.success(), String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn builds_example_with_zero_errors() {
    let svg = tmp("math4200.svg");
    let tex = tmp("math4200.tex");
    // The hand-made map this tool replaces reached 16pt on this page; cmapc must fit 19.8pt.
    let (ok, log) = cmapc(&["examples/math4200.cmap", "-o", svg.to_str().unwrap(), "--tikz", tex.to_str().unwrap(), "--page", "2296x1524", "--min-font", "19.8"]);
    assert!(ok, "{log}");
    assert!(log.contains(" 0 errors"), "{log}");
    let body: f64 = log.split("body font ").nth(1).unwrap().split("pt").next().unwrap().parse().unwrap();
    assert!(body >= 19.8, "body font regressed to {body}pt\n{log}");
    let s = std::fs::read_to_string(&svg).unwrap();
    assert!(s.starts_with("<svg") && s.contains("Groups"));
    assert!(std::fs::read_to_string(&tex).unwrap().contains("\\begin{tikzpicture}"));
}

#[test]
fn general_graph_with_cycle_and_no_root() {
    let src = tmp("cycle.cmap");
    std::fs::write(&src, "node a \"Alpha\"\nnode b \"Beta\"\n| detail\nnode c \"Gamma\"\nnode d \"Delta\"\na -- b\nb -- c\nc -- a\nc -- d\n").unwrap();
    let (ok, log) = cmapc(&[src.to_str().unwrap(), "-o", tmp("cycle.svg").to_str().unwrap(), "--page", "a4"]);
    assert!(ok, "{log}");
    assert!(log.contains("chose graph center"), "{log}");
    assert!(log.contains("closes a cycle"), "{log}");
}

#[test]
fn impossible_page_is_rejected() {
    let (ok, log) = cmapc(&["examples/math4200.cmap", "-o", tmp("x.svg").to_str().unwrap(), "--page", "200x150"]);
    assert!(!ok);
    assert!(log.contains("no layout fits"), "{log}");
}

#[test]
fn parse_errors_point_at_line() {
    let src = tmp("bad.cmap");
    std::fs::write(&src, "node a\na -> ghost\n").unwrap();
    let (ok, log) = cmapc(&[src.to_str().unwrap()]);
    assert!(!ok);
    assert!(log.contains("error[E0001]: unknown node `ghost`") && log.contains("bad.cmap:2"), "{log}");
}

#[test]
fn explain_known_code() {
    let out = Command::new(env!("CARGO_BIN_EXE_cmapc")).args(["explain", "e0101"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("overlapping regions"));
}

#[test]
fn layered_example_meets_required_constraints() {
    let svg = tmp("compiler.svg");
    let (ok, log) = cmapc(&["examples/compiler.cmap", "-o", svg.to_str().unwrap()]);
    assert!(ok, "{log}");
    assert!(log.contains(" 0 errors") && !log.contains("E0108"), "{log}");
    assert!(std::fs::read_to_string(&svg).unwrap().contains("marker-end"));
}

#[test]
fn groups_frame_members_and_edges_take_colors() {
    let src = tmp("group.cmap");
    std::fs::write(&src, "layout layered\npage a4\nstyle hot color=#C62828\nnode a\nnode b\nnode c\nnode d\na -> b .hot\nb -> c, d\nc ..> a color=#2E7D32\ngroup \"Pair\" c, d color=#1565C0\n").unwrap();
    let (svg, tex) = (tmp("group.svg"), tmp("group.tex"));
    let (ok, log) = cmapc(&[src.to_str().unwrap(), "-o", svg.to_str().unwrap(), "--tikz", tex.to_str().unwrap()]);
    assert!(ok, "{log}");
    assert!(log.contains(" 0 errors, 0 warnings"), "{log}");
    let s = std::fs::read_to_string(&svg).unwrap();
    assert!(s.contains(">Pair</text>") && s.contains("stroke=\"#1565C0\""), "{s}");
    assert!(s.contains("stroke=\"#C62828\" stroke-width") && s.contains("url(#mC62828)") && s.contains("stroke=\"#2E7D32\""), "{s}");
    assert!(std::fs::read_to_string(&tex).unwrap().contains("Pair"));
    // A group whose members must sit around a non-member cannot hold.
    std::fs::write(&src, "layout layered\npage a4\nnode a\nnode b\nnode c\na -> b\nb -> c\ngroup a, c\nrequire align column(a, b, c)\n").unwrap();
    let (ok, log) = cmapc(&[src.to_str().unwrap(), "-o", svg.to_str().unwrap()]);
    assert!(!ok);
    assert!(log.contains("error[E0110]: these 2 constraints") && log.contains("8 | group a, c"), "{log}");
}

#[test]
fn unsat_constraints_report_minimal_core() {
    let src = tmp("core.cmap");
    let text = "layout layered\npage 420x300pt\nnode a\nnode b\nnode c\nnode d\na -> b, c\nc -> d\n\
                require b at left\nrequire align column(c, d)\nrequire c at left\nrequire d at bottom-left\nrequire b left-of c gap=20em\n";
    std::fs::write(&src, text).unwrap();
    let (ok, log) = cmapc(&[src.to_str().unwrap(), "-o", tmp("core.svg").to_str().unwrap()]);
    assert!(!ok);
    assert!(log.contains("error[E0110]: these 2 constraints"), "{log}");
    assert!(log.contains("11 | require c at left") && log.contains("13 | require b left-of c gap=20em"), "{log}");
    assert!(log.contains("7pt is the default font floor"), "{log}");
}

#[test]
fn chained_orderings_fit_and_overlong_chains_fail_fast() {
    // Forward checking sees one link of a chain at a time, so without arc consistency a
    // satisfiable two-link chain ran out of budget and was reported as an unsat core.
    let base = std::fs::read_to_string("examples/cell_biology.cmap").unwrap();
    let chain = |gaps: &[(&str, &str)], gap: u32| gaps.iter().map(|(a, b)| format!("require {a} above {b} gap={gap}em\n")).collect::<String>();
    let links = [("anaerobic", "regulation"), ("regulation", "yield"), ("yield", "water"), ("water", "oxygen")];
    let fits = tmp("chain-fits.cmap");
    std::fs::write(&fits, base.clone() + &chain(&links[1..3], 30)).unwrap();
    let (ok, log, _) = cmapc_cpu(&[fits.to_str().unwrap(), "-o", tmp("chain-fits.svg").to_str().unwrap()]);
    assert!(ok && log.contains(" 0 errors"), "{log}");
    let tall = tmp("chain-tall.cmap");
    std::fs::write(&tall, base + &chain(&links, 40)).unwrap();
    let (ok, log, took) = cmapc_cpu(&[tall.to_str().unwrap(), "-o", tmp("chain-tall.svg").to_str().unwrap()]);
    assert!(!ok && log.contains("error[E0110]: these 3 constraints"), "{log}");
    assert!(took <= 3.0, "unsat core took {took:.1} CPU s");
}

#[test]
fn contradictory_orderings_fail_before_search() {
    let src = tmp("cycle-order.cmap");
    std::fs::write(&src, "node a\nnode b\nnode c\nrequire a left-of b\nrequire b left-of c\nrequire c left-of a\n").unwrap();
    let (ok, log) = cmapc(&[src.to_str().unwrap()]);
    assert!(!ok);
    assert!(log.contains("error[E0008]: contradictory horizontal constraints"), "{log}");
    assert!(!log.contains("body font"), "search should not run\n{log}");
}

#[test]
fn png_is_cropped_to_drawing() {
    let png = tmp("compiler.png");
    let (ok, log) = cmapc(&["examples/compiler.cmap", "-o", png.to_str().unwrap(), "--page", "a3", "--scale", "1"]);
    assert!(ok, "{log}");
    let b = std::fs::read(&png).unwrap();
    assert_eq!(&b[..8], b"\x89PNG\r\n\x1a\n");
    let (w, h) = (u32::from_be_bytes(b[16..20].try_into().unwrap()), u32::from_be_bytes(b[20..24].try_into().unwrap()));
    // An A3 page is 1191x842pt; the layered example is a narrow column well inside it.
    assert!(w > 100 && h > 100 && w < 1191 && (w as f64) < 0.8 * h as f64, "{w}x{h}");
}

#[test]
fn theme_colors_reach_svg_and_tikz() {
    let src = tmp("theme.cmap");
    std::fs::write(&src, "theme dark background=#102030 palette=\"#FFB74D #4DD0E1\"\nnode c\nnode a fill=#FFE0B2 text=#3E2723\nnode b\nc -> a, b\n").unwrap();
    let (svg, tex) = (tmp("theme.svg"), tmp("theme.tex"));
    let (ok, log) = cmapc(&[src.to_str().unwrap(), "-o", svg.to_str().unwrap(), "--tikz", tex.to_str().unwrap()]);
    assert!(ok, "{log}");
    let s = std::fs::read_to_string(&svg).unwrap();
    for c in ["#102030", "#4DD0E1", "#FFE0B2", "#3E2723", "#E6E6E6"] {
        assert!(s.contains(c), "{c} missing from svg");
    }
    assert!(std::fs::read_to_string(&tex).unwrap().contains("\\definecolor{c102030}"));
    let (ok, log) = cmapc(&[src.to_str().unwrap(), "-o", svg.to_str().unwrap(), "--theme", "neon"]);
    assert!(!ok && log.contains("unknown theme `neon`"), "{log}");
}

/// EXAMPLES lists each example with its warning baseline and its solver check budget, about
/// 10% above what it uses. Checks are deterministic, unlike CPU time: hyperthreads sharing a
/// busy core run slower per CPU second, so a CPU budget failed under load with identical
/// output. CPU time is still capped loosely (HANG_SECS) to catch a hang. Lower a number when a
/// change improves on it.
const EXAMPLES: [(&str, usize, u64); 5] = [
    ("cell_biology", 0, 4_300_000),
    ("compiler", 0, 4_600_000),
    ("earth_science", 2, 128_000_000),
    ("math4200", 0, 40_000_000),
    ("math4200-v2", 0, 56_000_000),
];

const HANG_SECS: f64 = 60.0;

#[repr(C)]
struct Rusage {
    utime: [i64; 2],
    stime: [i64; 2],
    rest: [i64; 14],
}

unsafe extern "C" {
    fn wait4(pid: i32, status: *mut i32, options: i32, usage: *mut Rusage) -> i32;
}

/// cmapc_cpu runs cmapc and also returns the CPU seconds it used across all its threads.
#[allow(clippy::zombie_processes, reason = "reaped by wait4")]
fn cmapc_cpu(args: &[&str]) -> (bool, String, f64) {
    use std::io::Read;
    let mut child = Command::new(env!("CARGO_BIN_EXE_cmapc")).args(args).stderr(std::process::Stdio::piped()).spawn().unwrap();
    let mut log = String::new();
    child.stderr.take().unwrap().read_to_string(&mut log).unwrap();
    let (mut status, mut ru) = (0, Rusage { utime: [0; 2], stime: [0; 2], rest: [0; 14] });
    // std's Child::wait drops the child's resource usage, so reap it directly.
    assert_eq!(unsafe { wait4(child.id() as i32, &mut status, 0, &mut ru) }, child.id() as i32);
    let secs = |t: [i64; 2]| t[0] as f64 + t[1] as f64 * 1e-6;
    // Exited normally with status 0.
    (status & 0xffff == 0, log, secs(ru.utime) + secs(ru.stime))
}

#[test]
fn every_example_builds_within_baseline() {
    for (name, warnings, budget) in EXAMPLES {
        let src = format!("examples/{name}.cmap");
        let outs = [tmp(&format!("{name}-a.svg")), tmp(&format!("{name}-b.svg"))];
        // A second build runs alongside to catch HashMap order or thread timing leaking into
        // the layout.
        let twin = Command::new(env!("CARGO_BIN_EXE_cmapc")).args([&src, "-o", outs[1].to_str().unwrap()]).stderr(std::process::Stdio::null()).spawn().unwrap();
        let (ok, log, took) = cmapc_cpu(&[&src, "-o", outs[0].to_str().unwrap()]);
        assert!(twin.wait_with_output().unwrap().status.success(), "{name}: second build failed");
        assert!(ok && log.contains(" 0 errors"), "{name}\n{log}");
        let got: usize = log.split(" errors, ").nth(1).unwrap().split(' ').next().unwrap().parse().unwrap();
        assert!(got <= warnings, "{name}: {got} warnings, baseline {warnings}\n{log}");
        let checks: u64 = log.split(" checks").next().unwrap().rsplit(' ').next().unwrap().parse().unwrap();
        assert!(checks <= budget, "{name}: {checks} solver checks, budget {budget}\n{log}");
        assert!(took <= HANG_SECS, "{name}: used {took:.1} CPU s");
        assert!(std::fs::read(&outs[0]).unwrap() == std::fs::read(&outs[1]).unwrap(), "{name}: two builds differ");
    }
}

#[test]
fn fixtures_fail_or_build_as_designed() {
    let (ok, log) = cmapc(&["tests/fixtures/stress_test.cmap", "-o", tmp("stress.svg").to_str().unwrap()]);
    assert!(!ok && log.contains("error[E0008]"), "{log}");
    let (ok, log) = cmapc(&["tests/fixtures/complex_constraints.cmap", "-o", tmp("complex.svg").to_str().unwrap()]);
    assert!(ok && log.contains(" 0 errors"), "{log}");
}

#[test]
fn row_alignment_merges_layers_in_flowchart() {
    let src = tmp("row.cmap");
    std::fs::write(&src, "layout layered\npage a4\nnode a\nnode b\nnode c\nnode d\na -> b\nb -> c\nc -> d\nrequire align row(b, c, d)\nrequire b left-of c left-of d\n").unwrap();
    let (ok, log) = cmapc(&[src.to_str().unwrap(), "-o", tmp("row.svg").to_str().unwrap()]);
    assert!(ok, "{log}");
    assert!(log.contains(" 0 errors"), "{log}");
}

#[test]
fn output_directories_are_created() {
    // The README writes to `out/`, which a fresh clone does not have.
    let dir = tmp("fresh").join("out");
    let _ = std::fs::remove_dir_all(&dir);
    let tex = dir.join("compiler.tex");
    let (ok, log) = cmapc(&["examples/compiler.cmap", "-o", dir.join("c.svg").to_str().unwrap(), "--tikz", tex.to_str().unwrap()]);
    assert!(ok, "{log}");
    assert!(tex.exists());
}

#[test]
fn guide_examples_build_from_stdin() {
    use std::io::Write;
    let out = Command::new(env!("CARGO_BIN_EXE_cmapc")).arg("guide").output().unwrap();
    let guide = String::from_utf8(out.stdout).unwrap();
    // The guide is what an agent reads instead of LANGUAGE.md, so it must stay small.
    assert!(guide.len() <= 2500, "guide grew to {} bytes", guide.len());
    for name in ["Concept map:", "Flowchart:"] {
        let body: String = guide.split(name).nth(1).unwrap().trim_start_matches('\n').lines().take_while(|l| !l.is_empty()).map(|l| l.trim_start().to_string() + "\n").collect();
        let svg = tmp(&format!("guide-{}.svg", name.len()));
        let mut child = Command::new(env!("CARGO_BIN_EXE_cmapc")).args(["-", "-o", svg.to_str().unwrap(), "--short"]).stdin(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(body.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        let log = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success() && log.contains("0 errors, 0 warnings"), "{name}\n{body}\n{log}");
        assert_eq!(log.lines().count(), 2, "--short should print a summary and the output path\n{log}");
    }
}

#[test]
fn layers_that_exactly_fill_the_page_fit_and_crowded_flows_say_why() {
    // An agent's first flowchart: at 8pt the layer gaps fill a4 exactly, and rounding once put
    // the last layer a hair off the page, so cmapc wrongly said nothing fits.
    let nodes = [
        ("intro", "Bill Introduced", "Introduced in House or Senate"),
        ("committee", "Committee Action", "Review, Markup, and Vote"),
        ("floor", "Floor Vote", "Debate and Vote in Chamber"),
        ("other", "Other Chamber", "Repeated process in House/Senate"),
        ("conference", "Conference Committee", "Resolving differences between versions"),
        ("president", "Presidential Action", "Signs or Vetoes"),
        ("law", "Law", "Bill becomes law"),
        ("veto", "Veto", "President rejects bill"),
        ("override", "Veto Override", "2/3 vote in both chambers"),
    ];
    let mut text = String::from("layout layered dir=down\npage a4 portrait\nfont 8pt wrap=10em\n");
    for (id, title, body) in nodes {
        text += &format!("node {id} \"{title}\"\n| {body}\n");
    }
    text += "intro -> committee\ncommittee -> floor\nfloor -> other\nother -> conference\nconference -> president\npresident -> law, veto\nveto -> override\noverride -> law\n";
    let down = tmp("bill-down.cmap");
    std::fs::write(&down, &text).unwrap();
    let (ok, log) = cmapc(&[down.to_str().unwrap(), "-o", tmp("bill-down.svg").to_str().unwrap()]);
    assert!(ok && log.contains("body font 8.0pt"), "{log}");
    // Across a portrait page the nine layers do not fit in one line, so the flow wraps.
    let right = tmp("bill-right.cmap");
    std::fs::write(&right, text.replace("dir=down", "dir=right")).unwrap();
    let (ok, log) = cmapc(&[right.to_str().unwrap(), "-o", tmp("bill-right.svg").to_str().unwrap()]);
    assert!(ok && log.contains("body font 8.0pt") && log.contains(" 0 errors"), "{log}");
    // On a strip too small to wrap into, the error says why.
    std::fs::write(&right, text.replace("dir=down", "dir=right").replace("page a4 portrait", "page 400x150pt")).unwrap();
    let (ok, log) = cmapc(&[right.to_str().unwrap(), "-o", tmp("bill-right.svg").to_str().unwrap()]);
    assert!(!ok && log.contains("the longest path has 9 layers") && log.contains("dir=down"), "{log}");
}
