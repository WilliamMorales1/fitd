# Solver

Layout is a finite-domain CSP. `src/csp.rs` is a small solver that knows nothing about diagrams; `src/layout/` turns a diagram into a `csp::Problem`.

## Engine

Variables `0..n` are assigned in order; each has domain `0..k` in preference order. Constraints have any arity and are hard (weight 0) or soft (weight > 0).

- **Forward checking.** After each assignment, every hard constraint left with one unassigned variable prunes that variable's domain. A problem may supply `filter` to do this faster than one check per value.
- **Arc consistency at the root.** Before search, AC-3 runs over binary hard constraints the problem marks with `propagate` (the scene marks `require` relations and alignments). Forward checking sees one link of a chain such as `a above b above c` at a time, so on `cell_biology.fitd` with two 30em `above` steps it spent 200M checks without finding the layout that exists; AC-3 prunes the impossible spots up front, it builds in 1 s, and an over-long chain is proved unsat with no search. Structural constraints (`Apart`, `Clear`) are not propagated: almost every value has support, so the pass would cost checks for nothing.
- **Conflict-directed backjumping** (Prosser 1993). Each prune records which assigned variables caused it. When a domain empties, search backs up straight to the deepest variable responsible instead of the previous one.
- **Branch and bound** for `prefer`. After the font is fixed, search continues past the first solution, minimizing the total weight of unmet soft constraints until it proves optimality or the budget runs out (then it keeps the best found).

Budgets count constraint checks, so the same input gives the same output on any machine. `--budget` raises it when a large diagram stops short of the font it could reach.

## Diagrams as CSPs

A strategy (`radial.rs`, `layered.rs`) measures every box at the trial font and proposes candidate centers per node (ordered by preference), structural constraints, and the edges to draw. `layout/scene.rs` adds the user's `require`/`prefer` constraints.

| Constraint | Arity | Where |
|---|---|---|
| `Apart`: two boxes keep a gap | 2 | all pairs (radial), same-layer pairs (layered) |
| `Outward`: child farther out than parent | 2 | radial tree edges |
| `Clear`: edge misses a third box | 3 | every straight edge and every other node in range |
| `NoCross`: two edges do not cross | 4 | hard (radial tree), soft weight 1 (layered) |
| user relation | k | one per `require`/`prefer`; hard alignments split into pairs |
| label clear | 3-4 | an edge label stays off every node and on the page (hard) |
| labels apart | up to 6 | two labels do not overlap (hard) |
| links apart | up to 6 | two dashed links neither cross nor run within half an em (soft, 1) |
| link clear | 4 | a dashed link stays out of an unrelated node (soft, 2) |

**Routes.** Every dashed link, and every edge with a label, gets a route variable after all the nodes. Its values pair a bend (8 for links, straight otherwise) with a label position along the path (9 spots, middle first). Once nodes are fixed (step 3 below), a link that cannot run straight also gets detours: the shortest path around the other boxes, grown by 0.4em, over their corners (a visibility graph), drawn once as a centripetal Catmull-Rom spline and once with sharp corners in case the spline cuts into a box. Solver, checker, and emitter all draw the path with `Layout::path`, so the label the solver cleared is the label that gets drawn. Labels are drawn last, so nothing covers them. Because label constraints are hard, a node layout that leaves no room for a label is rejected during the font search, not patched afterwards.

**Radial.** Nodes in BFS order. Candidates are points on stretched polar rays inside the node's wedge (sized by leaf count) plus up to ±30° of nudging, about 3000 per node. Preference: distance from an ideal ring for the node's depth, plus an angle penalty.

**Layered.** Back edges found by DFS are left out of layering and drawn as links; longest-path layering; barycenter sweeps order each layer, which only sets preference. Candidates are positions on one grid shared by all nodes, so centers can line up exactly. Crossings are soft.

