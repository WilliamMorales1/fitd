# Internals

## Pipeline

| Stage   | File                      | What it does |
| ------- | ------------------------- | ------------ |
| Lex     | `lex.rs`                  | Tokens with byte spans; newlines are tokens (one statement per line). |
| Parse   | `parse.rs`, `ast.rs`      | Recursive descent; a bad line is reported and skipped, so later errors still show. |
| Analyze | `sema.rs`                 | Two passes: declarations, then references. Styles, typed values, units (`pt mm cm in em`), "did you mean" suggestions, and a static check that hard orderings and alignments do not contradict (union-find plus cycle search). |
| Plan    | `radial.rs`, `layered.rs` | Measure every box with real font metrics, propose candidate positions and structural constraints. |
| Solve   | `csp.rs`, `layout/`       | Small CSP engine: forward checking with conflict-directed backjumping, a spatial index for overlap pruning, branch and bound for `prefer`. Binary search on the font, then a trade-off: the top font and 5% and 10% smaller ones are polished in parallel and the best readability-plus-font merit wins. With nodes fixed, each dashed link picks a bend or a detour around boxes. If nothing fits, QuickXplain extracts a minimal set of conflicting `require`s. |
| Polish  | `quality.rs`, `layout/`   | Simulated annealing moves nodes to improve readability without breaking any constraint: fewer link crossings, related nodes close, branches grouped, edges fanned out, ink spread evenly (straight flow for layered). |
| Check   | `check.rs`                | Recompute every property from the finished layout. Any error stops output. |
| Emit    | `emit.rs`                 | SVG, PNG (via resvg, same font files as measurement), and TikZ with absolute positions; none re-flows text. |

References:

- P. Prosser. Hybrid algorithms for the constraint satisfaction problem. *Computational Intelligence* 9(3), 1993. (conflict-directed backjumping)
- U. Junker. QuickXplain: preferred explanations and relaxations for over-constrained problems. AAAI 2004. (unsat cores)
- K. Sugiyama, S. Tagawa, M. Toda. Methods for visual understanding of hierarchical system structures. *IEEE SMC* 11(2), 1981. (layered layout)

## Diagnostics

Errors point into the source, rustc style:

```
error[E0110]: these 2 constraints cannot all hold at 7pt on this page
  --> core.fitd:11:1
   |
11 | require c at left
   | ^^^^^^^^^^^^^^^^^
   ...
13 | require b left-of c gap=20em
   | ----------------------------
   |
   = note: 7pt is the default font floor; a `font` statement can lower it
   = note: drop any one of them and the rest can be satisfied (within the search budget)
```

## Tests

`cargo test --release`: unit tests for the lexer, parser, semantic checks, geometry, the solver (queens, pigeonhole, branch and bound, spatial index equivalence), layering, and QuickXplain; end-to-end runs of every example (zero errors, a warning baseline, a CPU-time budget, and two concurrent builds that must match byte for byte), unsat cores, static contradictions, and parse errors. A fuzzer feeds mutated sources to the front end for 2 seconds; `FITD_FUZZ_SECS=1800 cargo test --release fuzz` runs it longer.

## Examples

| File | Shows |
| --- | --- |
| `group_theory.fitd` | group theory notes with no settings at all: a large radial map on the defaults, with bodies, math markup, and cross links |
| `french_history.fitd` | real course notes: a layered timeline (down) on a portrait page, colored `group` frames per period, styles, a colored link, `require ... at` and `prefer ... at` |
| `earth_science.fitd` | radial map: `root=`, theme overrides (`root=`, `link=`), styles on nodes and edges, a node `fill`, labeled links, `require` and `prefer` |
| `compiler.fitd` | layered flowchart (down): `theme dark`, `page ... margin=`, `shape=circle` terminals, two groups, a row alignment, `gap=`, `near ... within=` |
| `cell_biology.fitd` | layered flowchart (right) that wraps into two bands: a group frame, circle molecules, a solid `fill` (text turns white), labeled arrows, an edge `color=` |

`tests/fixtures/` holds inputs that fail or strain the solver on purpose.
