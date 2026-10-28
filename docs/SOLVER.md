# Solver

Layout is a finite-domain CSP. `src/csp.rs` is a generic solver that knows nothing about diagrams; `src/layout.rs` turns a diagram into a `csp::Problem`.

## The engine

A problem has variables `0..n`, each with domain `0..k` in preference order, and constraints given as a scope (any arity) plus a predicate. A constraint is hard (weight 0) or soft (weight > 0).

Search is recursive labeling with three independent knobs, selected by `--algo`:

| Knob | Options |
|---|---|
| Lookahead | none (`bt`, `cbj`); forward checking (`fc`), which prunes a variable once it is the last unassigned one in some scope, so n-ary constraints propagate; MAC (`mac`), which also maintains arc consistency over binary constraints between unassigned variables (AC-3 queue). |
| Lookback | chronological, or conflict-directed backjumping (`-cbj`, Prosser 1993). With lookahead this is FC-CBJ: every prune records its culprits, and a wipeout adds the wiped variable's culprits to the current conflict set. |
| Variable order | static (strategy order), `+dom` (smallest live domain), `+wdeg` (domain over weighted degree, Boussemart et al. 2004; a constraint's weight grows each time it fails or wipes out a domain). |

Conflict sets are bitsets. A failure returns the set of assigned variables that caused it; each level either jumps past itself (its variable is not in the set) or merges the set into its own and tries its next value. Chronological search uses the same code with the conflict set "everything assigned", so it always stops at the parent.

**Branch and bound** (`csp::optimize`) adds soft constraints. Lookahead ignores them. When a soft constraint becomes fully assigned and fails, its weight is added to the path cost; a path whose cost already reaches the best solution's cost fails with conflict set "everything assigned", which keeps backjumping sound. Each solution tightens the bound; the search is anytime and returns the best solution found when its budget runs out.

**Min-conflicts** (Minton et al. 1992) is local search: start from the preferred values and repeatedly move a variable in a violated constraint to its least-conflicting value, with 10% random walk.

Budgets count constraint checks, so the same input gives the same output on any machine.

## Diagrams as CSPs

A strategy (`radial.rs`, `layered.rs`) measures every box at the trial font and proposes:

- candidate centers per node, ordered by a preference cost (the value ordering);
- structural constraints over nodes;
- the edges to draw.

`layout.rs` adds the user's `require`/`prefer` constraints and hands the result to the engine.

| Constraint | Arity | Where |
|---|---|---|
| `Apart`: two boxes keep a gap | 2 | all pairs (radial), same-layer pairs (layered) |
| `Outward`: child farther out than parent | 2 | radial tree edges |
| `Clear`: edge misses a third box | 3 | every straight edge and every other node in range |
| `NoCross`: two edges do not cross | 4 | hard (radial tree), soft weight 1 (layered) |
| user `Rel` | k | one constraint per `require`/`prefer` |
| user `Aligned` | 2 | hard alignments split into pairs, which propagate better |

**Radial.** Variables in BFS order. A node's candidates are (angle, radius) points on stretched polar rays inside its wedge (sized by leaf count) plus up to ±30° of nudging, about 3000 per node. Cost is distance from an ideal ring for its depth plus an angle penalty.

**Layered.** Sugiyama's pipeline with the final step replaced: DFS back edges are excluded from layering; longest-path layering; eight barycenter sweeps order each layer. The order only sets value preference. Candidates are cross-axis positions on one grid shared by all nodes (so centers can line up exactly), about 200 per node. Crossings are soft, so after the font is fixed branch and bound minimizes them along with user preferences.

**Outer search.** Binary search on the body font: each probe is one CSP. Then branch and bound at the best font with a quarter of the budget.

**Unsat cores.** If the smallest font fails, the solver checks whether it still fails with no user constraints (then the page is simply too small). Otherwise QuickXplain (Junker 2004) finds a minimal subset of `require` constraints that is infeasible, using "probe at the minimum font" as the oracle. A core of size k among n constraints takes O(k log(n/k)) probes. Budget exhaustion counts as infeasible, so minimality is relative to the budget.

```
error[E0110]: these 2 constraints cannot all hold at 7pt on this page
  --> core.cmap:11:1
   |
11 | require c at left
   | ^^^^^^^^^^^^^^^^^
   ...
13 | require b left-of c gap=20em
   | ----------------------------
   |
   = note: drop any one of them and the rest can be satisfied (within the search budget)
```

**Verification.** `check.rs` recomputes overlaps, edge clearance, crossings, page bounds, text fit, and every user constraint from the finished layout. Any error stops output. User relations share one predicate (`layout::relation_holds`) between solver and checker; geometry checks are written separately.

## Benchmarks

`cmapc bench` (50M-check budget, i7-8650U, release build). Nodes are assignments tried.

| problem | algorithm | result | nodes | checks | ms |
|---|---|---|---:|---:|---:|
| queens-28 | bt | budget | 6012171 | 50000001 | 588 |
| queens-28 | cbj | budget | 5984913 | 50000001 | 566 |
| queens-28 | fc | sat | 2042129 | 48544537 | 2143 |
| queens-28 | mac | budget | 65584 | 50000001 | 1973 |
| queens-28 | fc+dom | sat | 28 | 5665 | 0.1 |
| queens-28 | min-conflicts | sat | 43 | 46116 | 0.2 |
| pigeons-8 | bt | unsat | 876808 | 3781240 | 71 |
| pigeons-8 | cbj | unsat | 876808 | 3781240 | 68 |
| pigeons-8 | fc | unsat | 109600 | 547912 | 31 |
| pigeons-8 | mac | unsat | 69280 | 1481880 | 53 |
| color-60x3 | bt | budget | 23738908 | 50000001 | 3299 |
| color-60x3 | cbj | unsat | 17459789 | 36140529 | 1665 |
| color-60x3 | fc | budget | 11260228 | 50000001 | 2998 |
| color-60x3 | fc-cbj | unsat | 276404 | 1466749 | 88 |
| color-60x3 | mac | unsat | 777 | 424848 | 11 |
| color-60x3 | fc-cbj+wdeg | unsat | 115 | 1287 | 0.5 |

- **Queens:** variable ordering dominates. With static order every method thrashes; `dom` solves 28-queens almost without backtracking.
- **Pigeonhole:** every conflict involves every pigeon, so backjumping never skips a level (CBJ = BT). Lookahead cuts nodes 8x. All methods stay exponential, as theory says.
- **3-coloring at the phase transition** (60 vertices, average degree 4.6, unsatisfiable): structure matters. CBJ beats BT, FC-CBJ beats both FC and CBJ by 40x or more, and dom/wdeg learns which constraints are hard, finishing in 115 nodes.

The diagram (`examples/math4200.cmap` on 2296x1524pt, 20M-check budget):

| font | bt | cbj | fc | fc-cbj | mac | fc+dom | fc-cbj+wdeg |
|---|---|---|---|---|---|---|---|
| 18pt | sat, 14 ms | sat, 15 ms | sat, 64 ms | sat, 64 ms | budget | budget | sat, 265 ms |
| 19.8pt | budget | budget | budget | **sat, 121 ms** | budget | budget | budget |

- Far from the threshold, plain backtracking is fastest: checks are cheap and the value ordering is good.
- At the threshold (the largest font that fits), only FC-CBJ finishes. That is why it is the default.
- MAC never finishes: one arc revision between two ~3000-value domains is up to 9M checks. Arc consistency pays off for small domains (coloring), not dense geometric ones.
- `dom` misleads: a geometric domain's size measures how much page room a node has, not how constrained it is.
- Difficulty spikes at the satisfiability threshold, the same phase-transition behavior as random CSPs: 19.8pt solves in 4.9M checks, 19.85pt exhausts 20M.

## References

- P. Prosser. Hybrid algorithms for the constraint satisfaction problem. *Computational Intelligence* 9(3), 1993.
- D. Sabin, E. Freuder. Contradicting conventional wisdom in constraint satisfaction. ECAI 1994.
- F. Boussemart, F. Hemery, C. Lecoutre, L. Sais. Boosting systematic search by weighting constraints. ECAI 2004.
- S. Minton, M. Johnston, A. Philips, P. Laird. Minimizing conflicts: a heuristic repair method. *Artificial Intelligence* 58, 1992.
- U. Junker. QuickXplain: preferred explanations and relaxations for over-constrained problems. AAAI 2004.
- K. Sugiyama, S. Tagawa, M. Toda. Methods for visual understanding of hierarchical system structures. *IEEE SMC* 11(2), 1981.
- P. Cheeseman, B. Kanefsky, W. Taylor. Where the really hard problems are. IJCAI 1991.
