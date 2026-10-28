# Solver

Layout is a finite-domain CSP. `src/csp.rs` is a small solver that knows nothing about diagrams; `src/layout.rs` turns a diagram into a `csp::Problem`.

## Engine

Variables `0..n` are assigned in order; each has domain `0..k` in preference order. Constraints have any arity and are hard (weight 0) or soft (weight > 0).

- **Forward checking.** After each assignment, every hard constraint left with one unassigned variable prunes that variable's domain. A problem may supply `filter` to do this faster than one check per value.
- **Conflict-directed backjumping** (Prosser 1993). Each prune records which assigned variables caused it. When a domain empties, search backs up straight to the deepest variable responsible instead of the previous one.
- **Branch and bound** for `prefer`. After the font is fixed, search continues past the first solution, minimizing the total weight of unmet soft constraints until it proves optimality or the budget runs out (then it keeps the best found).

Budgets count constraint checks, so the same input gives the same output on any machine. `--budget` raises it when a large diagram stops short of the font it could reach.

## Diagrams as CSPs

A strategy (`radial.rs`, `layered.rs`) measures every box at the trial font and proposes candidate centers per node (ordered by preference), structural constraints, and the edges to draw. `layout.rs` adds the user's `require`/`prefer` constraints.

| Constraint | Arity | Where |
|---|---|---|
| `Apart`: two boxes keep a gap | 2 | all pairs (radial), same-layer pairs (layered) |
| `Outward`: child farther out than parent | 2 | radial tree edges |
| `Clear`: edge misses a third box | 3 | every straight edge and every other node in range |
| `NoCross`: two edges do not cross | 4 | hard (radial tree), soft weight 1 (layered) |
| user relation | k | one per `require`/`prefer`; hard alignments split into pairs |

**Radial.** Nodes in BFS order. Candidates are points on stretched polar rays inside the node's wedge (sized by leaf count) plus up to ±30° of nudging, about 3000 per node. Preference: distance from an ideal ring for the node's depth, plus an angle penalty.

**Layered.** Back edges found by DFS are left out of layering and drawn as links; longest-path layering; barycenter sweeps order each layer, which only sets preference. Candidates are positions on one grid shared by all nodes, so centers can line up exactly. Crossings are soft.

**Fast pruning.** Every candidate of a node has the same box, so "this candidate's box meets region R" is "its center lies in R grown by half the box", a range query. Each variable's candidates are bucketed in a grid, and `Apart` and `Clear` (when the box is the open variable) prune through it. On the MATH 4200 map this makes the same search 40% cheaper; a full build drops from 7.0 s to 4.1 s.

## Outer search

1. Binary search on the body font; each probe is one CSP with every `require`.
2. At the best font, branch and bound over `prefer` constraints and soft crossings with a quarter of the budget.
3. If even `--min-font` fails: probe again without user constraints. If that also fails, the page is too small. Otherwise QuickXplain (Junker 2004) finds a minimal set of `require`s that cannot hold together (E0110), in O(k log(n/k)) probes for a core of k out of n. Running out of budget counts as failure, so minimality is relative to the budget.

## Checking

`check.rs` recomputes overlaps, edge clearance, crossings, page bounds, text fit, and every user constraint from the finished layout; any error stops output. User relations share one predicate (`layout::relation_holds`) with the solver; the geometric checks are written separately.

## When a build fails or the font is small

- **E0110:** drop one listed constraint or make it `prefer`. In radial layout, branches stay within 30° of their wedge, so left/right relations between distant branches may be impossible; `layout layered` places nodes freely within layers.
- **Page too small:** larger `page`, lower `font` minimum, or narrower `wrap`.
- **Font lower than expected:** near the largest font that fits, search gets much harder; `--budget 100000000` sometimes finds a slightly larger font. Layered layouts of long chains are limited by the number of layers; try `dir=right` or `page ... portrait`.

## References

- P. Prosser. Hybrid algorithms for the constraint satisfaction problem. *Computational Intelligence* 9(3), 1993.
- U. Junker. QuickXplain: preferred explanations and relaxations for over-constrained problems. AAAI 2004.
- K. Sugiyama, S. Tagawa, M. Toda. Methods for visual understanding of hierarchical system structures. *IEEE SMC* 11(2), 1981.
