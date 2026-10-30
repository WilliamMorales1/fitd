//! Annealing on readability once every hard constraint holds.

use super::*;

/// Readable holds what `quality::score` needs that stays fixed while nodes move.
pub struct Readable {
    pub tree: Vec<(usize, usize)>,
    pub links: Vec<(usize, usize)>,
    pub branch: Vec<Option<usize>>,
    pub flow: Option<bool>,
    pub page: Rect,
    pub unit: f64,
}

impl Readable {
    pub fn new(t: &Tree, kind: LayoutKind, edges: &[Drawn], page: Rect, unit: f64) -> Readable {
        let pairs = |link: bool| edges.iter().filter(|e| (e.style == EdgeStyle::Link) == link).map(|e| (e.a, e.b)).collect();
        // Branch grouping only means something when branches own directions, as in radial.
        let branch = (0..t.parent.len()).map(|v| if kind == LayoutKind::Radial { t.branch(v) } else { None }).collect();
        let flow = match kind {
            LayoutKind::Radial => None,
            LayoutKind::Layered(dir) => Some(dir == Dir::Down),
        };
        Readable { tree: pairs(false), links: pairs(true), branch, flow, page, unit }
    }

    pub fn score(&self, rects: &[Rect]) -> Score {
        quality::score(&quality::Input { rects, tree: &self.tree, links: &self.links, branch: &self.branch, flow: self.flow, page: self.page, unit: self.unit })
    }
}

/// Moves tried by `polish`, and how far one move may go, in ems.
pub(super) const POLISH_MOVES: usize = 150_000;

pub(super) const POLISH_REACH: f64 = 10.0;

/// Readability penalty per unit of unmet `prefer` weight; high, since a `prefer` is the
/// author's stated intent and the aesthetics are only general guidance.
pub(super) const SOFT_PENALTY: f64 = 1000.0;

/// polish moves one node at a time to a nearby candidate spot, keeping every hard constraint,
/// and anneals on readability plus unmet `prefer` weight. The walk is seeded, so the result is
/// deterministic. Returns the final soft cost.
pub(super) fn polish(sc: &Scene, ctx: &Readable, val: &mut [u32], moves: usize) -> u64 {
    let nv = sc.node.len();
    let mut hard = vec![Vec::new(); nv];
    let mut soft = vec![Vec::new(); nv];
    for (c, scope) in sc.scopes.iter().enumerate() {
        for &v in scope.iter().filter(|&&v| v < nv) {
            if sc.weights[c] == 0 { hard[v].push(c) } else { soft[v].push(c) }
        }
    }
    let unmet = |cs: &[usize], val: &[u32]| cs.iter().filter(|&&c| !sc.holds(c, val)).map(|&c| sc.weights[c]).sum::<u64>();
    let mut rects: Vec<Rect> = (0..nv).map(|v| sc.node_rect(v, val)).collect();
    let mut soft_cost: u64 = (0..sc.scopes.len()).filter(|&c| sc.weights[c] > 0 && !sc.holds(c, val)).map(|c| sc.weights[c]).sum();
    let energy = |rects: &[Rect], soft: u64| ctx.score(rects).total() + SOFT_PENALTY * soft as f64;
    let mut cur = energy(&rects, soft_cost);
    let (mut best, mut best_val, mut best_soft) = (cur, val.to_vec(), soft_cost);
    let movable: Vec<usize> = (0..nv).filter(|&i| sc.at[i].len() > 1).collect();
    if movable.is_empty() {
        return soft_cost;
    }
    let mut rng: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = |k: usize| {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng % k as u64) as usize
    };
    // Temperature falls geometrically from about one em of edge to almost nothing.
    let (t0, t1): (f64, f64) = (2.0, 0.01);
    let mut near = Vec::new();
    for k in 0..moves {
        let temp = t0 * (t1 / t0).powf(k as f64 / moves as f64);
        let i = movable[next(movable.len())];
        let u = sc.node[i];
        let reach = POLISH_REACH * sc.body;
        near.clear();
        sc.grids[i].near(&Rect::centered(sc.at[i][val[i] as usize], 2.0 * reach, 2.0 * reach), &mut |a| near.push(a));
        if near.is_empty() {
            continue;
        }
        let old = val[i];
        let a = near[next(near.len())];
        if a == old {
            continue;
        }
        let before = unmet(&soft[i], val);
        val[i] = a;
        if !hard[i].iter().all(|&c| sc.holds(c, val)) {
            val[i] = old;
            continue;
        }
        let new_soft = soft_cost - before + unmet(&soft[i], val);
        let old_rect = rects[u];
        rects[u] = sc.rects[i][a as usize];
        let e = energy(&rects, new_soft);
        let x = (next(1 << 20) as f64 + 0.5) / (1 << 20) as f64;
        if e <= cur || x < ((cur - e) / temp).exp() {
            cur = e;
            soft_cost = new_soft;
            if cur < best {
                (best, best_soft) = (cur, soft_cost);
                best_val.copy_from_slice(&val[..]);
            }
        } else {
            val[i] = old;
            rects[u] = old_rect;
        }
    }
    val.copy_from_slice(&best_val);
    best_soft
}
