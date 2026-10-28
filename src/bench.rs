//! `cmapc bench`: every search algorithm on classic CSPs and on real diagrams.

use crate::csp::{self, Algo, Outcome, Problem};
use std::time::Instant;

/// Queens places one queen per row; the value is the column.
pub struct Queens {
    pub n: usize,
    scopes: Vec<Vec<usize>>,
}

impl Queens {
    pub fn new(n: usize) -> Queens {
        let scopes = (0..n).flat_map(|i| (i + 1..n).map(move |j| vec![i, j])).collect();
        Queens { n, scopes }
    }
}

impl Problem for Queens {
    fn num_vars(&self) -> usize {
        self.n
    }
    fn domain_size(&self, _: usize) -> usize {
        self.n
    }
    fn scopes(&self) -> &[Vec<usize>] {
        &self.scopes
    }
    fn holds(&self, c: usize, val: &[u32]) -> bool {
        let (i, j) = (self.scopes[c][0], self.scopes[c][1]);
        let (a, b) = (val[i] as i64, val[j] as i64);
        a != b && (a - b).abs() != (j as i64 - i as i64)
    }
}

/// Pigeons puts n+1 pigeons in n holes: unsatisfiable, and exponential for every algorithm here.
pub struct Pigeons {
    n: usize,
    scopes: Vec<Vec<usize>>,
}

impl Pigeons {
    pub fn new(n: usize) -> Pigeons {
        Pigeons { n, scopes: (0..=n).flat_map(|i| (i + 1..=n).map(move |j| vec![i, j])).collect() }
    }
}

impl Problem for Pigeons {
    fn num_vars(&self) -> usize {
        self.n + 1
    }
    fn domain_size(&self, _: usize) -> usize {
        self.n
    }
    fn scopes(&self) -> &[Vec<usize>] {
        &self.scopes
    }
    fn holds(&self, c: usize, val: &[u32]) -> bool {
        val[self.scopes[c][0]] != val[self.scopes[c][1]]
    }
}

/// Coloring is graph coloring of a seeded random graph.
pub struct Coloring {
    n: usize,
    k: usize,
    scopes: Vec<Vec<usize>>,
}

impl Coloring {
    pub fn random(n: usize, edges: usize, k: usize, seed: u64) -> Coloring {
        let mut rng = csp::Rng::new(seed);
        let mut scopes: Vec<Vec<usize>> = Vec::new();
        while scopes.len() < edges {
            let (a, b) = (rng.below(n), rng.below(n));
            let e = vec![a.min(b), a.max(b)];
            if a != b && !scopes.contains(&e) {
                scopes.push(e);
            }
        }
        Coloring { n, k, scopes }
    }
}

impl Problem for Coloring {
    fn num_vars(&self) -> usize {
        self.n
    }
    fn domain_size(&self, _: usize) -> usize {
        self.k
    }
    fn scopes(&self) -> &[Vec<usize>] {
        &self.scopes
    }
    fn holds(&self, c: usize, val: &[u32]) -> bool {
        val[self.scopes[c][0]] != val[self.scopes[c][1]]
    }
}

pub const ALGOS: &[&str] = &["bt", "cbj", "fc", "fc-cbj", "mac", "fc+dom", "fc+wdeg", "fc-cbj+wdeg", "mac+wdeg", "cbj+learn", "fc-cbj+learn", "fc-cbj+restart", "fc-cbj+wdeg+restart"];

pub struct Row {
    pub problem: String,
    pub algo: String,
    pub outcome: &'static str,
    pub stats: csp::Stats,
    pub ms: f64,
}

pub fn outcome_name(o: &Outcome) -> &'static str {
    match o {
        Outcome::Sat(_) => "sat",
        Outcome::Unsat => "unsat",
        Outcome::Unknown => "budget",
    }
}

/// classic runs every algorithm on the textbook problems.
pub fn classic(budget: u64) -> Vec<Row> {
    let problems: Vec<(String, Box<dyn Fn(Algo) -> (Outcome, csp::Stats)>)> = vec![
        ("queens-12".into(), Box::new(move |a| csp::solve(&Queens::new(12), a, budget))),
        ("queens-28".into(), Box::new(move |a| csp::solve(&Queens::new(28), a, budget))),
        ("pigeons-8".into(), Box::new(move |a| csp::solve(&Pigeons::new(8), a, budget))),
        // Edge density near the 3-coloring phase transition (average degree about 4.6).
        ("color-60x3".into(), Box::new(move |a| csp::solve(&Coloring::random(60, 138, 3, 11), a, budget))),
    ];
    let mut rows = Vec::new();
    for (name, run) in &problems {
        for &a in ALGOS {
            let t = Instant::now();
            let (out, stats) = run(Algo::parse(a).unwrap());
            rows.push(Row { problem: name.clone(), algo: a.into(), outcome: outcome_name(&out), stats, ms: t.elapsed().as_secs_f64() * 1e3 });
        }
        let t = Instant::now();
        if name.starts_with("queens") {
            let n: usize = name[7..].parse().unwrap();
            let (out, stats) = csp::min_conflicts(&Queens::new(n), budget / 1000, 1);
            rows.push(Row { problem: name.clone(), algo: "min-conflicts".into(), outcome: outcome_name(&out), stats, ms: t.elapsed().as_secs_f64() * 1e3 });
        }
    }
    rows
}

pub fn table(rows: &[Row]) -> String {
    let mut s = String::from("| problem | algorithm | result | nodes | checks | ms |\n|---|---|---|---:|---:|---:|\n");
    for r in rows {
        s += &format!("| {} | {} | {} | {} | {} | {:.1} |\n", r.problem, r.algo, r.outcome, r.stats.nodes, r.stats.checks, r.ms);
    }
    s
}
