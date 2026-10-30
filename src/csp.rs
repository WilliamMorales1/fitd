//! Finite-domain CSP solver: forward checking with conflict-directed backjumping (FC-CBJ,
//! Prosser 1993), plus branch and bound over soft constraints.
//!
//! A problem has variables `0..n`, each with domain `0..k` in preference order, and
//! constraints of any arity given as scopes plus a predicate. Variables are assigned in index
//! order, so the problem decides the order. Forward checking prunes a variable as soon as it
//! is the last unassigned one in some scope.

use std::sync::atomic::{AtomicBool, Ordering};

pub const UNSET: u32 = u32::MAX;

pub trait Problem {
    fn num_vars(&self) -> usize;
    fn domain_size(&self, v: usize) -> usize;
    fn scopes(&self) -> &[Vec<usize>];
    /// holds evaluates constraint `c`; every variable in its scope is assigned in `val`.
    fn holds(&self, c: usize, val: &[u32]) -> bool;
    /// weight is 0 for a hard constraint, or the cost of violating a soft one.
    fn weight(&self, _c: usize) -> u64 {
        0
    }
    /// filter is an optional fast path for forward checking. Constraint `c` has `y` as its
    /// only unassigned variable; call `bad` with every alive value of `y` that violates `c`.
    /// Return the number of elementary tests done, or None to fall back to one check per value.
    fn filter(&self, _c: usize, _val: &[u32], _y: usize, _alive: &dyn Fn(usize) -> bool, _bad: &mut dyn FnMut(usize)) -> Option<u64> {
        None
    }
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Stats {
    /// Assignments tried.
    pub nodes: u64,
    /// Constraint evaluations.
    pub checks: u64,
    pub backjumps: u64,
}

impl std::ops::AddAssign for Stats {
    fn add_assign(&mut self, o: Stats) {
        self.nodes += o.nodes;
        self.checks += o.checks;
        self.backjumps += o.backjumps;
    }
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Sat(Vec<u32>),
    Unsat,
    /// The check budget ran out before search finished.
    Unknown,
}

#[derive(Clone)]
struct VarSet(Vec<u64>);

impl VarSet {
    fn new(n: usize) -> VarSet {
        VarSet(vec![0; n.div_ceil(64)])
    }
    fn insert(&mut self, v: usize) {
        self.0[v / 64] |= 1 << (v % 64);
    }
    fn remove(&mut self, v: usize) {
        self.0[v / 64] &= !(1 << (v % 64));
    }
    fn contains(&self, v: usize) -> bool {
        self.0[v / 64] >> (v % 64) & 1 == 1
    }
    fn union(&mut self, o: &VarSet) {
        for (a, b) in self.0.iter_mut().zip(&o.0) {
            *a |= b;
        }
    }
}

enum Fail {
    /// The variables whose current values caused the failure; search backs up to the deepest.
    Conflict(VarSet),
    Abort,
}

struct Search<'a, P: Problem> {
    p: &'a P,
    n: usize,
    /// Hard and soft constraints per variable.
    hard: Vec<Vec<usize>>,
    soft: Vec<Vec<usize>>,
    val: Vec<u32>,
    /// removed[v][a] is the depth that pruned value a, or UNSET while it is alive.
    removed: Vec<Vec<u32>>,
    size: Vec<usize>,
    trail: Vec<(usize, u32)>,
    /// Per variable: (depth, culprits) for each prune, so a wipeout knows whom to blame.
    pruned_by: Vec<Vec<(u32, VarSet)>>,
    budget: u64,
    /// Set by another thread when this search's answer is no longer needed.
    stop: &'a AtomicBool,
    stats: Stats,
    optimize: bool,
    cost: u64,
    best: Option<(u64, Vec<u32>)>,
}