**Fast pruning.** Every candidate of a node has the same box, so "this candidate's box meets region R" is "its center lies in R grown by half the box", a range query over a per-node grid of candidate centers. `Apart`, `Clear` (box open), and a group's `Outside` (the non-member open) prune through that query. When an edge endpoint is open instead, a candidate can only fail inside the wedge seen from the fixed endpoint across the box (or across the other edge, for crossings); grid cells outside the wedge are skipped. Only still-alive candidates are tested. On the group theory map a build went from 7.0 s to 1.6 s at the same font. The grid is a flat, column-major array of cells (not a map), so a range query reads one contiguous slice per column; with the forward-checking scan inlined instead of called through closures, this halved the CPU time of a build again.

## Outer search

1. Binary search on the body font; each probe is one CSP with every `require`. If the first probe fails, the floor is probed next with the full budget, so an unsatisfiable input stops there instead of running every probe down to the floor on its cap. In layered layout, group members are assigned first: once a frame is known, forward checking keeps other nodes off it (a group on `compiler.fitd` at a3 went from 5.6M checks to 77k). A radial probe solves four plans in parallel, with the branches rotated 0, 10, 30, and 45 degrees, and keeps the first in that order that solves (later ones stop once an earlier one wins, so the result is deterministic). Near the threshold, which rotation fits is close to chance: alone they range from 15.7pt to 19.8pt on `group_theory.fitd`.
2. Font trade-off. The binary search stops at 3% precision, since this step moves in 5% steps anyway. The largest feasible font F, 0.95F, and 0.9F (only those at or above the legibility floor) are each laid out and polished in parallel, and the lowest merit wins: readability penalty, plus 1000 per unmet `prefer` weight, minus 800 times ln(font / floor). The floor is the smallest comfortable body font for a page read from about its diagonal, 16pt per meter (poster guides give roughly 24pt at 1.5m), at least 8pt and at least `--min-font`. Above the floor a larger font has diminishing returns (log), while crossings and proximity matter most for understanding (Purchase 1997), so the font is one criterion, not the first. All lengths in the penalty are in floor ems, the same unit at every font, so smaller fonts are not penalized for their own em. A smaller font starts from the larger layout's spots, which still fit since boxes shrink around the same centers; this makes those probes immediate instead of a gamble near the satisfiability threshold. The build prints each font's merit. Each layout first gets branch and bound over `prefer` constraints and soft crossings with a sixteenth of the budget, starting from the known spots, then the polish below.
   Polish for readability: simulated annealing (150k moves, seeded, so deterministic) moves one node at a time to a candidate within 10em, never breaking a hard constraint, rescoring only the terms the move touches (`quality::Incremental`), and minimizes the readability penalty from `quality.rs` plus 1000 per unit of unmet `prefer` weight (a `prefer` is the author's intent, so aesthetics never outweigh it). The penalty follows the graph-drawing literature, most important first: crossings and links tunneling through boxes, at each link's best bend (Purchase 1997; Dunne and Shneiderman 2009); proximity, the squared gap of every edge and link, which also evens out edge lengths (Gestalt proximity; Huang and Eades); grouping, a node nearer another branch than its own parent; angular resolution at each node; and either even ink coverage over the page (radial) or straight edges along the flow (layered; Purchase, Pilcher, and Plimmer 2012). The build prints the final penalty per term. On `group_theory.fitd` at 19.8pt polishing cut it from 1386 to 893; the trade-off then picked 17.8pt (penalty 551, link crossings and tunnels down from 9 to 5).
3. With nodes fixed, branch and bound over link routes alone. Each link's bends are ordered by how many boxes they pass through, so a clear routing is found first even if the budget runs out; on the examples the search is exhaustive, so the links are as clear of nodes and of each other as the node positions allow. Letting links move nodes made the search too large to finish.
4. If even `--min-font` fails: probe again without user constraints. If that also fails, the page is too small. Otherwise QuickXplain (Junker 2004) finds a minimal set of `require`s that cannot hold together (E0110), in O(k log(n/k)) probes for a core of k out of n. Running out of budget counts as failure, so minimality is relative to the budget. The full set already failed on the whole budget; core probes get a quarter of it, since subsets that fit usually solve fast.

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
