//! Randomized robustness test of the front end: mutated sources must never panic the lexer,
//! parser, semantic analysis, tree building, or diagnostic rendering.
//!
//! `cargo test --release fuzz` runs it for 2 seconds; set `FITD_FUZZ_SECS` to run longer.
//! A failure prints the seed and saves the input, so it can be replayed.

use crate::{diag, parse, sema, tree};
use std::panic::{AssertUnwindSafe, catch_unwind};

const SEEDS: [&str; 10] = [
    include_str!("../tests/fixtures/cell_biology.fitd"),
    include_str!("../examples/compiler.fitd"),
    include_str!("../tests/fixtures/earth_science.fitd"),
    include_str!("../tests/fixtures/french_history.fitd"),
    include_str!("../examples/group_theory.fitd"),
    include_str!("../examples/fib_calls.fitd"),
    include_str!("../examples/semester.fitd"),
    include_str!("../tests/fixtures/collatz.fitd"),
    include_str!("../tests/fixtures/stress_test.fitd"),
    include_str!("../tests/fixtures/complex_constraints.fitd"),
];

/// Tokens and fragments worth splicing in: keywords, operators, units, and odd values.
const WORDS: &[&str] = &[
    "node", "title", "root", "layout", "radial", "layered", "dir=", "page", "portrait", "margin=", "font", "wrap=", "family=", "theme", "dark",
    "palette=", "style", "group", "require", "prefer", "align", "row", "column", "top", "left-of", "right-of", "above", "below", "near", "at", "top-left",
    "center", "gap=", "weight=", "within=", "shape=", "circle", "color=", "fill=", "->", "--", "..>", "..", "|", "\"", "(", ")", ",", ".", "=", "#",
    "#FFF", "#12345G", "\n", " ", "\t", "\r\n", "2em", "12pt", "0pt", "-3pt", "1e9pt", "99999999999999999999pt", "594x420mm", "0x0pt", "a3", "a9",
    "x", "é", "\u{0}", "\u{200b}", "^{", "_{", "}", "{", "..40pt", "7pt..", "a", "b", "a b", "weight=0", "weight=-1", "within=0em",
    "for i in 1..3\n", "for s in a, b\n", "end\n", "if i > 1\n", "else\n", "else if i\n", "def t(a)\n", "use t(1)\n", "use t(\"x\")\n",
    "${", "${i}", "${i + 1}", "$${", "${names(\"s\", 1, 3)}", "${1 / 0}",
];

fn front_end(text: &str) {
    let crate::macros::Loaded { src, parse, mut diags } = crate::macros::expand_text("fuzz.fitd", text, 2_000);
    let (stmts, more) = parse::parse(&parse);
    diags.extend(more);
    let (g, more) = sema::analyze(&stmts);
    diags.extend(more);
    diags.sort_by_key(diag::Diag::pos);
    let _ = diag::render(&src, &diags);
    if !diags.iter().any(|d| d.level == diag::Level::Error) {
        let t = tree::build(&g);
        let _ = diag::render(&src, &t.notes);
    }
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, k: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % k.max(1) as u64) as usize
    }
}

/// mutate applies a few random edits to a seed source, keeping it valid UTF-8.
fn mutate(seed: u64) -> String {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut s: Vec<char> = SEEDS[r.below(SEEDS.len())].chars().collect();
    for _ in 0..1 + r.below(8) {
        let at = r.below(s.len() + 1);
        match r.below(6) {
            0 => {
                let end = (at + r.below(40)).min(s.len());
                s.drain(at..end);
            }
            1 => {
                let w: Vec<char> = WORDS[r.below(WORDS.len())].chars().collect();
                s.splice(at..at, w);
            }
            2 if !s.is_empty() => {
                // Copy a random slice elsewhere: duplicate declarations and constraints.
                let from = r.below(s.len());
                let piece: Vec<char> = s[from..(from + r.below(80)).min(s.len())].to_vec();
                s.splice(at..at, piece);
            }
            3 if at < s.len() => s[at] = char::from_u32(r.below(0x250) as u32).unwrap_or('?'),
            4 => s.truncate(at),
            _ => {
                let w: Vec<char> = format!(" {} ", WORDS[r.below(WORDS.len())]).chars().collect();
                s.splice(at..at, w);
            }
        }
    }
    s.into_iter().collect()
}

#[test]
fn fuzz_front_end() {
    let secs: u64 = std::env::var("FITD_FUZZ_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(2);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    // Half the cores, so a long run leaves the machine usable.
    let threads = (std::thread::available_parallelism().map_or(2, |n| n.get()) / 2).max(1) as u64;
    let runs: u64 = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|k| {
                s.spawn(move || {
                    let mut seed = k;
                    while std::time::Instant::now() < deadline {
                        let text = mutate(seed);
                        if catch_unwind(AssertUnwindSafe(|| front_end(&text))).is_err() {
                            let path = std::env::temp_dir().join(format!("fitd-fuzz-{seed}.fitd"));
                            std::fs::write(&path, &text).unwrap();
                            panic!("front end panicked on seed {seed}; input saved to {}", path.display());
                        }
                        seed += threads;
                    }
                    seed / threads
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).sum()
    });
    assert!(runs > 100, "only {runs} inputs in {secs}s");
    eprintln!("fuzz: {runs} inputs in {secs}s, no panics");
}