impl<P: Problem> Search<'_, P> {
    fn unassigned_in(&self, c: usize) -> usize {
        self.p.scopes()[c].iter().filter(|&&v| self.val[v] == UNSET).count()
    }

    fn spend(&mut self, n: u64) -> Result<(), Fail> {
        self.stats.checks += n;
        if self.stats.checks > self.budget || self.stop.load(Ordering::Relaxed) { Err(Fail::Abort) } else { Ok(()) }
    }

    fn culprits(&self, c: usize) -> VarSet {
        let mut s = VarSet::new(self.n);
        self.p.scopes()[c].iter().filter(|&&v| self.val[v] != UNSET).for_each(|&v| s.insert(v));
        s
    }

    fn everyone(&self) -> VarSet {
        let mut s = VarSet::new(self.n);
        (0..self.n).filter(|&v| self.val[v] != UNSET).for_each(|v| s.insert(v));
        s
    }

    fn past_fc(&self, y: usize) -> VarSet {
        let mut s = VarSet::new(self.n);
        for (_, why) in &self.pruned_by[y] {
            s.union(why);
        }
        s
    }

    /// revise removes the values of `y` that violate `c`, whose other variables are assigned.
    fn revise(&mut self, c: usize, y: usize, depth: u32) -> Result<(), Fail> {
        let mut bad = Vec::new();
        let removed = &self.removed[y];
        match self.p.filter(c, &self.val, y, &|a| removed[a] == UNSET, &mut |a| bad.push(a)) {
            Some(tests) => self.spend(tests)?,
            None => {
                for a in 0..self.p.domain_size(y) {
                    if self.removed[y][a] == UNSET {
                        self.spend(1)?;
                        self.val[y] = a as u32;
                        if !self.p.holds(c, &self.val) {
                            bad.push(a);
                        }
                    }
                }
                self.val[y] = UNSET;
            }
        }
        let mut changed = false;
        for a in bad {
            if self.removed[y][a] == UNSET {
                self.removed[y][a] = depth;
                self.size[y] -= 1;
                self.trail.push((y, a as u32));
                changed = true;
            }
        }
        if changed {
            let why = self.culprits(c);
            self.pruned_by[y].push((depth, why));
        }
        Ok(())
    }

    /// forward_check returns the conflict set of the first wiped-out domain, if any.
    fn forward_check(&mut self, x: usize, depth: u32) -> Result<Option<VarSet>, Fail> {
        for i in 0..self.hard[x].len() {
            let c = self.hard[x][i];
            if self.unassigned_in(c) != 1 {
                continue;
            }
            let y = *self.p.scopes()[c].iter().find(|&&v| self.val[v] == UNSET).unwrap();
            self.revise(c, y, depth)?;
            if self.size[y] == 0 {
                return Ok(Some(self.past_fc(y)));
            }
        }
        Ok(None)
    }

    /// soft_delta is the weight of soft constraints that `x` just completed and violates.
    fn soft_delta(&mut self, x: usize) -> Result<u64, Fail> {
        let mut d = 0;
        for i in 0..self.soft[x].len() {
            let c = self.soft[x][i];
            if self.unassigned_in(c) == 0 {
                self.spend(1)?;
                if !self.p.holds(c, &self.val) {
                    d += self.p.weight(c);
                }
            }
        }
        Ok(d)
    }

    fn undo(&mut self, depth: u32) {
        while let Some(&(y, a)) = self.trail.last() {
            if self.removed[y][a as usize] != depth {
                break;
            }
            self.removed[y][a as usize] = UNSET;
            self.size[y] += 1;
            self.trail.pop();
        }
        for p in &mut self.pruned_by {
            while p.last().is_some_and(|(d, _)| *d == depth) {
                p.pop();
            }
        }
    }

    fn label(&mut self, x: usize) -> Result<(), Fail> {
        if x == self.n {
            if !self.optimize {
                return Ok(());
            }
            self.best = Some((self.cost, self.val.clone()));
            // Keep searching for a cheaper solution unless this one is perfect.
            return if self.cost == 0 { Ok(()) } else { Err(Fail::Conflict(self.everyone())) };
        }
        let depth = x as u32 + 1;
        let mut conf = self.past_fc(x);
        for a in 0..self.p.domain_size(x) {
            if self.removed[x][a] != UNSET {
                continue;
            }
            self.stats.nodes += 1;
            self.val[x] = a as u32;
            let mut ok = true;
            if let Some(wipe) = self.forward_check(x, depth)? {
                conf.union(&wipe);
                ok = false;
            }
            let mut delta = 0;
            if ok && self.optimize {
                delta = self.soft_delta(x)?;
                if self.best.as_ref().is_some_and(|b| self.cost + delta >= b.0) {
                    // Bound failure: blame the whole path, since the bound depends on all of it.
                    conf.union(&self.everyone());
                    ok = false;
                }
            }
            if ok {
                self.cost += delta;
                let res = self.label(x + 1);
                self.cost -= delta;
                match res {
                    Ok(()) => return Ok(()),
                    Err(Fail::Abort) => return Err(Fail::Abort),
                    Err(Fail::Conflict(cs)) if !cs.contains(x) => {
                        self.stats.backjumps += 1;
                        self.undo(depth);
                        self.val[x] = UNSET;
                        return Err(Fail::Conflict(cs));
                    }
                    Err(Fail::Conflict(cs)) => conf.union(&cs),
                }
            }
            self.undo(depth);
            self.val[x] = UNSET;
        }
        conf.remove(x);
        Err(Fail::Conflict(conf))
    }
}

/// solve searches for an assignment satisfying every hard constraint, within `budget` checks
/// or until `stop` is set. Soft constraints are ignored.
pub fn solve<P: Problem>(p: &P, budget: u64, stop: &AtomicBool) -> (Outcome, Stats) {
    let (out, _, stats) = run(p, budget, stop, false);
    (out, stats)
}

