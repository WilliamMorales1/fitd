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
    let (ok, log) = cmapc(&["build", "examples/math4200.cmap", "-o", svg.to_str().unwrap(), "--tikz", tex.to_str().unwrap(), "--page", "2296x1524"]);
    assert!(ok, "{log}");
    assert!(log.contains(" 0 errors"), "{log}");
    let body: f64 = log.split("body font ").nth(1).unwrap().split("pt").next().unwrap().parse().unwrap();
    assert!(body >= 14.0, "body font regressed to {body}pt\n{log}");
    let s = std::fs::read_to_string(&svg).unwrap();
    assert!(s.starts_with("<svg") && s.contains("Groups"));
    assert!(std::fs::read_to_string(&tex).unwrap().contains("\\begin{tikzpicture}"));
}

#[test]
fn general_graph_with_cycle_and_no_root() {
    let src = tmp("cycle.cmap");
    std::fs::write(&src, "node a \"Alpha\"\nnode b \"Beta\"\n| detail\nnode c \"Gamma\"\nnode d \"Delta\"\na -- b\nb -- c\nc -- a\nc -- d\n").unwrap();
    let (ok, log) = cmapc(&["build", src.to_str().unwrap(), "-o", tmp("cycle.svg").to_str().unwrap(), "--page", "a4"]);
    assert!(ok, "{log}");
    assert!(log.contains("chose graph center"), "{log}");
    assert!(log.contains("closes a cycle"), "{log}");
}

#[test]
fn impossible_page_is_rejected() {
    let (ok, log) = cmapc(&["build", "examples/math4200.cmap", "-o", tmp("x.svg").to_str().unwrap(), "--page", "200x150"]);
    assert!(!ok);
    assert!(log.contains("no layout fits"), "{log}");
}

#[test]
fn parse_errors_point_at_line() {
    let src = tmp("bad.cmap");
    std::fs::write(&src, "node a\na -> ghost\n").unwrap();
    let (ok, log) = cmapc(&["build", src.to_str().unwrap()]);
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
    let (ok, log) = cmapc(&["build", "examples/compiler.cmap", "-o", svg.to_str().unwrap()]);
    assert!(ok, "{log}");
    assert!(log.contains(" 0 errors") && !log.contains("E0108"), "{log}");
    assert!(std::fs::read_to_string(&svg).unwrap().contains("marker-end"));
}

#[test]
fn unsat_constraints_report_minimal_core() {
    let src = tmp("core.cmap");
    let text = "layout layered\npage 420x300pt\nnode a\nnode b\nnode c\nnode d\na -> b, c\nc -> d\n\
                require b at left\nrequire align column(c, d)\nrequire c at left\nrequire d at bottom-left\nrequire b left-of c gap=20em\n";
    std::fs::write(&src, text).unwrap();
    let (ok, log) = cmapc(&["build", src.to_str().unwrap(), "-o", tmp("core.svg").to_str().unwrap()]);
    assert!(!ok);
    assert!(log.contains("error[E0110]: these 2 constraints"), "{log}");
    assert!(log.contains("11 | require c at left") && log.contains("13 | require b left-of c gap=20em"), "{log}");
    assert!(log.contains("7pt is the default font floor"), "{log}");
}

#[test]
fn contradictory_orderings_fail_before_search() {
    let src = tmp("cycle-order.cmap");
    std::fs::write(&src, "node a\nnode b\nnode c\nrequire a left-of b\nrequire b left-of c\nrequire c left-of a\n").unwrap();
    let (ok, log) = cmapc(&["build", src.to_str().unwrap()]);
    assert!(!ok);
    assert!(log.contains("error[E0008]: contradictory horizontal constraints"), "{log}");
    assert!(!log.contains("body font"), "search should not run\n{log}");
}

