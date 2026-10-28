//! Generic finite-domain CSP solver with pluggable search.
//!
//! A problem is a set of variables with integer domains `0..k` (index order is value
//! preference) and constraints of any arity given as scopes plus a predicate. Search combines
//! a lookahead (none, forward checking, maintained arc consistency), a lookback
//! (chronological or conflict-directed backjumping), and a variable ordering heuristic.
//!
//! Optional nogood learning records each conflict set as a forbidden partial assignment, and
//! optional restarts rerun search with a growing failure cutoff, keeping nogoods and weights.
//!
//! References: Prosser 1993 (CBJ, FC-CBJ), Sabin & Freuder 1994 (MAC), Boussemart et al.
//! 2004 (dom/wdeg), Minton et al. 1992 (min-conflicts), Dechter 1990 (learning while
//! searching), Lecoutre et al. 2007 (nogood recording from restarts).

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
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Lookahead {
    None,
    /// Forward checking: prune values of a variable that is the last unassigned one in a scope.
    Fc,
    /// FC, then arc consistency over binary constraints between unassigned variables.
    Mac,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum VarOrder {
    /// Variable index order.
    Static,
    /// Smallest remaining domain first.
    Dom,
    /// Smallest domain divided by the weighted degree; weights grow on each failure.
    DomWdeg,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Algo {
    pub lookahead: Lookahead,
    pub backjump: bool,
    pub order: VarOrder,
    /// Record conflict sets as nogoods; needs `backjump`, since that computes them.
    pub learn: bool,
    /// Restart with a geometrically growing failure cutoff (implies `learn`).
    pub restarts: bool,
}

impl Algo {
    pub const BT: Algo = Algo { lookahead: Lookahead::None, backjump: false, order: VarOrder::Static, learn: false, restarts: false };
    pub const CBJ: Algo = Algo { backjump: true, ..Algo::BT };
    pub const FC: Algo = Algo { lookahead: Lookahead::Fc, ..Algo::BT };
    pub const FC_CBJ: Algo = Algo { lookahead: Lookahead::Fc, ..Algo::CBJ };
    pub const MAC: Algo = Algo { lookahead: Lookahead::Mac, ..Algo::BT };

    pub fn name(&self) -> String {
        let la = match self.lookahead {
            Lookahead::None if self.backjump => "",
            Lookahead::None => "bt",
            Lookahead::Fc => "fc",
            Lookahead::Mac => "mac",
        };
        let bj = if self.backjump { if la.is_empty() { "cbj" } else { "-cbj" } } else { "" };
        let ord = match self.order {
            VarOrder::Static => "",
            VarOrder::Dom => "+dom",
            VarOrder::DomWdeg => "+wdeg",
        };
        let extra = if self.restarts { "+restart" } else if self.learn { "+learn" } else { "" };
        format!("{la}{bj}{ord}{extra}")
    }

    /// parse reads names like `bt`, `cbj`, `fc-cbj+wdeg`, `mac+dom`, or `fc-cbj+wdeg+restart`.
    pub fn parse(s: &str) -> Option<Algo> {
        let mut parts = s.split('+');
        let mut a = match parts.next()? {
            "bt" => Algo::BT,
            "cbj" => Algo::CBJ,
            "fc" => Algo::FC,
            "fc-cbj" => Algo::FC_CBJ,
            "mac" => Algo::MAC,
            "mac-cbj" => Algo { backjump: true, ..Algo::MAC },
            _ => return None,
        };
        for p in parts {
            match p {
                "dom" => a.order = VarOrder::Dom,
                "wdeg" => a.order = VarOrder::DomWdeg,
                "learn" => a.learn = true,
                "restart" => (a.learn, a.restarts) = (true, true),
                _ => return None,
            }
        }
        // Nogoods are conflict sets; chronological search has none, only the whole path.
        if a.learn && !a.backjump {
            return None;
        }
        Some(a)
    }
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Stats {
    /// Assignments tried.
    pub nodes: u64,
    /// Constraint evaluations.
    pub checks: u64,
    pub backjumps: u64,
    /// Values removed by lookahead.
    pub prunes: u64,
    pub nogoods: u64,
    pub restarts: u64,
}

impl std::ops::AddAssign for Stats {
    fn add_assign(&mut self, o: Stats) {
        self.nodes += o.nodes;
        self.checks += o.checks;
        self.backjumps += o.backjumps;
        self.prunes += o.prunes;
        self.nogoods += o.nogoods;
        self.restarts += o.restarts;
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
    /// The failure cutoff was hit; unwind everything and start over.
    Restart,
}

/// Nogoods longer than this are rarely matched again and cost a check on every visit.
const MAX_NOGOOD: usize = 10;
const MAX_NOGOODS: usize = 200_000;

struct Search<'a, P: Problem> {
    p: &'a P,
    algo: Algo,
    n: usize,
    var_cons: Vec<Vec<usize>>,
    val: Vec<u32>,
    /// removed[v][a] is the depth that pruned value a, or UNSET while it is alive.
    removed: Vec<Vec<u32>>,
    size: Vec<usize>,
    trail: Vec<(usize, u32)>,
    /// Per variable: (depth, culprits) for each prune, so FC-CBJ knows who shrank its domain.
    pruned_by: Vec<Vec<(u32, VarSet)>>,
    weight: Vec<u64>,
    budget: u64,
    stats: Stats,
    /// Soft constraints per variable; only consulted when optimizing.
    soft_cons: Vec<Vec<usize>>,
    optimize: bool,
    cost: u64,
    best: Option<(u64, Vec<u32>)>,
    /// Learned nogoods as (variable, value) literals, indexed by literal.
    nogoods: Vec<Vec<(usize, u32)>>,
    ng_by_lit: Vec<std::collections::HashMap<u32, Vec<usize>>>,
    failures: u64,
    cutoff: u64,
}

impl<P: Problem> Search<'_, P> {
    fn unassigned_in(&self, c: usize) -> usize {
        self.p.scopes()[c].iter().filter(|&&v| self.val[v] == UNSET).count()
    }

    fn check(&mut self, c: usize) -> Result<bool, Fail> {
        self.stats.checks += 1;
        if self.stats.checks > self.budget {
            return Err(Fail::Abort);
        }
        Ok(self.p.holds(c, &self.val))
    }

    fn culprits(&self, c: usize) -> VarSet {
        let mut s = VarSet::new(self.n);
        for &v in &self.p.scopes()[c] {
            if self.val[v] != UNSET {
                s.insert(v);
            }
        }
        s
    }

    fn select(&self) -> Option<usize> {
        let free = (0..self.n).filter(|&v| self.val[v] == UNSET);
        match self.algo.order {
            VarOrder::Static => free.into_iter().next(),
            VarOrder::Dom => free.min_by_key(|&v| self.size[v]),
            VarOrder::DomWdeg => free.min_by(|&a, &b| self.wdeg_score(a).total_cmp(&self.wdeg_score(b))),
        }
    }

    fn wdeg_score(&self, v: usize) -> f64 {
        let w: u64 = self.var_cons[v].iter().filter(|&&c| self.unassigned_in(c) > 1).map(|&c| self.weight[c]).sum();
        self.size[v] as f64 / w.max(1) as f64
    }

    fn prune(&mut self, y: usize, a: usize, depth: u32) {
        self.removed[y][a] = depth;
        self.size[y] -= 1;
        self.trail.push((y, a as u32));
        self.stats.prunes += 1;
    }

    /// revise removes values of `y` that violate `c` given the other scope values; the
    /// other variables must be assigned (FC) or `x` is iterated as the single support (AC).
    fn revise(&mut self, c: usize, y: usize, depth: u32, support: Option<usize>) -> Result<bool, Fail> {
        let mut changed = false;
        for a in 0..self.p.domain_size(y) {
            if self.removed[y][a] != UNSET {
                continue;
            }
            self.val[y] = a as u32;
            let ok = match support {
                None => self.check(c)?,
                Some(x) => {
                    let mut found = false;
                    for b in 0..self.p.domain_size(x) {
                        if self.removed[x][b] == UNSET {
                            self.val[x] = b as u32;
                            if self.check(c)? {
                                found = true;
                                break;
                            }
                        }
                    }
                    self.val[x] = UNSET;
                    found
                }
            };
            self.val[y] = UNSET;
            if !ok {
                self.prune(y, a, depth);
                changed = true;
            }
        }
        if changed {
            let why = match support {
                None => self.culprits(c),
                // AC prunes depend on the whole past; blame every assigned variable.
                Some(_) => self.everyone(),
            };
            self.pruned_by[y].push((depth, why));
        }
        Ok(changed)
    }

    /// lookahead returns the conflict set of the first wiped-out domain, if any.
    fn lookahead(&mut self, x: usize, depth: u32) -> Result<Option<VarSet>, Fail> {
        for i in 0..self.var_cons[x].len() {
            let c = self.var_cons[x][i];
            if self.unassigned_in(c) != 1 {
                continue;
            }
            let y = *self.p.scopes()[c].iter().find(|&&v| self.val[v] == UNSET).unwrap();
            self.revise(c, y, depth, None)?;
            if self.size[y] == 0 {
                self.weight[c] += 1;
                return Ok(Some(self.past_fc(y)));
            }
        }
        if self.algo.lookahead == Lookahead::Mac {
            let mut queue: Vec<(usize, usize)> = Vec::new();
            for (c, s) in self.p.scopes().iter().enumerate() {
                if s.len() == 2 && self.val[s[0]] == UNSET && self.val[s[1]] == UNSET {
                    queue.push((c, s[0]));
                    queue.push((c, s[1]));
                }
            }
            while let Some((c, y)) = queue.pop() {
                let s = &self.p.scopes()[c];
                let x = if s[0] == y { s[1] } else { s[0] };
                if self.revise(c, y, depth, Some(x))? {
                    if self.size[y] == 0 {
                        self.weight[c] += 1;
                        return Ok(Some(self.past_fc(y)));
                    }
                    for &d in &self.var_cons[y] {
                        let s = &self.p.scopes()[d];
                        if d != c && s.len() == 2 {
                            let z = if s[0] == y { s[1] } else { s[0] };
                            if self.val[z] == UNSET {
                                queue.push((d, z));
                            }
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    fn past_fc(&self, y: usize) -> VarSet {
        let mut s = VarSet::new(self.n);
        for (_, why) in &self.pruned_by[y] {
            s.union(why);
        }
        s
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

    fn everyone(&self) -> VarSet {
        let mut s = VarSet::new(self.n);
        (0..self.n).filter(|&v| self.val[v] != UNSET).for_each(|v| s.insert(v));
        s
    }

    /// soft_delta is the weight of soft constraints that `x` just completed and violates.
    fn soft_delta(&mut self, x: usize) -> Result<u64, Fail> {
        let mut d = 0;
        for i in 0..self.soft_cons[x].len() {
            let c = self.soft_cons[x][i];
            if self.unassigned_in(c) == 0 && !self.check(c)? {
                d += self.p.weight(c);
            }
        }
        Ok(d)
    }

    fn learn(&mut self, conf: &VarSet) {
        let lits: Vec<(usize, u32)> = (0..self.n).filter(|&v| conf.contains(v)).map(|v| (v, self.val[v])).collect();
        if lits.is_empty() || lits.len() > MAX_NOGOOD || self.nogoods.len() >= MAX_NOGOODS {
            return;
        }
        for &(v, a) in &lits {
            self.ng_by_lit[v].entry(a).or_default().push(self.nogoods.len());
        }
        self.nogoods.push(lits);
        self.stats.nogoods += 1;
    }

    /// nogood_check tests nogoods mentioning the just-assigned `x`: a fully matched one is a
    /// conflict; with lookahead, one missing a single literal prunes that literal's value.
    fn nogood_check(&mut self, x: usize, depth: u32) -> Result<Option<VarSet>, Fail> {
        let ids = self.ng_by_lit[x].get(&self.val[x]).cloned().unwrap_or_default();
        for id in ids {
            self.stats.checks += 1;
            if self.stats.checks > self.budget {
                return Err(Fail::Abort);
            }
            let mut open = None;
            let mut matched = true;
            for &(v, a) in &self.nogoods[id] {
                if self.val[v] == UNSET {
                    if open.is_some() {
                        matched = false;
                        break;
                    }
                    open = Some((v, a));
                } else if self.val[v] != a {
                    matched = false;
                    break;
                }
            }
            if !matched {
                continue;
            }
            let mut why = VarSet::new(self.n);
            self.nogoods[id].iter().filter(|l| Some(**l) != open).for_each(|&(v, _)| why.insert(v));
            match open {
                None => return Ok(Some(why)),
                Some((y, b)) if self.algo.lookahead != Lookahead::None && self.removed[y][b as usize] == UNSET => {
                    self.prune(y, b as usize, depth);
                    self.pruned_by[y].push((depth, why));
                    if self.size[y] == 0 {
                        return Ok(Some(self.past_fc(y)));
                    }
                }
                Some(_) => {}
            }
        }
        Ok(None)
    }

    fn label(&mut self, depth: u32) -> Result<(), Fail> {
        let Some(x) = self.select() else {
            if !self.optimize {
                return Ok(());
            }
            self.best = Some((self.cost, self.val.clone()));
            // Keep searching for a cheaper solution unless this one is perfect.
            return if self.cost == 0 { Ok(()) } else { Err(Fail::Conflict(self.everyone())) };
        };
        let mut conf = self.past_fc(x);
        for a in 0..self.p.domain_size(x) {
            if self.removed[x][a] != UNSET {
                continue;
            }
            self.stats.nodes += 1;
            self.val[x] = a as u32;
            let mut ok = true;
            if self.algo.learn {
                if let Some(cs) = self.nogood_check(x, depth)? {
                    conf.union(&cs);
                    ok = false;
                }
            }
            if !ok {
            } else if self.algo.lookahead == Lookahead::None {
                for i in 0..self.var_cons[x].len() {
                    let c = self.var_cons[x][i];
                    if self.unassigned_in(c) == 0 && !self.check(c)? {
                        self.weight[c] += 1;
                        conf.union(&self.culprits(c));
                        ok = false;
                        break;
                    }
                }
            } else if let Some(wipe) = self.lookahead(x, depth)? {
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
                let res = self.label(depth + 1);
                self.cost -= delta;
                match res {
                    Ok(()) => return Ok(()),
                    Err(Fail::Abort) => return Err(Fail::Abort),
                    Err(Fail::Restart) => {
                        self.undo(depth);
                        self.val[x] = UNSET;
                        return Err(Fail::Restart);
                    }
                    Err(Fail::Conflict(cs)) => {
                        if self.algo.backjump && !cs.contains(x) {
                            self.stats.backjumps += 1;
                            self.undo(depth);
                            self.val[x] = UNSET;
                            return Err(Fail::Conflict(cs));
                        }
                        conf.union(&cs);
                    }
                }
            }
            self.undo(depth);
            self.val[x] = UNSET;
        }
        conf.remove(x);
        if !self.algo.backjump {
            // Chronological: blame everything assigned, so the parent always retries.
            conf = self.everyone();
            conf.remove(x);
        }
        // Bound failures make conflict sets depend on the incumbent, so learn only when satisfying.
        if self.algo.learn && !self.optimize {
            self.learn(&conf);
        }
        self.failures += 1;
        if self.algo.restarts && self.failures >= self.cutoff {
            return Err(Fail::Restart);
        }
        Err(Fail::Conflict(conf))
    }
}

/// solve searches for an assignment satisfying every hard constraint, within `budget` checks.
/// Soft constraints are ignored.
pub fn solve<P: Problem>(p: &P, algo: Algo, budget: u64) -> (Outcome, Stats) {
    let (out, _, stats) = run(p, algo, budget, false);
    (out, stats)
}

/// optimize is branch and bound: it satisfies every hard constraint while minimizing the
/// total weight of violated soft ones. On budget exhaustion it returns the best found so far
/// as `Sat`; the returned cost is that solution's soft cost.
pub fn optimize<P: Problem>(p: &P, algo: Algo, budget: u64) -> (Outcome, u64, Stats) {
    run(p, algo, budget, true)
}

fn run<P: Problem>(p: &P, algo: Algo, budget: u64, optimize: bool) -> (Outcome, u64, Stats) {
    let n = p.num_vars();
    let mut var_cons = vec![Vec::new(); n];
    let mut soft_cons = vec![Vec::new(); n];
    for (c, s) in p.scopes().iter().enumerate() {
        for &v in s {
            if p.weight(c) == 0 {
                var_cons[v].push(c);
            } else {
                soft_cons[v].push(c);
            }
        }
    }
    let mut s = Search {
        p,
        algo,
        n,
        var_cons,
        val: vec![UNSET; n],
        removed: (0..n).map(|v| vec![UNSET; p.domain_size(v)]).collect(),
        size: (0..n).map(|v| p.domain_size(v)).collect(),
        trail: Vec::new(),
        pruned_by: vec![Vec::new(); n],
        weight: vec![1; p.scopes().len()],
        budget,
        stats: Stats::default(),
        soft_cons,
        optimize,
        cost: 0,
        best: None,
        nogoods: Vec::new(),
        ng_by_lit: vec![std::collections::HashMap::new(); n],
        failures: 0,
        cutoff: 100,
    };
    if s.size.contains(&0) {
        return (Outcome::Unsat, 0, s.stats);
    }
    // Unary constraints have nothing to propagate from; filter them once up front.
    for c in 0..p.scopes().len() {
        if p.scopes()[c].len() == 1 && p.weight(c) == 0 {
            let y = p.scopes()[c][0];
            if s.revise(c, y, 0, None).is_err() {
                return (Outcome::Unknown, 0, s.stats);
            }
            if s.size[y] == 0 {
                return (Outcome::Unsat, 0, s.stats);
            }
        }
    }
    let res = loop {
        match s.label(1) {
            Err(Fail::Restart) => {
                s.stats.restarts += 1;
                s.failures = 0;
                s.cutoff += s.cutoff / 2;
            }
            r => break r,
        }
    };
    let stats = s.stats;
    match (res, s.best) {
        (_, Some((cost, val))) => (Outcome::Sat(val), cost, stats),
        (Ok(()), None) => (Outcome::Sat(s.val), 0, stats),
        (Err(Fail::Conflict(_)), None) => (Outcome::Unsat, 0, stats),
        (Err(Fail::Abort | Fail::Restart), None) => (Outcome::Unknown, 0, stats),
    }
}

/// Rng is xorshift64*, enough for reproducible local search.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.max(1))
    }
    pub fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545F4914F6CDD1D) >> 33) as usize % n.max(1)
    }
}

/// min_conflicts repairs a full assignment: repeatedly move a conflicted variable to the value
/// with the fewest violated constraints (ties: preferred value), with a little random walk.
pub fn min_conflicts<P: Problem>(p: &P, max_steps: u64, seed: u64) -> (Outcome, Stats) {
    let n = p.num_vars();
    let mut var_cons = vec![Vec::new(); n];
    for (c, s) in p.scopes().iter().enumerate() {
        for &v in s {
            var_cons[v].push(c);
        }
    }
    let mut rng = Rng::new(seed);
    let mut stats = Stats::default();
    let mut val: Vec<u32> = vec![0; n];
    let violated = |val: &[u32], c: usize, stats: &mut Stats| {
        stats.checks += 1;
        !p.holds(c, val)
    };
    for _ in 0..max_steps {
        let bad: Vec<usize> = (0..p.scopes().len()).filter(|&c| p.weight(c) == 0 && violated(&val, c, &mut stats)).collect();
        if bad.is_empty() {
            return (Outcome::Sat(val), stats);
        }
        let scope = &p.scopes()[bad[rng.below(bad.len())]];
        let x = scope[rng.below(scope.len())];
        stats.nodes += 1;
        if rng.below(10) == 0 {
            val[x] = rng.below(p.domain_size(x)) as u32;
            continue;
        }
        let mut best = (usize::MAX, val[x]);
        for a in 0..p.domain_size(x) as u32 {
            val[x] = a;
            let cost = var_cons[x].iter().filter(|&&c| p.weight(c) == 0 && violated(&val, c, &mut stats)).count();
            if cost < best.0 {
                best = (cost, a);
            }
        }
        val[x] = best.1;
    }
    (Outcome::Unknown, stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::{Pigeons, Queens};

    const ALL: [&str; 13] = ["bt", "cbj", "fc", "fc-cbj", "mac", "mac-cbj", "fc+dom", "fc-cbj+wdeg", "mac+wdeg", "cbj+learn", "fc-cbj+learn", "fc-cbj+wdeg+restart", "cbj+restart"];

    #[test]
    fn every_algorithm_solves_queens() {
        let q = Queens::new(10);
        for name in ALL {
            let (out, _) = solve(&q, Algo::parse(name).unwrap(), u64::MAX);
            let Outcome::Sat(v) = out else { panic!("{name}: {out:?}") };
            for c in 0..q.scopes().len() {
                assert!(q.holds(c, &v), "{name} returned a bad solution");
            }
        }
    }

    #[test]
    fn every_algorithm_proves_unsat() {
        let p = Pigeons::new(5);
        for name in ALL {
            assert_eq!(solve(&p, Algo::parse(name).unwrap(), u64::MAX).0, Outcome::Unsat, "{name}");
        }
    }

    #[test]
    fn lookahead_checks_fewer_nodes() {
        let q = Queens::new(16);
        let (_, bt) = solve(&q, Algo::BT, u64::MAX);
        let (_, fc) = solve(&q, Algo::FC, u64::MAX);
        let (_, mac) = solve(&q, Algo::MAC, u64::MAX);
        assert!(fc.nodes < bt.nodes && mac.nodes <= fc.nodes, "bt {bt:?} fc {fc:?} mac {mac:?}");
    }

    /// Chain colors a path a-b-c with two colors; soft constraints ask for a=1 and c=0.
    struct Chain {
        scopes: Vec<Vec<usize>>,
    }

    impl Problem for Chain {
        fn num_vars(&self) -> usize {
            3
        }
        fn domain_size(&self, _: usize) -> usize {
            2
        }
        fn scopes(&self) -> &[Vec<usize>] {
            &self.scopes
        }
        fn holds(&self, c: usize, val: &[u32]) -> bool {
            match c {
                0 => val[0] != val[1],
                1 => val[1] != val[2],
                2 => val[0] == 1,
                _ => val[2] == 0,
            }
        }
        fn weight(&self, c: usize) -> u64 {
            [0, 0, 5, 1][c]
        }
    }

    #[test]
    fn branch_and_bound_minimizes_soft_cost() {
        let p = Chain { scopes: vec![vec![0, 1], vec![1, 2], vec![0], vec![2]] };
        for name in ["bt", "cbj", "fc-cbj", "mac+wdeg", "fc-cbj+learn"] {
            let (out, cost, _) = optimize(&p, Algo::parse(name).unwrap(), u64::MAX);
            // a=1 forces b=0, c=1: pay 1 for c rather than 5 for a.
            assert_eq!((out, cost), (Outcome::Sat(vec![1, 0, 1]), 1), "{name}");
        }
        assert_eq!(solve(&p, Algo::BT, u64::MAX).0, Outcome::Sat(vec![0, 1, 0]));
    }

    #[test]
    fn learning_and_restarts_agree_with_plain_search_on_random_coloring() {
        for seed in 1..30 {
            let p = crate::bench::Coloring::random(25, 55, 3, seed);
            let plain = matches!(solve(&p, Algo::FC_CBJ, u64::MAX).0, Outcome::Sat(_));
            for name in ["cbj+learn", "fc-cbj+learn", "fc-cbj+wdeg+restart", "cbj+restart"] {
                let (out, _) = solve(&p, Algo::parse(name).unwrap(), u64::MAX);
                if let Outcome::Sat(v) = &out {
                    assert!((0..p.scopes().len()).all(|c| p.holds(c, v)), "{name} seed {seed}: bad solution");
                }
                assert_eq!(matches!(out, Outcome::Sat(_)), plain, "{name} seed {seed}");
            }
        }
    }

    #[test]
    fn min_conflicts_solves_large_queens() {
        let q = Queens::new(60);
        assert!(matches!(min_conflicts(&q, 5_000, 7).0, Outcome::Sat(_)));
    }
}
