use std::path::PathBuf;
use std::process::Command;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fitd-test-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

fn fitd(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fitd"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn edge_source_lists_fan_in() {
    let src = tmp("fan.fitd");
    std::fs::write(&src, "layout layered\nnode a\nnode b\nnode c\nnode d\na, b -> c, d \"x\"\n").unwrap();
    let out = tmp("fan.svg");
    let (ok, log) = fitd(&[src.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert!(ok && log.contains("4 nodes, 4 edges"), "{log}");
}

#[test]
fn unconnected_grid_nodes_stack_in_layered() {
    let src = tmp("grid.fitd");
    std::fs::write(
        &src,
        "layout layered\nnode a row=1 col=1\nnode b row=1 col=2\nnode c row=2 col=1\nnode d row=2 col=2\n",
    )
    .unwrap();
    let out = tmp("grid.svg");
    let (ok, log) = fitd(&[src.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert!(ok && log.contains("0 errors"), "{log}");
}

#[test]
fn ratio_pages_fold_long_chains_into_bands() {
    let page = |name: &str, extra: &[&str]| {
        let out = tmp(name);
        let mut args = vec!["examples/gallery/17-timeline.fitd", "-o", out.to_str().unwrap()];
        args.extend(extra);
        let (ok, log) = fitd(&args);
        assert!(ok, "{log}");
        let w = log.split("fits on ").nth(1).unwrap().split('x').next().unwrap();
        w.parse::<f64>().unwrap()
    };
    // A smaller page for the same font floor means a larger font at the requested ratio.
    assert!(page("fold.svg", &[]) < 0.8 * page("one-band.svg", &["--bands", "1"]));
}

#[test]
fn image_rows_embed_pictures_and_report_missing_files() {
    let out = tmp("image.svg");
    let tex = tmp("image.tex");
    let (ok, log) = fitd(&[
        "examples/gallery/29-image.fitd",
        "-o",
        out.to_str().unwrap(),
        "--tikz",
        tex.to_str().unwrap(),
    ]);
    assert!(ok, "{log}");
    let svg = std::fs::read_to_string(&out).unwrap();
    assert!(svg.contains("href=\"data:image/png;base64,iVBORw0KGgo"), "no embedded PNG");
    let tex = std::fs::read_to_string(&tex).unwrap();
    assert!(tex.contains("\\includegraphics[width="), "no includegraphics");
    for e in ["png", "pdf"] {
        let out = tmp(&format!("image.{e}"));
        let (ok, log) = fitd(&["examples/gallery/29-image.fitd", "-o", out.to_str().unwrap()]);
        assert!(ok, "{log}");
    }
    let src = tmp("missing.fitd");
    std::fs::write(&src, "node a\n~ image \"nope.png\"\n").unwrap();
    let (ok, log) = fitd(&[src.to_str().unwrap(), "-o", tmp("missing.svg").to_str().unwrap()]);
    assert!(!ok && log.contains("E0014"), "{log}");
}

#[test]
fn builds_example_with_zero_errors() {
    let svg = tmp("group_theory.svg");
    let tex = tmp("group_theory.tex");
    // The hand-made map this tool replaces reached 16pt on this page; fitd must fit 19.2pt.
    let (ok, log) = fitd(&[
        "examples/group_theory.fitd",
        "-o",
        svg.to_str().unwrap(),
        "--tikz",
        tex.to_str().unwrap(),
        "--page",
        "2296x1524",
        "--min-font",
        "19.2",
    ]);
    assert!(ok, "{log}");
    assert!(log.contains(" 0 errors"), "{log}");
    let body: f64 = log
        .split("body font ")
        .nth(1)
        .unwrap()
        .split("pt")
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(body >= 19.2, "body font regressed to {body}pt\n{log}");
    let s = std::fs::read_to_string(&svg).unwrap();
    assert!(s.starts_with("<svg") && s.contains("Groups"));
    assert!(
        std::fs::read_to_string(&tex)
            .unwrap()
            .contains("\\begin{tikzpicture}")
    );
}

#[test]
fn general_graph_with_cycle_and_no_root() {
    let src = tmp("cycle.fitd");
    std::fs::write(&src, "node a \"Alpha\"\nnode b \"Beta\"\n| detail\nnode c \"Gamma\"\nnode d \"Delta\"\na -- b\nb -- c\nc -- a\nc -- d\n").unwrap();
    let (ok, log) = fitd(&[
        src.to_str().unwrap(),
        "-o",
        tmp("cycle.svg").to_str().unwrap(),
        "--page",
        "a4",
    ]);
    assert!(ok, "{log}");
    assert!(log.contains("chose graph center"), "{log}");
    assert!(log.contains("closes a cycle"), "{log}");
}

#[test]
fn radial_tree_edges_keep_their_labels() {
    // `d` is reachable only by its link, which becomes its tree edge.
    let src = tmp("labels.fitd");
    std::fs::write(&src, "layout radial root=a\nnode a \"Alpha\"\nnode b \"Beta\"\nnode c \"Gamma\"\nnode d \"Delta\"\na -> b \"feeds\"\na -> c\nd ..> c \"pumps\"\n").unwrap();
    let svg = tmp("labels.svg");
    let (ok, log) = fitd(&[
        src.to_str().unwrap(),
        "-o",
        svg.to_str().unwrap(),
        "--page",
        "a4",
    ]);
    assert!(ok && log.contains(" 0 warnings"), "{log}");
    let s = std::fs::read_to_string(&svg).unwrap();
    assert!(
        s.contains(">feeds<") && s.contains(">pumps<"),
        "labels missing"
    );
}

#[test]
fn labeled_arrows_get_room_between_layers() {
    let src = tmp("bill.fitd");
    std::fs::write(&src, "layout layered dir=down\npage a4\nnode a \"Bill Introduced\"\nnode b \"Committee Review\"\nnode c \"Floor Vote\"\nnode d \"President\"\na -> b\nb -> c \"approved\"\nc -> d\n").unwrap();
    let svg = tmp("bill.svg");
    let (ok, log) = fitd(&[src.to_str().unwrap(), "-o", svg.to_str().unwrap()]);
    assert!(ok && log.contains(" 0 warnings"), "{log}");
    let s = std::fs::read_to_string(&svg).unwrap();
    let y = |title: &str| -> f64 {
        let line = s
            .lines()
            .find(|l| l.contains(&format!(">{title}<")))
            .unwrap();
        line.split("y=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .parse()
            .unwrap()
    };
    let body: f64 = log
        .split("body font ")
        .nth(1)
        .unwrap()
        .split("pt")
        .next()
        .unwrap()
        .parse()
        .unwrap();
    // One-line boxes of equal height, so the labeled step is longer by the label and its margins.
    let (plain, labeled) = (
        y("Committee Review") - y("Bill Introduced"),
        y("Floor Vote") - y("Committee Review"),
    );
    assert!(
        labeled >= plain + body,
        "plain {plain}, labeled {labeled}, body {body}"
    );
}

#[test]
fn impossible_page_is_rejected() {
    let (ok, log) = fitd(&[
        "examples/group_theory.fitd",
        "-o",
        tmp("x.svg").to_str().unwrap(),
        "--page",
        "200x150",
    ]);
    assert!(!ok);
    assert!(
        log.contains("no layout fits") && log.contains("proved for every candidate"),
        "{log}"
    );
    assert!(
        log.contains("split the map: the branch under `cos` (9 nodes"),
        "{log}"
    );
}

#[test]
fn parse_errors_point_at_line() {
    let src = tmp("bad.fitd");
    std::fs::write(&src, "node a\na -> ghost\n").unwrap();
    let (ok, log) = fitd(&[src.to_str().unwrap()]);
    assert!(!ok);
    assert!(
        log.contains("error[E0001]: unknown node `ghost`") && log.contains("bad.fitd:2"),
        "{log}"
    );
}

#[test]
fn explain_known_code() {
    let out = Command::new(env!("CARGO_BIN_EXE_fitd"))
        .args(["explain", "e0101"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("overlapping regions"));
}

#[test]
fn layered_example_meets_required_constraints() {
    let svg = tmp("compiler.svg");
    let (ok, log) = fitd(&["examples/compiler.fitd", "-o", svg.to_str().unwrap()]);
    assert!(ok, "{log}");
    assert!(log.contains(" 0 errors") && !log.contains("E0108"), "{log}");
    assert!(
        std::fs::read_to_string(&svg)
            .unwrap()
            .contains("marker-end")
    );
}

#[test]
fn groups_frame_members_and_edges_take_colors() {
    let src = tmp("group.fitd");
    std::fs::write(&src, "layout layered\npage a4\nstyle hot color=#C62828\nnode a\nnode b\nnode c\nnode d\nhot\n  a -> b\nb -> c, d\nc ..> a color=#2E7D32\ngroup \"Pair\" c, d color=#1565C0\n").unwrap();
    let (svg, tex) = (tmp("group.svg"), tmp("group.tex"));
    let (ok, log) = fitd(&[
        src.to_str().unwrap(),
        "-o",
        svg.to_str().unwrap(),
        "--tikz",
        tex.to_str().unwrap(),
    ]);
    assert!(ok, "{log}");
    assert!(log.contains(" 0 errors, 0 warnings"), "{log}");
    let s = std::fs::read_to_string(&svg).unwrap();
    assert!(
        s.contains(">Pair</text>") && s.contains("stroke=\"#1565C0\""),
        "{s}"
    );
    assert!(
        s.contains("stroke=\"#C62828\" stroke-width")
            && s.contains("url(#maC62828)")
            && s.contains("stroke=\"#2E7D32\""),
        "{s}"
    );
    assert!(std::fs::read_to_string(&tex).unwrap().contains("Pair"));
    // A class gives the frame its color, so a group and its key entry share one style.
    std::fs::write(&src, "layout layered\npage a4\nstyle d1 color=#E69F00\nnode a\nnode b\na -> b\nd1\n  group \"D1\" a, b\n  key \"design\"\n").unwrap();
    let (ok, log) = fitd(&[src.to_str().unwrap(), "-o", svg.to_str().unwrap()]);
    assert!(ok && log.contains(" 0 errors, 0 warnings"), "{log}");
    let s = std::fs::read_to_string(&svg).unwrap();
    assert!(
        s.contains(">D1</text>") && s.matches("#E69F00").count() >= 2,
        "{s}"
    );
    // A group whose members must sit around a non-member cannot hold.
    std::fs::write(&src, "layout layered\npage a4\nnode a\nnode b\nnode c\na -> b\nb -> c\ngroup a, c\nrequire align column(a, b, c)\n").unwrap();
    let (ok, log) = fitd(&[src.to_str().unwrap(), "-o", svg.to_str().unwrap()]);
    assert!(!ok);
    assert!(
        log.contains("error[E0110]: these 2 constraints") && log.contains("8 | group a, c"),
        "{log}"
    );
}

#[test]
fn nested_groups_draw_inner_frames_inside_outer() {
    let src = tmp("nested.fitd");
    let body = "node a\nnode b\nnode c\nnode d\nnode e\na -> b\nb -> c\nc -> d\nd -> e\ngroup \"Outer\" b, c, d color=#1565C0\ngroup \"Inner\" b, c color=#2E7D32\n";
    for layout in [
        "layout layered\npage a4\n",
        "layout radial root=a\npage a4\n",
    ] {
        std::fs::write(&src, format!("{layout}{body}")).unwrap();
        let svg = tmp("nested.svg");
        let (ok, log) = fitd(&[src.to_str().unwrap(), "-o", svg.to_str().unwrap()]);
        assert!(ok && log.contains(" 0 errors, 0 warnings"), "{log}");
        let s = std::fs::read_to_string(&svg).unwrap();
        // Outer frame comes first so the inner one is drawn on top.
        assert!(
            s.find(">Outer<").unwrap() < s.find(">Inner<").unwrap(),
            "{s}"
        );
    }
    std::fs::write(&src, format!("layout layered\npage a4\n{body}group b, e\n")).unwrap();
    let (ok, log) = fitd(&[
        src.to_str().unwrap(),
        "-o",
        tmp("nested.svg").to_str().unwrap(),
    ]);
    assert!(!ok && log.contains("error[E0010]"), "{log}");
}

#[test]
fn unsat_constraints_report_minimal_core() {
    let src = tmp("core.fitd");
    let text = "layout layered\npage 420x300pt\nnode a\nnode b\nnode c\nnode d\na -> b, c\nc -> d\n\
                require b at left\nrequire align column(c, d)\nrequire c at left\nrequire d at bottom-left\nrequire b left-of c gap=20em\n";
    std::fs::write(&src, text).unwrap();
    let (ok, log) = fitd(&[
        src.to_str().unwrap(),
        "-o",
        tmp("core.svg").to_str().unwrap(),
    ]);
    assert!(!ok);
    assert!(log.contains("error[E0110]: these 2 constraints"), "{log}");
    assert!(
        log.contains("11 | require c at left") && log.contains("13 | require b left-of c gap=20em"),
        "{log}"
    );
    assert!(
        log.contains("7pt is the default font floor") && log.contains("proved for every candidate"),
        "{log}"
    );
    // A starved search cannot prove anything, so the core it finds is flagged as such.
    let (ok, log) = fitd(&[
        src.to_str().unwrap(),
        "-o",
        tmp("core.svg").to_str().unwrap(),
        "--budget",
        "3000",
    ]);
    assert!(
        !ok && log.contains("error[E0110]")
            && log.contains("cut off after 750 checks")
            && log.contains("--budget 12000"),
        "{log}"
    );
}

#[test]
fn chained_orderings_fit_and_overlong_chains_fail_fast() {
    // Forward checking sees one link of a chain at a time, so without arc consistency a
    // satisfiable two-link chain ran out of budget and was reported as an unsat core.
    let base = std::fs::read_to_string("tests/fixtures/cell_biology.fitd").unwrap();
    let chain = |gaps: &[(&str, &str)], gap: u32| {
        gaps.iter()
            .map(|(a, b)| format!("require {a} above {b} gap={gap}em\n"))
            .collect::<String>()
    };
    let links = [
        ("anaerobic", "regulation"),
        ("regulation", "yield"),
        ("yield", "water"),
        ("water", "oxygen"),
    ];
    let fits = tmp("chain-fits.fitd");
    std::fs::write(&fits, base.clone() + &chain(&links[1..3], 30)).unwrap();
    let (ok, log, _) = fitd_cpu(&[
        fits.to_str().unwrap(),
        "-o",
        tmp("chain-fits.svg").to_str().unwrap(),
    ]);
    assert!(ok && log.contains(" 0 errors"), "{log}");
    let tall = tmp("chain-tall.fitd");
    std::fs::write(&tall, base + &chain(&links, 40)).unwrap();
    let (ok, log, took) = fitd_cpu(&[
        tall.to_str().unwrap(),
        "-o",
        tmp("chain-tall.svg").to_str().unwrap(),
    ]);
    assert!(
        !ok && log.contains("error[E0110]: these 3 constraints"),
        "{log}"
    );
    assert!(took <= 3.0, "unsat core took {took:.1} CPU s");
}

#[test]
fn contradictory_orderings_fail_before_search() {
    let src = tmp("cycle-order.fitd");
    std::fs::write(
        &src,
        "node a\nnode b\nnode c\nrequire a left-of b\nrequire b left-of c\nrequire c left-of a\n",
    )
    .unwrap();
    let (ok, log) = fitd(&[src.to_str().unwrap()]);
    assert!(!ok);
    assert!(
        log.contains("error[E0008]: contradictory horizontal constraints"),
        "{log}"
    );
    assert!(!log.contains("body font"), "search should not run\n{log}");
}

#[test]
fn png_is_cropped_to_drawing() {
    let png = tmp("compiler.png");
    let (ok, log) = fitd(&[
        "examples/compiler.fitd",
        "-o",
        png.to_str().unwrap(),
        "--page",
        "a3",
        "--scale",
        "1",
    ]);
    assert!(ok, "{log}");
    let b = std::fs::read(&png).unwrap();
    assert_eq!(&b[..8], b"\x89PNG\r\n\x1a\n");
    let (w, h) = (
        u32::from_be_bytes(b[16..20].try_into().unwrap()),
        u32::from_be_bytes(b[20..24].try_into().unwrap()),
    );
    // An A3 page is 1191x842pt; the layered example is a narrow column well inside it.
    assert!(
        w > 100 && h > 100 && w < 1191 && (w as f64) < 0.8 * h as f64,
        "{w}x{h}"
    );
}

#[test]
fn theme_colors_reach_svg_and_tikz() {
    let src = tmp("theme.fitd");
    std::fs::write(&src, "theme dark background=#102030 palette=\"#FFB74D #4DD0E1\"\nnode c\nnode a fill=#FFE0B2 text=#3E2723\nnode b\nc -> a, b\n").unwrap();
    let (svg, tex) = (tmp("theme.svg"), tmp("theme.tex"));
    let (ok, log) = fitd(&[
        src.to_str().unwrap(),
        "-o",
        svg.to_str().unwrap(),
        "--tikz",
        tex.to_str().unwrap(),
    ]);
    assert!(ok, "{log}");
    let s = std::fs::read_to_string(&svg).unwrap();
    for c in ["#102030", "#4DD0E1", "#FFE0B2", "#3E2723", "#E6EDF3"] {
        assert!(s.contains(c), "{c} missing from svg");
    }
    assert!(
        std::fs::read_to_string(&tex)
            .unwrap()
            .contains("\\definecolor{c102030}")
    );
    let (ok, log) = fitd(&[
        src.to_str().unwrap(),
        "-o",
        svg.to_str().unwrap(),
        "--theme",
        "neon",
    ]);
    assert!(!ok && log.contains("unknown theme `neon`"), "{log}");
}

/// EXAMPLES lists each example with its warning baseline and its solver check budget, about
/// 10% above what it uses. Checks are deterministic, unlike CPU time: hyperthreads sharing a
/// busy core run slower per CPU second, so a CPU budget failed under load with identical
/// output. CPU time is still capped loosely (HANG_SECS) to catch a hang. Lower a number when a
/// change improves on it.
const EXAMPLES: [(&str, usize, u64); 11] = [
    ("coffee", 2, 155_000_000),
    ("compiler", 2, 4_200_000),
    ("debugging", 0, 33_000),
    ("fib_calls", 0, 8_000_000),
    ("fitd_language", 1, 1_700_000),
    ("group_theory", 0, 164_000_000),
    // Built from a user library beside it, `library/runbook.fitd`.
    ("library/oncall", 0, 5_000),
    // Dense layers cross by nature: every W0206 here is an unavoidable crossing.
    ("neural_net", 60, 8_200_000),
    ("pipeline", 0, 94_000),
    ("release_pipeline", 0, 11_700_000),
    ("semester", 0, 34_000),
];

const HANG_SECS: f64 = 60.0;

#[cfg(unix)]
#[repr(C)]
struct Rusage {
    utime: [i64; 2],
    stime: [i64; 2],
    rest: [i64; 14],
}

#[cfg(unix)]
unsafe extern "C" {
    fn wait4(pid: i32, status: *mut i32, options: i32, usage: *mut Rusage) -> i32;
}

/// fitd_cpu runs fitd and also returns the CPU seconds it used across all its threads.
#[cfg(unix)]
#[allow(clippy::zombie_processes, reason = "reaped by wait4")]
fn fitd_cpu(args: &[&str]) -> (bool, String, f64) {
    use std::io::Read;
    let mut child = Command::new(env!("CARGO_BIN_EXE_fitd"))
        .args(args)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut log = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut log)
        .unwrap();
    let (mut status, mut ru) = (
        0,
        Rusage {
            utime: [0; 2],
            stime: [0; 2],
            rest: [0; 14],
        },
    );
    // std's Child::wait drops the child's resource usage, so reap it directly.
    assert_eq!(
        unsafe { wait4(child.id() as i32, &mut status, 0, &mut ru) },
        child.id() as i32
    );
    let secs = |t: [i64; 2]| t[0] as f64 + t[1] as f64 * 1e-6;
    // Exited normally with status 0.
    (status & 0xffff == 0, log, secs(ru.utime) + secs(ru.stime))
}

/// fitd_cpu runs fitd and returns its wall seconds, which stand in for CPU time without wait4.
#[cfg(not(unix))]
fn fitd_cpu(args: &[&str]) -> (bool, String, f64) {
    let start = std::time::Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_fitd"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        start.elapsed().as_secs_f64(),
    )
}

#[test]
fn every_example_builds_within_baseline() {
    for (name, warnings, budget) in EXAMPLES {
        let src = if name == "pipeline" {
            "docs/pipeline.fitd".to_string()
        } else {
            format!("examples/{name}.fitd")
        };
        let slug = name.replace('/', "-");
        let outs = [tmp(&format!("{slug}-a.svg")), tmp(&format!("{slug}-b.svg"))];
        // A second build runs alongside to catch HashMap order or thread timing leaking into
        // the layout.
        let twin = Command::new(env!("CARGO_BIN_EXE_fitd"))
            .args([&src, "-o", outs[1].to_str().unwrap()])
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let (ok, log, took) = fitd_cpu(&[&src, "-o", outs[0].to_str().unwrap()]);
        assert!(
            twin.wait_with_output().unwrap().status.success(),
            "{name}: second build failed"
        );
        assert!(ok && log.contains(" 0 errors"), "{name}\n{log}");
        let got: usize = log
            .split(" errors, ")
            .nth(1)
            .unwrap()
            .split(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            got <= warnings,
            "{name}: {got} warnings, baseline {warnings}\n{log}"
        );
        let checks: u64 = log
            .split(" checks")
            .next()
            .unwrap()
            .rsplit(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            checks <= budget,
            "{name}: {checks} solver checks, budget {budget}\n{log}"
        );
        assert!(took <= HANG_SECS, "{name}: used {took:.1} CPU s");
        assert!(
            std::fs::read(&outs[0]).unwrap() == std::fs::read(&outs[1]).unwrap(),
            "{name}: two builds differ"
        );
    }
}

#[test]
fn fixtures_fail_or_build_as_designed() {
    let (ok, log) = fitd(&[
        "tests/fixtures/stress_test.fitd",
        "-o",
        tmp("stress.svg").to_str().unwrap(),
    ]);
    assert!(!ok && log.contains("error[E0008]"), "{log}");
    let (ok, log) = fitd(&[
        "tests/fixtures/complex_constraints.fitd",
        "-o",
        tmp("complex.svg").to_str().unwrap(),
    ]);
    assert!(ok && log.contains(" 0 errors"), "{log}");
    let (ok, log) = fitd(&[
        "tests/fixtures/missing_glyph.fitd",
        "-o",
        tmp("glyph.svg").to_str().unwrap(),
    ]);
    assert!(
        ok && log.contains("warning[W0202]") && log.contains("missing_glyph.fitd:4:6"),
        "{log}"
    );
}

/// centers reads node centers from a `--stable` lock.
fn centers(lock: &std::path::Path) -> std::collections::HashMap<String, (f64, f64)> {
    let text = std::fs::read_to_string(lock).unwrap();
    text.lines()
        .filter_map(|l| l.strip_prefix("node "))
        .map(|l| {
            let w: Vec<&str> = l.split(' ').collect();
            (
                w[0].to_string(),
                (w[1].parse().unwrap(), w[2].parse().unwrap()),
            )
        })
        .collect()
}

#[test]
fn stable_lock_keeps_nodes_in_place_after_adding_a_leaf() {
    // No font pinned: an edit must not grow the font at the cost of moving most nodes.
    for (dir, name, last) in [
        ("examples", "compiler", "failure"),
        ("tests/fixtures", "cell_biology", "regulation"),
        ("examples", "group_theory", "disprove"),
    ] {
        let lock = tmp(&format!("{name}.lock"));
        let _ = std::fs::remove_file(&lock);
        let (ok, log) = fitd(&[
            &format!("{dir}/{name}.fitd"),
            "-o",
            tmp(&format!("{name}-s0.svg")).to_str().unwrap(),
            "--stable",
            lock.to_str().unwrap(),
        ]);
        assert!(ok, "{log}");
        let before = centers(&lock);
        let src = tmp(&format!("{name}-leaf.fitd"));
        let text = std::fs::read_to_string(format!("{dir}/{name}.fitd")).unwrap();
        assert!(
            text.contains(&format!("node {last} ")) || text.contains(&format!("  {last} \"")),
            "{name} has no node {last}"
        );
        std::fs::write(
            &src,
            text + &format!("\nnode extra \"Extra leaf\"\n{last} -> extra\n"),
        )
        .unwrap();
        let args = [
            src.to_str().unwrap().to_string(),
            "-o".into(),
            tmp(&format!("{name}-s1.svg")).to_str().unwrap().into(),
            "--stable".into(),
            lock.to_str().unwrap().into(),
        ];
        let (ok, log) = fitd(&args.iter().map(String::as_str).collect::<Vec<_>>());
        assert!(ok && log.contains(" 0 errors"), "{log}");
        let after = centers(&lock);
        assert!(after.contains_key("extra"));
        let kept = before.iter().filter(|(id, p)| after[*id] == **p).count();
        assert!(
            kept * 5 >= before.len() * 4,
            "{name}: only {kept} of {} nodes kept their spots\n{log}",
            before.len()
        );
    }
}

#[test]
fn layout_needs_no_fontconfig() {
    let (with, without) = (tmp("fc-with.svg"), tmp("fc-without.svg"));
    assert!(fitd(&["examples/compiler.fitd", "-o", with.to_str().unwrap()]).0);
    let out = Command::new(env!("CARGO_BIN_EXE_fitd"))
        .args(["examples/compiler.fitd", "-o", without.to_str().unwrap()])
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = std::fs::read_to_string(&without).unwrap();
    assert!(
        s.contains("<!-- fitd fonts: DejaVu Sans (embedded:DejaVuSans.ttf) "),
        "{s}"
    );
    assert!(
        s == std::fs::read_to_string(&with).unwrap(),
        "layout depends on fc-match"
    );
}

#[test]
fn open_orientation_turns_and_flags_fix_it() {
    // `compiler` keeps warnings on landscape a3 and has none on portrait.
    let src = tmp("turn.fitd");
    std::fs::write(
        &src,
        std::fs::read_to_string("examples/compiler.fitd")
            .unwrap()
            .replace("page 240x360mm margin=1cm", "page a3 margin=1cm"),
    )
    .unwrap();
    let src = src.to_str().unwrap();
    let (ok, log) = fitd(&[src, "-o", tmp("turn.svg").to_str().unwrap()]);
    assert!(
        ok && log.contains("turned the page to portrait") && log.contains("on 842x1191pt"),
        "{log}"
    );
    let (ok, log) = fitd(&[src, "-o", tmp("turn.svg").to_str().unwrap(), "--landscape"]);
    assert!(
        ok && !log.contains("turned the page") && log.contains("on 1191x842pt"),
        "{log}"
    );
}

#[test]
fn row_alignment_merges_layers_in_flowchart() {
    let src = tmp("row.fitd");
    std::fs::write(&src, "layout layered\npage a4\nnode a\nnode b\nnode c\nnode d\na -> b\nb -> c\nc -> d\nrequire align row(b, c, d)\nrequire b left-of c left-of d\n").unwrap();
    let (ok, log) = fitd(&[
        src.to_str().unwrap(),
        "-o",
        tmp("row.svg").to_str().unwrap(),
    ]);
    assert!(ok, "{log}");
    assert!(log.contains(" 0 errors"), "{log}");
}

#[test]
fn output_directories_are_created() {
    // The README writes to `out/`, which a fresh clone does not have.
    let dir = tmp("fresh").join("out");
    let _ = std::fs::remove_dir_all(&dir);
    let tex = dir.join("compiler.tex");
    let (ok, log) = fitd(&[
        "examples/compiler.fitd",
        "-o",
        dir.join("c.svg").to_str().unwrap(),
        "--tikz",
        tex.to_str().unwrap(),
    ]);
    assert!(ok, "{log}");
    assert!(tex.exists());
}

#[test]
fn undirected_edges_have_no_arrowheads() {
    let src = tmp("undirected.fitd");
    let (svg, tex) = (tmp("undirected.svg"), tmp("undirected.tex"));
    std::fs::write(&src, "layout layered dir=down\nnode a \"A\"\nnode b \"B\"\nnode c \"C\"\nnode d \"D\"\na -> b\na -- c\nb ..> d\nc .. d\n").unwrap();
    let (ok, log) = fitd(&[
        src.to_str().unwrap(),
        "-o",
        svg.to_str().unwrap(),
        "--tikz",
        tex.to_str().unwrap(),
    ]);
    assert!(ok && log.contains(" 0 errors"), "{log}");
    let s = std::fs::read_to_string(&svg).unwrap();
    let edges: Vec<&str> = s.lines().filter(|l| l.starts_with("<polyline")).collect();
    assert_eq!(edges.len(), 4, "{s}");
    assert_eq!(
        edges.iter().filter(|l| l.contains("marker-end")).count(),
        2,
        "{s}"
    );
    assert_eq!(
        edges
            .iter()
            .filter(|l| l.contains("dasharray") && !l.contains("marker-end"))
            .count(),
        1,
        "{s}"
    );
    let t = std::fs::read_to_string(&tex).unwrap();
    assert_eq!(
        t.lines()
            .filter(|l| l.starts_with("\\draw[") && l.contains("Stealth"))
            .count(),
        2,
        "{t}"
    );
}

#[test]
fn guide_examples_build_from_stdin() {
    use std::io::Write;
    let out = Command::new(env!("CARGO_BIN_EXE_fitd"))
        .arg("guide")
        .output()
        .unwrap();
    let guide = String::from_utf8(out.stdout).unwrap();
    // The guide is what an agent reads instead of the doc diagrams, so it must stay small.
    assert!(guide.len() <= 2900, "guide grew to {} bytes", guide.len());
    for name in ["Concept map:", "Flowchart:"] {
        let body: String = guide
            .split(name)
            .nth(1)
            .unwrap()
            .trim_start_matches('\n')
            .lines()
            .take_while(|l| !l.is_empty())
            .map(|l| l.strip_prefix("  ").unwrap_or(l).to_string() + "\n")
            .collect();
        let svg = tmp(&format!("guide-{}.svg", name.len()));
        let mut child = Command::new(env!("CARGO_BIN_EXE_fitd"))
            .args(["-", "-o", svg.to_str().unwrap(), "--short"])
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        let log = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success() && log.contains("0 errors, 0 warnings"),
            "{name}\n{body}\n{log}"
        );
        assert_eq!(
            log.lines().count(),
            2,
            "--short should print a summary and the output path\n{log}"
        );
    }
}

#[test]
fn layers_that_exactly_fill_the_page_fit_and_crowded_flows_say_why() {
    // An agent's first flowchart: at 8pt the layer gaps fill a4 exactly, and rounding once put
    // the last layer a hair off the page, so fitd wrongly said nothing fits.
    let nodes = [
        ("intro", "Bill Introduced", "Introduced in House or Senate"),
        ("committee", "Committee Action", "Review, Markup, and Vote"),
        ("floor", "Floor Vote", "Debate and Vote in Chamber"),
        ("other", "Other Chamber", "Repeated process in House/Senate"),
        (
            "conference",
            "Conference Committee",
            "Resolving differences between versions",
        ),
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
    let down = tmp("bill-down.fitd");
    std::fs::write(&down, &text).unwrap();
    let (ok, log) = fitd(&[
        down.to_str().unwrap(),
        "-o",
        tmp("bill-down.svg").to_str().unwrap(),
    ]);
    assert!(ok && log.contains("body font 8.0pt"), "{log}");
    // Across a portrait page the nine layers do not fit in one line, so the flow wraps.
    let right = tmp("bill-right.fitd");
    std::fs::write(&right, text.replace("dir=down", "dir=right")).unwrap();
    let (ok, log) = fitd(&[
        right.to_str().unwrap(),
        "-o",
        tmp("bill-right.svg").to_str().unwrap(),
    ]);
    assert!(
        ok && log.contains("body font 8.0pt") && log.contains(" 0 errors"),
        "{log}"
    );
    // `bands=1` and `--bands 1` keep the flow in one line, which does not fit here.
    std::fs::write(&right, text.replace("dir=down", "dir=right bands=1")).unwrap();
    let (ok, log) = fitd(&[
        right.to_str().unwrap(),
        "-o",
        tmp("bill-right.svg").to_str().unwrap(),
    ]);
    assert!(
        !ok && log.contains("the longest path has 9 layers"),
        "{log}"
    );
    std::fs::write(&right, text.replace("dir=down", "dir=right")).unwrap();
    let (ok, log) = fitd(&[
        right.to_str().unwrap(),
        "-o",
        tmp("bill-right.svg").to_str().unwrap(),
        "--bands",
        "1",
    ]);
    assert!(
        !ok && log.contains("the longest path has 9 layers"),
        "{log}"
    );
    // On a strip too small to wrap into, the error says why.
    std::fs::write(
        &right,
        text.replace("dir=down", "dir=right")
            .replace("page a4 portrait", "page 400x150pt"),
    )
    .unwrap();
    let (ok, log) = fitd(&[
        right.to_str().unwrap(),
        "-o",
        tmp("bill-right.svg").to_str().unwrap(),
    ]);
    assert!(
        !ok && log.contains("the longest path has 9 layers") && log.contains("dir=down"),
        "{log}"
    );
}

#[test]
fn watch_rebuilds_on_change() {
    let (src, svg) = (tmp("watch.fitd"), tmp("watch.svg"));
    std::fs::write(&src, "node a \"Alpha\"\nnode b \"Beta\"\na -> b\n").unwrap();
    let _ = std::fs::remove_file(&svg);
    let mut child = Command::new(env!("CARGO_BIN_EXE_fitd"))
        .args([
            src.to_str().unwrap(),
            "-o",
            svg.to_str().unwrap(),
            "--watch",
        ])
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let wait_for = |text: &str| {
        let start = std::time::Instant::now();
        while start.elapsed().as_secs() < 30 {
            if std::fs::read_to_string(&svg).is_ok_and(|s| s.contains(text)) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        false
    };
    let first = wait_for(">Beta<");
    // Some filesystems keep whole-second modification times.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(&src, "node a \"Alpha\"\nnode b \"Gamma\"\na -> b\n").unwrap();
    let second = wait_for(">Gamma<");
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(
        first && second,
        "watch did not rebuild (first {first}, second {second})"
    );
    let leftovers = std::fs::read_dir(svg.parent().unwrap())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("fitd-tmp")
        })
        .count();
    assert_eq!(leftovers, 0, "temporary output files left behind");
}

#[test]
fn watch_cancels_a_slow_build_on_change() {
    use std::io::Read;
    let (src, svg) = (tmp("watch-slow.fitd"), tmp("watch-slow.svg"));
    std::fs::copy("tests/fixtures/earth_science.fitd", &src).unwrap();
    let _ = std::fs::remove_file(&svg);
    let mut child = Command::new(env!("CARGO_BIN_EXE_fitd"))
        .args([
            src.to_str().unwrap(),
            "-o",
            svg.to_str().unwrap(),
            "--watch",
        ])
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut err = child.stderr.take().unwrap();
    let log = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let start = std::time::Instant::now();
    while !std::fs::read_to_string(&svg).is_ok_and(|s| s.contains("</svg>"))
        && start.elapsed().as_secs() < 60
    {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    // The draft is out; the full build starts after the idle time and runs for seconds.
    std::thread::sleep(std::time::Duration::from_millis(2000));
    std::fs::write(&src, "node a \"Alpha\"\nnode b \"Swapped\"\na -> b\n").unwrap();
    let start = std::time::Instant::now();
    while !std::fs::read_to_string(&svg).is_ok_and(|s| s.contains(">Swapped<"))
        && start.elapsed().as_secs() < 30
    {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let swapped = std::fs::read_to_string(&svg).unwrap().contains(">Swapped<");
    // Let the small file's full build finish so nothing is mid-write when the watcher dies.
    std::thread::sleep(std::time::Duration::from_millis(2500));
    child.kill().unwrap();
    child.wait().unwrap();
    let log = log.join().unwrap();
    assert!(swapped && log.contains("--- cancelled"), "{log}");
    assert!(
        std::fs::read_to_string(&svg).unwrap().contains(">Swapped<"),
        "the cancelled build overwrote the new one\n{log}"
    );
}

/// Lsp drives `fitd lsp` over its stdin and stdout.
struct Lsp {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: std::io::BufReader<std::process::ChildStdout>,
}

impl Lsp {
    fn start() -> Lsp {
        let mut child = Command::new(env!("CARGO_BIN_EXE_fitd"))
            .arg("lsp")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let (stdin, stdout) = (
            child.stdin.take().unwrap(),
            std::io::BufReader::new(child.stdout.take().unwrap()),
        );
        let mut l = Lsp {
            child,
            stdin,
            stdout,
        };
        l.send(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#.into());
        assert!(l.recv().contains("definitionProvider"));
        l
    }

    fn send(&mut self, body: String) {
        use std::io::Write;
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    }

    fn recv(&mut self) -> String {
        use std::io::{BufRead, Read};
        let mut len = 0;
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).unwrap();
            if line.trim().is_empty() {
                break;
            }
            len = line
                .trim()
                .strip_prefix("Content-Length: ")
                .map_or(len, |n| n.parse().unwrap());
        }
        let mut body = vec![0; len];
        self.stdout.read_exact(&mut body).unwrap();
        String::from_utf8(body).unwrap()
    }

    fn open(&mut self, uri: &str, text: &str) {
        self.send(format!(r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{uri}","languageId":"fitd","version":1,"text":"{}"}}}}}}"#, esc(text)));
    }

    fn change(&mut self, uri: &str, text: &str) {
        self.send(format!(r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"{uri}","version":2}},"contentChanges":[{{"text":"{}"}}]}}}}"#, esc(text)));
    }

    fn save(&mut self, uri: &str) {
        self.send(format!(r#"{{"jsonrpc":"2.0","method":"textDocument/didSave","params":{{"textDocument":{{"uri":"{uri}"}}}}}}"#));
    }

    fn stop(mut self) {
        self.send(r#"{"jsonrpc":"2.0","id":99,"method":"shutdown"}"#.into());
        self.recv();
        self.send(r#"{"jsonrpc":"2.0","method":"exit"}"#.into());
        assert!(self.child.wait().unwrap().success());
    }
}

/// file_uri is the `file://` uri of a path, with Windows separators and drive turned around.
fn file_uri(p: &std::path::Path) -> String {
    let s = p.display().to_string().replace('\\', "/");
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{s}")
    }
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

#[test]
fn language_server_reports_and_navigates() {
    let src = tmp("lsp.fitd");
    let text = "node a \"Alpha\"\nnode b \"Beta\"\na -> b\na -> ghost\n";
    std::fs::write(&src, text).unwrap();
    let uri = file_uri(&src);
    let mut l = Lsp::start();
    l.open(&uri, text);
    let diags = l.recv();
    assert!(
        diags.contains("E0001") && diags.contains(r#""line":3"#),
        "{diags}"
    );
    l.send(format!(r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/definition","params":{{"textDocument":{{"uri":"{uri}"}},"position":{{"line":2,"character":5}}}}}}"#));
    let def = l.recv();
    assert!(def.contains(r#""start":{"character":5,"line":1}"#), "{def}");
    // Fixed on disk and saved: the full layout finds only the uncovered glyph.
    let fixed = "node a \"Alpha\"\nnode b \"Beta 𐀀\"\na -> b\n";
    std::fs::write(&src, fixed).unwrap();
    l.change(&uri, fixed);
    assert!(l.recv().contains(r#""diagnostics":[]"#));
    l.save(&uri);
    let saved = l.recv();
    assert!(
        saved.contains("W0202") && saved.contains(r#""line":1"#),
        "{saved}"
    );
    l.stop();
}

#[test]
fn language_server_follows_includes() {
    let dir = tmp("lsp-inc");
    std::fs::create_dir_all(&dir).unwrap();
    let (main, part) = (dir.join("main.fitd"), dir.join("part.fitd"));
    std::fs::write(&part, "node far \"Far 𐀀\"\nfar -> ghost\n").unwrap();
    let text = "node a \"A\"\ninclude \"part.fitd\"\na -> far\n";
    std::fs::write(&main, text).unwrap();
    let (uri, part_uri) = (file_uri(&main), file_uri(&part));
    let mut l = Lsp::start();
    l.open(&uri, text);
    let own = l.recv();
    assert!(
        own.contains(&uri) && own.contains(r#""diagnostics":[]"#),
        "{own}"
    );
    let inc = l.recv();
    assert!(
        inc.contains(&part_uri) && inc.contains("E0001") && inc.contains(r#""line":1"#),
        "{inc}"
    );
    l.send(format!(r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/definition","params":{{"textDocument":{{"uri":"{uri}"}},"position":{{"line":2,"character":6}}}}}}"#));
    let def = l.recv();
    assert!(
        def.contains(&part_uri) && def.contains(r#""start":{"character":5,"line":0}"#),
        "{def}"
    );
    // Fixed: the include's old diagnostics are cleared.
    std::fs::write(&part, "node far \"Far 𐀀\"\n").unwrap();
    l.change(&uri, text);
    assert!(l.recv().contains(&uri));
    let cleared = l.recv();
    assert!(
        cleared.contains(&part_uri) && cleared.contains(r#""diagnostics":[]"#),
        "{cleared}"
    );
    // The full layout's warning about the include's glyph lands on the include.
    l.save(&uri);
    let own = l.recv();
    assert!(
        own.contains(&uri) && own.contains(r#""diagnostics":[]"#),
        "{own}"
    );
    let inc = l.recv();
    assert!(
        inc.contains(&part_uri) && inc.contains("W0202") && inc.contains(r#""line":0"#),
        "{inc}"
    );
    l.stop();
}

#[test]
fn includes_splice_files_and_keep_their_names_in_diagnostics() {
    let dir = tmp("inc");
    std::fs::create_dir_all(dir.join("parts")).unwrap();
    std::fs::write(
        dir.join("parts/more.fitd"),
        "node c \"Gamma\"\nc -> ghost\n",
    )
    .unwrap();
    let main = dir.join("main.fitd");
    std::fs::write(
        &main,
        "layout layered\nnode a \"Alpha\"\ninclude \"parts/more.fitd\"\na -> c\n",
    )
    .unwrap();
    let (ok, log) = fitd(&[
        main.to_str().unwrap(),
        "-o",
        dir.join("m.svg").to_str().unwrap(),
        "--short",
    ]);
    assert!(
        !ok && log.contains("parts/more.fitd:2:6: error[E0001]: unknown node `ghost`"),
        "{log}"
    );
    std::fs::write(
        dir.join("parts/more.fitd"),
        "node c \"Gamma\"\ninclude \"../main.fitd\"\n",
    )
    .unwrap();
    let (ok, log) = fitd(&[
        main.to_str().unwrap(),
        "-o",
        dir.join("m.svg").to_str().unwrap(),
        "--short",
    ]);
    assert!(
        !ok && log.contains("parts/more.fitd:2:1: error[E0011]"),
        "{log}"
    );
    std::fs::write(dir.join("parts/more.fitd"), "node c \"Gamma\"\n").unwrap();
    let (ok, log) = fitd(&[
        main.to_str().unwrap(),
        "-o",
        dir.join("m.svg").to_str().unwrap(),
    ]);
    assert!(ok && log.contains(" 0 errors"), "{log}");
    assert!(
        std::fs::read_to_string(dir.join("m.svg"))
            .unwrap()
            .contains(">Gamma<")
    );
}

#[test]
fn imported_sources_build_cleanly() {
    let (ok, log) = fitd(&[
        "import",
        "tests/fixtures/import_mindmap.mmd",
        "-o",
        tmp("mindmap.fitd").to_str().unwrap(),
    ]);
    assert!(ok && log.contains("icons"), "{log}");
    let (ok, log) = fitd(&[
        tmp("mindmap.fitd").to_str().unwrap(),
        "-o",
        tmp("mindmap.svg").to_str().unwrap(),
    ]);
    assert!(
        ok && log.contains("13 nodes, 12 edges; 0 errors, 0 warnings"),
        "{log}"
    );
    for (src, dropped) in [
        ("import.dot", "edge attribute `constraint=false`"),
        ("import.mmd", "`classDef` statements"),
        ("import.d2", "shape `cylinder`"),
    ] {
        let out = tmp(&format!("{src}.fitd"));
        let (ok, log) = fitd(&[
            "import",
            &format!("tests/fixtures/{src}"),
            "-o",
            out.to_str().unwrap(),
        ]);
        assert!(ok && log.contains(dropped), "{log}");
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains("group \"") && text.contains("..>"), "{text}");
        let (ok, log) = fitd(&[
            out.to_str().unwrap(),
            "-o",
            tmp(&format!("{src}.svg")).to_str().unwrap(),
        ]);
        assert!(
            ok && log.contains(" 0 errors, 0 warnings"),
            "{src}\n{text}\n{log}"
        );
    }
}

#[test]
fn split_pages_each_build_alone() {
    let dir = tmp("split");
    let _ = std::fs::remove_dir_all(&dir);
    let (ok, log) = fitd(&[
        "split",
        "examples/group_theory.fitd",
        "-o",
        dir.to_str().unwrap(),
    ]);
    assert!(ok && !log.contains("left out"), "{log}");
    let overview = std::fs::read_to_string(dir.join("group_theory.fitd")).unwrap();
    assert!(
        overview.contains("| see page group_theory-cyc"),
        "{overview}"
    );
    let cos = std::fs::read_to_string(dir.join("group_theory-cos.fitd")).unwrap();
    assert!(
        cos.contains("corr ..> classif")
            && cos.contains("node classif \"Classification\"\n| see page group_theory-iso"),
        "{cos}"
    );
    let mut pages = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.to_string_lossy().ends_with("-shared.fitd") {
            continue;
        }
        let (ok, log) = fitd(&[
            p.to_str().unwrap(),
            "-o",
            p.with_extension("svg").to_str().unwrap(),
        ]);
        assert!(
            ok && log.contains(" 0 errors, 0 warnings"),
            "{}\n{log}",
            p.display()
        );
        pages += 1;
    }
    assert_eq!(pages, 6);
    let (ok, log) = fitd(&[
        "split",
        "tests/fixtures/stress_test.fitd",
        "-o",
        dir.to_str().unwrap(),
    ]);
    assert!(!ok && log.contains("has errors"), "{log}");
}

#[test]
fn readme_example_builds() {
    let readme = std::fs::read_to_string("README.md").unwrap();
    let src = readme
        .split_once("```fitd\n")
        .and_then(|(_, rest)| rest.split_once("\n```"))
        .map(|(src, _)| src)
        .expect("README must contain a fitd example");
    let input = tmp("readme-example.fitd");
    std::fs::write(&input, src).unwrap();
    let (ok, log) = fitd(&[
        input.to_str().unwrap(),
        "-o",
        tmp("readme-example.svg").to_str().unwrap(),
    ]);
    assert!(ok, "{log}");
}

/// glob expands one `dir/prefix*suffix` pattern, sorted as a shell would.
fn glob(pattern: &str) -> Vec<String> {
    let Some((pre, suf)) = pattern.split_once('*') else {
        return vec![pattern.to_string()];
    };
    let (dir, pre) = pre.rsplit_once('/').unwrap();
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(pre) && n.ends_with(suf))
        .map(|n| format!("{dir}/{n}"))
        .collect();
    out.sort();
    out
}

#[test]
fn committed_docs_match_a_fresh_build() {
    // Doc pages render beside their source.
    let mut jobs: Vec<(String, String)> = Vec::new();
    for dir in ["docs/language", "docs/options", "docs/internals"] {
        for f in glob(&format!("{dir}/*.fitd")) {
            jobs.push((f.clone(), f.replace(".fitd", ".png")));
        }
    }
    jobs.push(("docs/pipeline.fitd".into(), "docs/pipeline.png".into()));
    for (src, committed) in &jobs {
        let fresh = tmp(&committed.replace('/', "-"));
        let (ok, log) = fitd(&[src, "-o", fresh.to_str().unwrap()]);
        assert!(ok, "{src}\n{log}");
        assert!(!log.contains("warning["), "{src}\n{log}");
        assert!(
            std::fs::read(committed).ok() == std::fs::read(&fresh).ok(),
            "{committed} is stale; rebuild it from {src}"
        );
    }
    for src in glob("examples/*.fitd") {
        let fresh = tmp(&src.replace('/', "-"));
        let (ok, log) = fitd(&[&src, "-o", fresh.to_str().unwrap()]);
        assert!(ok, "{src}\n{log}");
    }
}

#[test]
fn gallery_and_every_std_module_compile_cleanly() {
    for src in glob("examples/gallery/*.fitd") {
        let fresh = tmp(&src.replace('/', "-").replace(".fitd", ".svg"));
        let (ok, log) = fitd(&[&src, "-o", fresh.to_str().unwrap()]);
        assert!(ok && !log.contains("warning["), "{src}\n{log}");
    }
    for path in glob("src/std/*.fitd") {
        let module = path.trim_start_matches("src/std/").trim_end_matches(".fitd");
        let name = format!("std-{module}.fitd");
        let input = tmp(&name);
        std::fs::write(
            &input,
            format!("include \"{module}\"\nnode a\nnode b\na -> b\n"),
        )
        .unwrap();
        let out = tmp(&name.replace(".fitd", ".svg"));
        let (ok, log) = fitd(&[input.to_str().unwrap(), "-o", out.to_str().unwrap()]);
        assert!(ok && !log.contains("warning["), "{module}\n{log}");
    }
}

#[test]
fn several_inputs_make_one_pdf() {
    let pdf = tmp("book.pdf");
    let (ok, log) = fitd(&[
        "docs/pipeline.fitd",
        "examples/coffee.fitd",
        "examples/semester.fitd",
        "-o",
        pdf.to_str().unwrap(),
    ]);
    assert!(ok && log.contains("(3 diagrams on 3 pages)"), "{log}");
    let (ok, log) = fitd(&[
        "docs/pipeline.fitd",
        "examples/coffee.fitd",
        "examples/semester.fitd",
        "--grid",
        "2x1",
        "--page",
        "a3",
        "-o",
        pdf.to_str().unwrap(),
    ]);
    assert!(
        ok && log.contains("(3 diagrams on 2 pages)") && log.matches("on 595x842pt").count() == 3,
        "{log}"
    );
    let (ok, log) = fitd(&[
        "docs/pipeline.fitd",
        "examples/coffee.fitd",
        "-o",
        tmp("book.svg").to_str().unwrap(),
    ]);
    assert!(!ok && log.contains("need -o FILE.pdf"), "{log}");
    let (ok, log) = fitd(&[
        "docs/pipeline.fitd",
        "examples/coffee.fitd",
        "--tikz",
        "x.tex",
        "-o",
        pdf.to_str().unwrap(),
    ]);
    assert!(!ok && log.contains("takes one input"), "{log}");
    let (ok, log) = fitd(&[
        "docs/pipeline.fitd",
        "tests/fixtures/stress_test.fitd",
        "-o",
        tmp("bad.pdf").to_str().unwrap(),
    ]);
    assert!(
        !ok && log.contains("1 of 2 diagrams failed") && !tmp("bad.pdf").exists(),
        "{log}"
    );
    let (ok, log) = fitd(&[
        "docs/pipeline.fitd",
        "--grid",
        "2",
        "-o",
        pdf.to_str().unwrap(),
    ]);
    assert!(!ok && log.contains("COLSxROWS"), "{log}");
}

#[test]
fn macros_expand_loops_templates_and_recursion() {
    let svg = tmp("collatz.svg");
    let (ok, log) = fitd(&["tests/fixtures/collatz.fitd", "-o", svg.to_str().unwrap()]);
    assert!(
        ok && log.contains(" 0 errors") && log.contains("9 nodes, 8 edges"),
        "{log}"
    );
    let s = std::fs::read_to_string(&svg).unwrap();
    assert!(s.contains(">16<") && s.contains(">1<"), "{s}");
    let (ok, log) = fitd(&[
        "tests/fixtures/collatz.fitd",
        "-o",
        svg.to_str().unwrap(),
        "--expand-budget",
        "20",
        "--short",
    ]);
    assert!(
        !ok && log.contains("error[E0013]: macro expansion ran past 20 steps"),
        "{log}"
    );

    let dir = tmp("macros");
    std::fs::create_dir_all(dir.join("parts")).unwrap();
    std::fs::write(
        dir.join("parts/t.fitd"),
        "def box(id)\n  node ${id}\n  ${id} -> ghost\nend\n",
    )
    .unwrap();
    let main = dir.join("main.fitd");
    std::fs::write(&main, "include \"parts/t.fitd\"\nfor s in a, b\n  use box(s)\nend\nuse box(\"c\", 2)\ndef f(n)\n  use f(n + 1)\nend\nuse f(0)\nif 1\n").unwrap();
    let (ok, log) = fitd(&[
        main.to_str().unwrap(),
        "-o",
        dir.join("m.svg").to_str().unwrap(),
        "--short",
    ]);
    // A generated line reports at the template line, in the file that holds the template.
    assert!(
        !ok && log.contains("parts/t.fitd:3:6: error[E0001]: unknown node `ghost`"),
        "{log}"
    );
    assert!(
        log.contains("main.fitd:5:1: error[E0012]: `box` takes 1 argument (id), found 2"),
        "{log}"
    );
    assert!(
        log.contains("main.fitd:7:3: error[E0012]: templates nest more than"),
        "{log}"
    );
    assert!(
        log.contains("main.fitd:10:1: error[E0002]: `if` has no matching `end`"),
        "{log}"
    );
}

#[test]
fn figure_rows_draw_cells_and_need_a_node() {
    let plain = tmp("figplain.fitd");
    std::fs::write(&plain, "node a \"A\"\nnode b\na -> b\n").unwrap();
    let grid = tmp("figgrid.fitd");
    std::fs::write(
        &grid,
        "node a \"A\"\n~ # . _ GLU\n~ #\"two words\" . . .\nnode b\na -> b\n",
    )
    .unwrap();
    let count = |src: &std::path::Path, out: &str| {
        let o = tmp(out);
        let (ok, log) = fitd(&[src.to_str().unwrap(), "-o", o.to_str().unwrap()]);
        assert!(ok, "{log}");
        let s = std::fs::read_to_string(&o).unwrap();
        (s.matches("<rect").count(), s)
    };
    let (base, _) = count(&plain, "figplain.svg");
    let (with, s) = count(&grid, "figgrid.svg");
    // 7 drawn cells (the `_` gap draws nothing), and a solid one adds a shade rect under its outline.
    assert_eq!(with, base + 9, "cell rects missing");
    assert!(s.contains("GLU") && s.contains("two words"), "{s}");

    let tex = tmp("figgrid.tex");
    let (ok, log) = fitd(&[
        grid.to_str().unwrap(),
        "-o",
        tmp("figgrid2.svg").to_str().unwrap(),
        "--tikz",
        tex.to_str().unwrap(),
    ]);
    assert!(ok, "{log}");
    assert!(std::fs::read_to_string(&tex).unwrap().contains("GLU"));

    let orphan = tmp("figorphan.fitd");
    std::fs::write(&orphan, "~ # .\nnode a\nnode b\na -> b\n").unwrap();
    let (ok, log) = fitd(&[
        orphan.to_str().unwrap(),
        "-o",
        tmp("figorphan.svg").to_str().unwrap(),
        "--short",
    ]);
    assert!(!ok && log.contains("figure row outside a node"), "{log}");
}

#[test]
fn shaded_cells_span_columns_and_nodes_stack() {
    let widths = |src: &str, out: &str| {
        let f = tmp(&format!("{out}.fitd"));
        std::fs::write(&f, src).unwrap();
        let o = tmp(&format!("{out}.svg"));
        let (ok, log) = fitd(&[f.to_str().unwrap(), "-o", o.to_str().unwrap(), "--short"]);
        assert!(ok, "{log}");
        let svg = std::fs::read_to_string(&o).unwrap();
        // The root <svg> carries a unit suffix, so keep only the plain numeric widths.
        let w: Vec<f64> = svg
            .match_indices("width=\"")
            .filter_map(|(i, m)| svg[i + m.len()..].split('"').next()?.parse().ok())
            .collect();
        (svg, w)
    };
    let (svg, _) = widths("node a \"A\"\n~ %60 %0 #\n node b\na -> b\n", "figshade");
    assert!(svg.contains("fill-opacity=\"0.60\""), "no 60% cell\n{svg}");
    assert!(
        svg.contains("fill-opacity=\"1.00\""),
        "no solid cell\n{svg}"
    );

    let (_, wide) = widths(
        "node a \"A\"\n~ . . .\n~ #\"x\"*3\nnode b\na -> b\n",
        "figspan",
    );
    let cell = wide.iter().copied().fold(f64::NAN, f64::min);
    assert!(
        wide.iter().any(|w| *w > 2.9 * cell),
        "no cell spans three columns: {wide:?}"
    );

    let (flat, _) = widths("node a \"A\" shape=box\nnode b\na -> b\n", "figflat");
    let (stacked, _) = widths(
        "node a \"A\" shape=box stack=3\nnode b\na -> b\n",
        "figstack",
    );
    assert_eq!(
        stacked.matches("<rect").count(),
        flat.matches("<rect").count() + 2,
        "a stack of 3 draws two copies behind the box"
    );

    let bad = tmp("figstackbad.fitd");
    std::fs::write(&bad, "node a stack=9\nnode b\na -> b\n").unwrap();
    let (ok, log) = fitd(&[
        bad.to_str().unwrap(),
        "-o",
        tmp("figstackbad.svg").to_str().unwrap(),
        "--short",
    ]);
    assert!(!ok && log.contains("1 to 8"), "{log}");
}

#[test]
fn library_modules_are_included_by_name() {
    let f = tmp("usestd.fitd");
    std::fs::write(
        &f,
        "include \"nn\"\nnn.input(\"x\", \"Tokens\", \"B x L\")\nnn.layer(\"e\", \"Encoder\", \"d 64\", 2)\nnn.mask(\"m\", \"Causal\", 4, 1)\nnn.block\n  b \"Block\"\nx -> e -> m\n",
    )
    .unwrap();
    let o = tmp("usestd.svg");
    let (ok, log) = fitd(&[f.to_str().unwrap(), "-o", o.to_str().unwrap(), "--short"]);
    assert!(ok && log.contains("0 errors, 0 warnings"), "{log}");
    let svg = std::fs::read_to_string(&o).unwrap();
    assert!(svg.contains("d 64") && svg.contains("B x L"), "{svg}");
    // 4x4 causal mask: 10 solid cells, so 10 shade rects among the outlines.
    assert_eq!(svg.matches("fill-opacity=\"1.00\"").count(), 12, "{svg}");

    let bad = tmp("badstd.fitd");
    std::fs::write(&bad, "include \"nope\"\nnode a\nnode b\na -> b\n").unwrap();
    let (ok, log) = fitd(&[
        bad.to_str().unwrap(),
        "-o",
        tmp("badstd.svg").to_str().unwrap(),
        "--short",
    ]);
    assert!(!ok && log.contains("no library module"), "{log}");

    let build = |name: &str, src: &str| {
        let f = tmp(&format!("{name}.fitd"));
        std::fs::write(&f, src).unwrap();
        let o = tmp(&format!("{name}.svg"));
        fitd(&[f.to_str().unwrap(), "-o", o.to_str().unwrap(), "--short"])
    };
    // Module templates live under the module name, so a file's own `input` does not clash.
    let (ok, log) = build(
        "stdown",
        "include \"nn\"\ndef input(id)\nnode ${id}\nend\nuse input(\"a\")\nnn.input(\"x\", \"X\", \"1\")\na -> x\n",
    );
    assert!(ok && log.contains("0 errors"), "{log}");
    let (ok, log) = build("stdold", "include \"std/nn.fitd\"\nnode a\n");
    assert!(!ok && log.contains("cannot read"), "{log}");
    let (ok, log) = build(
        "stdoldcall",
        "include \"nn\"\nuse nn_input(\"x\", \"X\", \"1\")\nnode a\n",
    );
    assert!(!ok && log.contains("unknown template `nn_input`"), "{log}");
}

#[test]
fn user_libraries_are_included_beside_the_file() {
    let dir = tmp("userlib");
    std::fs::create_dir_all(&dir).unwrap();
    let write = |name: &str, src: &str| {
        let f = dir.join(name);
        std::fs::write(&f, src).unwrap();
        f
    };
    // A library calls templates it includes by their module name, and a shared library splices once.
    write(
        "shapes.fitd",
        "# shapes: boxes for tests\ninclude \"user/base\"\ndef pill(id, name)\nbase.tag(id, name)\nend\n",
    );
    write("base.fitd", "def tag(id, name)\nnode ${id} \"${name}\"\nend\n");
    let main = write(
        "main.fitd",
        "include \"user/base\"\ninclude \"user/shapes\"\nshapes.pill(\"a\", \"Alpha\")\nbase.tag(\"b\", \"Beta\")\na -> b\n",
    );
    let o = dir.join("main.svg");
    let (ok, log) = fitd(&[main.to_str().unwrap(), "-o", o.to_str().unwrap(), "--short"]);
    assert!(ok && log.contains("0 errors, 0 warnings"), "{log}");
    let svg = std::fs::read_to_string(&o).unwrap();
    assert!(svg.contains("Alpha") && svg.contains("Beta"), "{svg}");

    // A library's templates are not global, so the bare name is unknown outside it.
    let bare = write("bare.fitd", "include \"user/shapes\"\nuse pill(\"a\", \"A\")\n");
    let (ok, log) = fitd(&[bare.to_str().unwrap(), "-o", o.to_str().unwrap(), "--short"]);
    assert!(!ok && log.contains("unknown template `pill`"), "{log}");

    let missing = write("missing.fitd", "include \"user/nope\"\nnode a\n");
    let (ok, log) = fitd(&[missing.to_str().unwrap(), "-o", o.to_str().unwrap(), "--short"]);
    assert!(!ok && log.contains("cannot read") && log.contains("nope.fitd"), "{log}");

    let badname = write("badname.fitd", "include \"user/a/b\"\nnode a\n");
    let (ok, log) = fitd(&[badname.to_str().unwrap(), "-o", o.to_str().unwrap(), "--short"]);
    assert!(!ok && log.contains("bad library name"), "{log}");
}

#[test]
fn repeated_groups_are_dashed_and_frameless_nodes_lose_their_box() {
    let build = |src: &str, name: &str| {
        let f = tmp(&format!("{name}.fitd"));
        std::fs::write(&f, src).unwrap();
        let (svg, tex) = (tmp(&format!("{name}.svg")), tmp(&format!("{name}.tex")));
        let (ok, log) = fitd(&[
            f.to_str().unwrap(),
            "-o",
            svg.to_str().unwrap(),
            "--tikz",
            tex.to_str().unwrap(),
            "--short",
        ]);
        assert!(ok, "{log}");
        (
            std::fs::read_to_string(&svg).unwrap(),
            std::fs::read_to_string(&tex).unwrap(),
        )
    };
    let body = "layout layered\npage a4\nnode a\nnode b\nnode c\na -> b\nb -> c\n";
    let (plain, ptex) = build(&format!("{body}group \"Layers\" b, c\n"), "grpplain");
    assert!(
        !plain.contains("stroke-dasharray") && !ptex.contains("dash pattern"),
        "{plain}"
    );

    let (rep, rtex) = build(&format!("{body}group \"Layers\" b, c times=3\n"), "grprep");
    assert!(
        rep.contains("stroke-dasharray") && rep.contains(">3x Layers<"),
        "{rep}"
    );
    assert!(
        rtex.contains("dash pattern") && rtex.contains("3x Layers"),
        "{rtex}"
    );

    let bad = tmp("grpbad.fitd");
    std::fs::write(&bad, format!("{body}group b, c times=1\n")).unwrap();
    let (ok, log) = fitd(&[
        bad.to_str().unwrap(),
        "-o",
        tmp("grpbad.svg").to_str().unwrap(),
        "--short",
    ]);
    assert!(!ok && log.contains("2 to 99"), "{log}");

    // A frameless node draws its text but no box, so the rect count drops by one.
    let boxed = "layout layered\npage a4\nnode a \"Lane\" shape=box\nnode b\na -> b\n";
    let (bsvg, btex) = build(boxed, "noframeboxed");
    let (nsvg, ntex) = build(&boxed.replace("shape=box", "shape=none"), "noframe");
    assert!(nsvg.contains(">Lane<") && ntex.contains("Lane"), "{nsvg}");
    assert_eq!(
        nsvg.matches("<rect").count() + 2,
        bsvg.matches("<rect").count(),
        "frameless node still draws a box"
    );
    assert_eq!(
        ntex.matches("\\filldraw").count() + 1,
        btex.matches("\\filldraw").count(),
        "frameless node still draws a box in TikZ"
    );
}

/// Stroked node outlines as (x, y, w, h), in source order.
fn outlines(svg: &str) -> Vec<(f64, f64, f64, f64)> {
    svg.split("<rect ")
        .skip(1)
        .map(|r| format!(" {}", &r[..r.find('>').unwrap()]))
        .filter(|r| r.contains("stroke=") && r.contains("rx="))
        .map(|r| {
            let f = |k: &str| -> f64 {
                let i = r
                    .find(&format!(" {k}=\""))
                    .map(|i| i + k.len() + 3)
                    .unwrap();
                r[i..i + r[i..].find('"').unwrap()].parse().unwrap()
            };
            (f("x"), f("y"), f("width"), f("height"))
        })
        .collect()
}

#[test]
fn aside_nodes_sit_after_the_flow_and_wrap_widens_one_node() {
    let flow = "layout layered dir=right\npage 200x120mm\nnode a \"A\"\nnode enc \"Encoder\"\nnode out \"Out\"\na -> enc -> out\n";
    let note = "| next-token objective on all unique sequences\npre ..> enc \"init\"\n";
    let build = |name: &str, pre: &str| {
        let src = tmp(&format!("{name}.fitd"));
        std::fs::write(&src, format!("{flow}{pre}\n{note}")).unwrap();
        let svg = tmp(&format!("{name}.svg"));
        let (ok, log) = fitd(&[
            src.to_str().unwrap(),
            "-o",
            svg.to_str().unwrap(),
            "--short",
        ]);
        assert!(ok && log.contains(" 0 warnings"), "{log}");
        outlines(&std::fs::read_to_string(&svg).unwrap())
    };
    let aside = build("aside", "node pre \"Pretraining\" wrap=34em place=aside");
    assert_eq!(aside.len(), 4, "{aside:?}");
    let flow_bottom = aside[..3].iter().map(|r| r.1 + r.3).fold(0.0, f64::max);
    assert!(
        aside[3].1 >= flow_bottom,
        "aside node overlaps the flow: {aside:?}"
    );

    let narrow = build(
        "aside-narrow",
        "node pre \"Pretraining\" wrap=8em place=aside",
    );
    assert!(
        narrow[3].2 < aside[3].2,
        "wrap= did not change width: {narrow:?} vs {aside:?}"
    );
}

#[test]
fn aside_on_a_flow_node_warns() {
    let src = tmp("aside-flow.fitd");
    std::fs::write(&src, "layout layered\nnode a\nnode b place=aside\na -> b\n").unwrap();
    let (ok, log) = fitd(&[
        src.to_str().unwrap(),
        "-o",
        tmp("aside-flow.svg").to_str().unwrap(),
        "--short",
    ]);
    assert!(ok && log.contains("warning[W0208]"), "{log}");
}

#[test]
fn ratio_page_is_smallest_that_fits() {
    let src = tmp("ratio.fitd");
    std::fs::write(
        &src,
        "layout layered dir=down\nfont 10pt\nnode a \"Start here\"\nnode b \"Then this\"\nnode c \"Or that\"\nnode d \"Done\"\na -> b, c\nb -> d\nc -> d\n",
    )
    .unwrap();
    let svg = tmp("ratio.svg");
    let (ok, log) = fitd(&[
        src.to_str().unwrap(),
        "-o",
        svg.to_str().unwrap(),
        "--page",
        "16:9",
    ]);
    assert!(ok, "{log}");
    let dims = log
        .split("fits on ")
        .nth(1)
        .and_then(|r| r.split("pt").next())
        .and_then(|r| r.split_once('x'))
        .map(|(w, h)| (w.parse::<f64>().unwrap(), h.parse::<f64>().unwrap()))
        .expect(&log);
    assert!((dims.0 / dims.1 - 16.0 / 9.0).abs() < 0.01, "{dims:?}");
    // Four short boxes at a fixed 10pt font need far less than an A4 sheet.
    assert!(dims.0 < 842.0 && dims.1 < 595.0, "{dims:?}");
    assert!(
        log.contains(&format!("on {:.0}x{:.0}pt", dims.0, dims.1)),
        "{log}"
    );
    let smaller = format!("{:.0}x{:.0}", dims.0 * 0.7, dims.1 * 0.7);
    let (ok, log) = fitd(&[
        src.to_str().unwrap(),
        "-o",
        svg.to_str().unwrap(),
        "--page",
        &smaller,
    ]);
    assert!(!ok, "a page 30% smaller should not fit at the floor: {log}");
}

#[test]
fn source_order_keeps_children_in_edge_order() {
    // Nodes are declared in reverse, so only edge order can put the leaves a..e left to right.
    let src = tmp("source-order.fitd");
    std::fs::write(
        &src,
        "layout layered order=source\nnode e \"E\"\nnode d \"D\"\nnode c \"C\"\nnode b \"B\"\nnode a \"A\"\nnode p \"P\"\np -> a, b, c, d, e\n",
    )
    .unwrap();
    let svg = tmp("source-order.svg");
    let (ok, log) = fitd(&[
        src.to_str().unwrap(),
        "-o",
        svg.to_str().unwrap(),
        "--short",
    ]);
    assert!(ok, "{log}");
    let xs: Vec<f64> = outlines(&std::fs::read_to_string(&svg).unwrap())
        .iter()
        .take(5)
        .map(|o| o.0)
        .collect();
    assert!(
        xs.windows(2).all(|w| w[0] > w[1]),
        "leaves out of edge order: {xs:?}"
    );
}

#[test]
fn diamond_edges_end_on_the_outline() {
    let src = tmp("diamond.fitd");
    let (svg, tex) = (tmp("diamond.svg"), tmp("diamond.tex"));
    std::fs::write(
        &src,
        "layout layered\nnode a \"Alpha\"\nnode d \"Decide?\" shape=diamond\nnode b \"B\"\nnode c \"C\"\na -> d\nd -> b, c\n",
    )
    .unwrap();
    let (ok, log) = fitd(&[
        src.to_str().unwrap(),
        "-o",
        svg.to_str().unwrap(),
        "--tikz",
        tex.to_str().unwrap(),
    ]);
    assert!(ok && log.contains(" 0 errors"), "{log}");
    let s = std::fs::read_to_string(&svg).unwrap();
    let pts = |l: &str, key: &str| -> Vec<(f64, f64)> {
        let i = l.find(key).unwrap() + key.len();
        l[i..i + l[i..].find('"').unwrap()]
            .split(' ')
            .map(|p| {
                let (x, y) = p.split_once(',').unwrap();
                (x.parse().unwrap(), y.parse().unwrap())
            })
            .collect()
    };
    let shape = s
        .lines()
        .find(|l| l.starts_with("<polygon") && l.contains("stroke="))
        .expect("diamond outline");
    let q = pts(shape, "points=\"");
    let (cx, cy, rx, ry) = (q[0].0, q[1].1, q[1].0 - q[0].0, q[1].1 - q[0].1);
    let ends: Vec<f64> = s
        .lines()
        .filter(|l| l.starts_with("<polyline"))
        .flat_map(|l| {
            let p = pts(l, "points=\"");
            [p[0], p[p.len() - 1]]
        })
        .map(|(x, y)| (x - cx).abs() / rx + (y - cy).abs() / ry)
        .filter(|&r| r < 1.5)
        .collect();
    assert_eq!(ends.len(), 3, "{s}");
    assert!(ends.iter().all(|r| (r - 1.0).abs() < 0.02), "{ends:?}");
    let t = std::fs::read_to_string(&tex).unwrap();
    assert!(t.contains("-- cycle;"), "{t}");
}

#[test]
fn edge_heads_and_dashes_reach_svg_and_tikz() {
    let src = tmp("heads.fitd");
    let (svg, tex) = (tmp("heads.svg"), tmp("heads.tex"));
    std::fs::write(
        &src,
        "layout layered\nnode a\nnode b\nnode c\na -> b head=triangle tail=odiamond\nb -- c head=crow line=dotted\n",
    )
    .unwrap();
    let (ok, log) = fitd(&[
        src.to_str().unwrap(),
        "-o",
        svg.to_str().unwrap(),
        "--tikz",
        tex.to_str().unwrap(),
    ]);
    assert!(ok && log.contains(" 0 errors"), "{log}");
    let s = std::fs::read_to_string(&svg).unwrap();
    let lines: Vec<&str> = s.lines().filter(|l| l.starts_with("<polyline")).collect();
    assert_eq!(lines.len(), 2);
    assert!(
        lines[0].contains("marker-start=\"url(#mo") && lines[0].contains("marker-end=\"url(#mt")
    );
    assert!(
        lines[1].contains("marker-end=\"url(#mc") && lines[1].contains("stroke-linecap=\"round\"")
    );
    assert!(!lines[1].contains("marker-start"));
    let t = std::fs::read_to_string(&tex).unwrap();
    assert!(t.contains("{Diamond[open]}-{Triangle[open]}"), "{t}");
    assert!(t.contains("-{Straight Barb[reversed]}, dotted"), "{t}");
}

/// text_at finds where a text element reading `word` is drawn.
fn text_at(svg: &str, word: &str) -> (f64, f64) {
    let end = svg
        .find(&format!(">{word}</"))
        .unwrap_or_else(|| panic!("no text {word}"));
    let start = svg[..end].rfind("<text ").unwrap();
    let tag = &svg[start..end];
    let f = |k: &str| -> f64 {
        let i = tag.find(&format!(" {k}=\"")).unwrap() + k.len() + 3;
        tag[i..i + tag[i..].find('"').unwrap()].parse().unwrap()
    };
    (f("x"), f("y"))
}

fn build_svg(name: &str, src: &str) -> String {
    let path = tmp(&format!("{name}.fitd"));
    std::fs::write(&path, src).unwrap();
    let svg = tmp(&format!("{name}.svg"));
    let (ok, log) = fitd(&[
        path.to_str().unwrap(),
        "-o",
        svg.to_str().unwrap(),
        "--short",
    ]);
    assert!(ok, "{log}");
    std::fs::read_to_string(&svg).unwrap()
}

#[test]
fn sequence_messages_keep_source_order() {
    // Steps zig-zag between lanes declared right to left, so only source order sorts them.
    let mut src = String::from("include \"seq\"\nlayout lanes order=source\n");
    for (id, name) in [("c", "Cc"), ("b", "Bb"), ("a", "Aa")] {
        src += &format!("seq.actor(\"{id}\", \"{name}\")\n");
    }
    let steps = [("a", "c"), ("b", "a"), ("c", "b"), ("a", "b"), ("c", "a")];
    for (i, (from, to)) in steps.iter().enumerate() {
        src += &format!("seq.msg(\"m{i}\", \"{from}\", \"{to}\", \"msg{i}\")\n");
    }
    let svg = build_svg("seq-order", &src);
    let ys: Vec<f64> = (0..steps.len())
        .map(|i| text_at(&svg, &format!("msg{i}")).1)
        .collect();
    assert!(
        ys.windows(2).all(|w| w[0] < w[1]),
        "messages out of order: {ys:?}"
    );
    let heads: Vec<f64> = ["Cc", "Bb", "Aa"]
        .iter()
        .map(|n| text_at(&svg, n).0)
        .collect();
    assert!(
        heads.windows(2).all(|w| w[0] < w[1]),
        "lanes out of order: {heads:?}"
    );
    assert!(text_at(&svg, "Cc").1 < ys[0], "heads below a message");
}

#[test]
fn swimlanes_keep_declaration_order() {
    // The flow runs x, y, z but the lanes are declared z, y, x.
    let svg = build_svg(
        "swim-order",
        "layout lanes\nnode x \"Xx\"\nnode y \"Yy\"\nnode z \"Zz\"\nx -> y -> z\n\
         group \"Lz\" z\ngroup \"Ly\" y\ngroup \"Lx\" x\n",
    );
    let lanes: Vec<f64> = ["Lz", "Ly", "Lx"]
        .iter()
        .map(|n| text_at(&svg, n).0)
        .collect();
    assert!(
        lanes.windows(2).all(|w| w[0] < w[1]),
        "lanes out of order: {lanes:?}"
    );
    let nodes: Vec<(f64, f64)> = ["Zz", "Yy", "Xx"]
        .iter()
        .map(|n| text_at(&svg, n))
        .collect();
    assert!(
        nodes.windows(2).all(|w| w[0].0 < w[1].0),
        "nodes left their lanes: {nodes:?}"
    );
    assert!(
        nodes.windows(2).all(|w| w[0].1 > w[1].1),
        "nodes out of flow order: {nodes:?}"
    );
    // Every lane frame spans the same height.
    let frames: Vec<(f64, f64)> = outlines(&svg).iter().map(|o| (o.1, o.3)).collect();
    let tallest = frames.iter().map(|f| f.1).fold(0.0, f64::max);
    assert_eq!(
        frames.iter().filter(|f| f.1 == tallest).count(),
        3,
        "{frames:?}"
    );
}

#[test]
fn lane_rejects_itself_and_nested_heads() {
    for (body, want) in [
        ("node a \"A\" lane=a\n", "cannot be its own lane"),
        (
            "node h \"H\"\nnode a \"A\" lane=h\nnode b \"B\" lane=a\n",
            "is itself on a lane",
        ),
    ] {
        let src = tmp("lane-err.fitd");
        std::fs::write(&src, format!("layout lanes\n{body}")).unwrap();
        let (ok, log) = fitd(&[
            src.to_str().unwrap(),
            "-o",
            tmp("lane-err.svg").to_str().unwrap(),
        ]);
        assert!(!ok && log.contains("E0007") && log.contains(want), "{log}");
    }
}

#[test]
fn venn_items_land_in_their_regions() {
    let svg = build_svg(
        "venn-regions",
        "layout venn\npage 1:1\nnode a \"Qa\"\nnode b \"Qb\"\nnode c \"Qc\"\nnode ab \"Qab\"\n\
         node ac \"Qac\"\nnode bc \"Qbc\"\nnode abc \"Qabc\"\nnode none \"Qnone\"\n\
         group \"A\" a, ab, ac, abc\ngroup \"B\" b, ab, bc, abc\ngroup \"C\" c, ac, bc, abc\n",
    );
    let circles: Vec<(f64, f64, f64)> = svg
        .split("<circle ")
        .skip(1)
        .map(|r| {
            let tag = &r[..r.find('>').unwrap()];
            let f = |k: &str| -> f64 {
                let i = tag.find(&format!("{k}=\"")).unwrap() + k.len() + 2;
                tag[i..i + tag[i..].find('"').unwrap()].parse().unwrap()
            };
            (f("cx"), f("cy"), f("r"))
        })
        .collect();
    assert_eq!(circles.len(), 3, "{svg}");
    for (word, sets) in [
        ("Qa", "a"),
        ("Qb", "b"),
        ("Qc", "c"),
        ("Qab", "ab"),
        ("Qac", "ac"),
        ("Qbc", "bc"),
        ("Qabc", "abc"),
        ("Qnone", ""),
    ] {
        let (x, y) = text_at(&svg, word);
        let inside: String = circles
            .iter()
            .zip(["a", "b", "c"])
            .filter(|((cx, cy, r), _)| (x - cx).hypot(y - cy) < *r)
            .map(|(_, s)| s)
            .collect();
        assert_eq!(inside, sets, "{word} at ({x}, {y})");
    }
}

#[test]
fn venn_rejects_a_fourth_set() {
    let src = tmp("venn-four.fitd");
    std::fs::write(
        &src,
        "layout venn\nnode a\nnode b\nnode c\nnode d\ngroup a\ngroup b\ngroup c\ngroup d\n",
    )
    .unwrap();
    let (ok, log) = fitd(&[
        src.to_str().unwrap(),
        "-o",
        tmp("venn-four.svg").to_str().unwrap(),
    ]);
    assert!(
        !ok && log.contains("E0010") && log.contains("at most three"),
        "{log}"
    );
}

#[test]
fn algorithm_lines_keep_indent_and_bold_keywords() {
    let src = "layout layered\ninclude \"algo\"\n\
               algo.block(\"a\", \"Algo\", \"**Qkw** Qa;   Qb;     Qc; Qd\")\n";
    let svg = build_svg("algo-indent", src);
    let x = |w: &str| text_at(&svg, w).0;
    let step = x("Qb") - x("Qkw");
    assert!(step > 1.0, "{svg}");
    assert!((x("Qc") - x("Qkw") - 2.0 * step).abs() < 0.05, "{svg}");
    assert!((x("Qd") - x("Qkw")).abs() < 0.05, "{svg}");
    assert!(!svg.contains("**"), "{svg}");
    let kw = &svg[..svg.find(">Qkw<").unwrap()];
    assert!(kw[kw.rfind("<tspan").unwrap()..].contains("font-weight=\"bold\""));
    let path = tmp("algo-indent.fitd");
    let tex = tmp("algo-indent.tex");
    let (ok, log) = fitd(&[
        path.to_str().unwrap(),
        "-o",
        tmp("algo-indent2.svg").to_str().unwrap(),
        "--tikz",
        tex.to_str().unwrap(),
    ]);
    assert!(ok, "{log}");
    assert!(
        std::fs::read_to_string(&tex)
            .unwrap()
            .contains("{\\bfseries Qkw}")
    );
}