#[test]
fn png_is_cropped_to_drawing() {
    let png = tmp("compiler.png");
    let (ok, log) = cmapc(&["build", "examples/compiler.cmap", "-o", png.to_str().unwrap(), "--page", "a3", "--scale", "1"]);
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
    let (ok, log) = cmapc(&["build", src.to_str().unwrap(), "-o", svg.to_str().unwrap(), "--tikz", tex.to_str().unwrap()]);
    assert!(ok, "{log}");
    let s = std::fs::read_to_string(&svg).unwrap();
    for c in ["#102030", "#4DD0E1", "#FFE0B2", "#3E2723", "#E6E6E6"] {
        assert!(s.contains(c), "{c} missing from svg");
    }
    assert!(std::fs::read_to_string(&tex).unwrap().contains("\\definecolor{c102030}"));
    let (ok, log) = cmapc(&["build", src.to_str().unwrap(), "-o", svg.to_str().unwrap(), "--theme", "neon"]);
    assert!(!ok && log.contains("unknown theme `neon`"), "{log}");
}

/// Every example with the most warnings and seconds it may take on the reference laptop
/// (ThinkPad X280, release build). Lower a number when a change improves on it.
const EXAMPLES: [(&str, usize, f64); 5] = [
    ("cell_biology", 0, 5.0),
    ("compiler", 0, 6.0),
    ("earth_science", 8, 12.0),
    ("math4200", 3, 15.0),
    ("math4200-v2", 1, 25.0),
];

#[test]
fn every_example_builds_within_baseline() {
    for (name, warnings, secs) in EXAMPLES {
        let src = format!("examples/{name}.cmap");
        let outs = [tmp(&format!("{name}-a.svg")), tmp(&format!("{name}-b.svg"))];
        // A second build runs alongside to catch HashMap order or thread timing leaking into
        // the layout; it shares the CPU, so the time budget allows for it.
        let twin = Command::new(env!("CARGO_BIN_EXE_cmapc")).args(["build", &src, "-o", outs[1].to_str().unwrap()]).spawn().unwrap();
        let t = std::time::Instant::now();
        let (ok, log) = cmapc(&["build", &src, "-o", outs[0].to_str().unwrap()]);
        let took = t.elapsed().as_secs_f64();
        assert!(twin.wait_with_output().unwrap().status.success(), "{name}: second build failed");
        assert!(ok && log.contains(" 0 errors"), "{name}\n{log}");
        let got: usize = log.split(" errors, ").nth(1).unwrap().split(' ').next().unwrap().parse().unwrap();
        assert!(got <= warnings, "{name}: {got} warnings, baseline {warnings}\n{log}");
        assert!(took <= secs, "{name}: took {took:.1}s, budget {secs}s");
        assert!(std::fs::read(&outs[0]).unwrap() == std::fs::read(&outs[1]).unwrap(), "{name}: two builds differ");
    }
}

#[test]
fn fixtures_fail_or_build_as_designed() {
    let (ok, log) = cmapc(&["build", "tests/fixtures/stress_test.cmap", "-o", tmp("stress.svg").to_str().unwrap()]);
    assert!(!ok && log.contains("error[E0008]"), "{log}");
    let (ok, log) = cmapc(&["build", "tests/fixtures/complex_constraints.cmap", "-o", tmp("complex.svg").to_str().unwrap()]);
    assert!(ok && log.contains(" 0 errors"), "{log}");
}

#[test]
fn row_alignment_merges_layers_in_flowchart() {
    let src = tmp("row.cmap");
    std::fs::write(&src, "layout layered\npage a4\nnode a\nnode b\nnode c\nnode d\na -> b\nb -> c\nc -> d\nrequire align row(b, c, d)\nrequire b left-of c left-of d\n").unwrap();
    let (ok, log) = cmapc(&["build", src.to_str().unwrap(), "-o", tmp("row.svg").to_str().unwrap()]);
    assert!(ok, "{log}");
    assert!(log.contains(" 0 errors"), "{log}");
}
