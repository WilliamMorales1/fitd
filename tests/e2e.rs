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
    std::fs::write(&src, "node a \"Alpha\"\nnode b \"Beta\"\n| detail\nnode c \"Gamma\"\nnode d \"Delta\"\nedge a -- b\nedge b -- c\nedge c -- a\nedge c -- d\n").unwrap();
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
    std::fs::write(&src, "node a\nedge a -> ghost\n").unwrap();
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
fn bench_runs_every_algorithm_on_a_diagram() {
    let out = Command::new(env!("CARGO_BIN_EXE_cmapc")).args(["bench", "examples/compiler.cmap", "--font", "8"]).output().unwrap();
    let table = String::from_utf8_lossy(&out.stdout);
    assert_eq!(table.matches("| sat |").count(), table.lines().count() - 2, "{table}");
}