/// optimize is branch and bound: it satisfies every hard constraint while minimizing the
/// total weight of violated soft ones. On budget exhaustion it returns the best found so far
/// as `Sat`; the returned cost is that solution's soft cost.
pub fn optimize<P: Problem>(p: &P, budget: u64, stop: &AtomicBool) -> (Outcome, u64, Stats) {
    run(p, budget, stop, true)
}

fn run<P: Problem>(p: &P, budget: u64, stop: &AtomicBool, optimize: bool) -> (Outcome, u64, Stats) {
    let n = p.num_vars();
    let (mut hard, mut soft) = (vec![Vec::new(); n], vec![Vec::new(); n]);
    for (c, s) in p.scopes().iter().enumerate() {
        for &v in s {
            if p.weight(c) == 0 { hard[v].push(c) } else { soft[v].push(c) }
        }
    }
    let mut s = Search {
        p,
        n,
        hard,
        soft,
        val: vec![UNSET; n],
        removed: (0..n).map(|v| vec![UNSET; p.domain_size(v)]).collect(),
        size: (0..n).map(|v| p.domain_size(v)).collect(),
        trail: Vec::new(),
        pruned_by: vec![Vec::new(); n],
        budget,
        stop,
        stats: Stats::default(),
        optimize,
        cost: 0,
        best: None,
    };
    // Unary constraints have nothing to propagate from; filter them once up front.
    for c in 0..p.scopes().len() {
        if p.scopes()[c].len() == 1 && p.weight(c) == 0 && s.revise(c, p.scopes()[c][0], 0).is_err() {
            return (Outcome::Unknown, 0, s.stats);
        }
    }
    if s.size.contains(&0) {
        return (Outcome::Unsat, 0, s.stats);
    }
    let res = s.label(0);
    let stats = s.stats;
    match (res, s.best) {
        (_, Some((cost, val))) => (Outcome::Sat(val), cost, stats),
        (Ok(()), None) => (Outcome::Sat(s.val), 0, stats),
        (Err(Fail::Conflict(_)), None) => (Outcome::Unsat, 0, stats),
        (Err(Fail::Abort), None) => (Outcome::Unknown, 0, stats),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static GO: AtomicBool = AtomicBool::new(false);

    /// Table is a small problem given as scopes, a predicate, and weights.
    struct Table {
        n: usize,
        k: usize,
        scopes: Vec<Vec<usize>>,
        ok: fn(usize, &[usize], &[u32]) -> bool,
        weights: Vec<u64>,
    }

    impl Problem for Table {
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
            (self.ok)(c, &self.scopes[c], val)
        }
        fn weight(&self, c: usize) -> u64 {
            self.weights.get(c).copied().unwrap_or(0)
        }
    }

    fn all_pairs(n: usize) -> Vec<Vec<usize>> {
        (0..n).flat_map(|i| (i + 1..n).map(move |j| vec![i, j])).collect()
    }

    #[test]
    fn solves_queens() {
        let ok = |_, s: &[usize], v: &[u32]| {
            let (a, b) = (v[s[0]] as i64, v[s[1]] as i64);
            a != b && (a - b).unsigned_abs() as usize != s[1] - s[0]
        };
        let q = Table { n: 12, k: 12, scopes: all_pairs(12), ok, weights: vec![] };
        let (Outcome::Sat(v), _) = solve(&q, u64::MAX, &GO) else { panic!("no solution") };
        assert!((0..q.scopes.len()).all(|c| q.holds(c, &v)));
    }

    #[test]
    fn proves_pigeonhole_unsat_and_respects_budget() {
        let p = Table { n: 6, k: 5, scopes: all_pairs(6), ok: |_, s, v| v[s[0]] != v[s[1]], weights: vec![] };
        assert_eq!(solve(&p, u64::MAX, &GO).0, Outcome::Unsat);
        assert_eq!(solve(&p, 100, &GO).0, Outcome::Unknown);
    }

    #[test]
    fn branch_and_bound_minimizes_soft_cost() {
        // Path a-b-c, two colors; soft: a=1 (weight 5), c=0 (weight 1).
        let ok = |c, s: &[usize], v: &[u32]| match c {
            0 | 1 => v[s[0]] != v[s[1]],
            2 => v[0] == 1,
            _ => v[2] == 0,
        };
        let p = Table { n: 3, k: 2, scopes: vec![vec![0, 1], vec![1, 2], vec![0], vec![2]], ok, weights: vec![0, 0, 5, 1] };
        // a=1 forces b=0, c=1: pay 1 for c rather than 5 for a.
        let (out, cost, _) = optimize(&p, u64::MAX, &GO);
        assert_eq!((out, cost), (Outcome::Sat(vec![1, 0, 1]), 1));
        assert_eq!(solve(&p, u64::MAX, &GO).0, Outcome::Sat(vec![0, 1, 0]));
    }
}
