# Internals

## Pipeline

| Stage   | File                      | What it does |
| ------- | ------------------------- | ------------ |
| Include | `include.rs`              | Splices included files in, line for line; a segment table maps every line back to its file. |
| Expand  | `macros.rs`               | Runs `for`, `if`, `def`, `use`, and `${...}` within a step budget. Every input line keeps one raw copy in the expanded text (blanked for the parser when it is a directive), so diagnostics can show it; generated lines map to the template line that wrote them. Files with no macro lines pass through untouched. |
| Lex     | `lex.rs`                  | Tokens with byte spans; newlines are tokens (one statement per line). |
| Parse   | `parse.rs`, `ast.rs`      | Recursive descent; a bad line is reported and skipped, so later errors still show. |
| Analyze | `sema.rs`                 | Two passes: declarations, then references. Styles, typed values, units (`pt mm cm in em`), "did you mean" suggestions, and a static check that hard orderings and alignments do not contradict (union-find plus cycle search). |
| Plan    | `radial.rs`, `layered.rs` | Measure every box with real font metrics, propose candidate positions and structural constraints. |
| Solve   | `csp.rs`, `layout/`       | Small CSP engine: forward checking with conflict-directed backjumping, a spatial index for overlap pruning, branch and bound for `prefer`. Binary search on the font, then a trade-off: the top font and 5% and 10% smaller ones are polished in parallel and the best readability-plus-font merit wins. With nodes fixed, each dashed link picks a bend or a detour around boxes. If nothing fits, QuickXplain extracts a minimal set of conflicting `require`s. |
| Polish  | `quality.rs`, `layout/`   | Simulated annealing moves nodes to improve readability without breaking any constraint: fewer link crossings, related nodes close, branches grouped, edges fanned out, ink spread evenly (straight flow for layered). |
| Check   | `check.rs`                | Recompute every property from the finished layout. Any error stops output. |
| Emit    | `emit.rs`                 | SVG, PNG (via resvg, same font files as measurement), and TikZ with absolute positions; none re-flows text. |

Fonts: DejaVu Serif (Book, Bold), DejaVu Sans Bold, and DejaVu Math TeX Gyre are compiled in (`fonts/`, license in `fonts/LICENSE`) behind the default `embedded-fonts` cargo feature, so layout does not depend on installed fonts or on fontconfig. Only a named `family` goes through `fc-match`. Every SVG carries a `<!-- fitd fonts: ... -->` comment naming each font file with an FNV-1a hash of its bytes, so a layout difference between machines can be traced to fonts. Building with `--no-default-features` restores the old fc-match chain.

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
   = note: drop any one of them and the rest can be satisfied
   = note: proved for every candidate position this layout considers
   = help: remove one, turn it into `prefer`, or use a larger page
```

## Tests

`cargo test --release`: unit tests for macro expansion (loops, templates, recursion, the budget, line mapping), the lexer, parser, semantic checks, geometry, the solver (queens, pigeonhole, branch and bound, spatial index equivalence), layering, and QuickXplain; end-to-end runs of every example (zero errors, a warning baseline, a CPU-time budget, and two concurrent builds that must match byte for byte), unsat cores, static contradictions, parse errors, and macro diagnostics across includes. A fuzzer feeds mutated sources, macro lines among them, to the front end for 2 seconds; `FITD_FUZZ_SECS=1800 cargo test --release fuzz` runs it longer. `tests/fixtures/` holds inputs that fail or strain the solver on purpose, and `collatz.fitd`, a template with no fixed bound.

## Performance

`scripts/bench.py` builds every example and fixture and reports the median of three runs. Measured on a ThinkPad X280 (i7-8650U, 4 cores and 8 threads, 16 GB, Linux 7.3, rustc 1.99.1):

| File | Wall s | CPU s | Checks | Font pt |
| --- | ---: | ---: | ---: | ---: |
| `examples/cell_biology.fitd` | 0.38 | 0.96 | 2,143,098 | 16.9 |
| `examples/compiler.fitd` | 1.90 | 4.70 | 4,028,513 | 9.1 |
| `examples/earth_science.fitd` | 3.60 | 14.27 | 80,934,070 | 14.5 |
| `examples/french_history.fitd` | 0.42 | 0.42 | 2,317,945 | 11.7 |
| `examples/group_theory.fitd` | 1.62 | 4.26 | 44,886,959 | 17.3 |
| `fixtures/complex_constraints.fitd` | 0.09 | 0.22 | 388,135 | 30.1 |
| `fixtures/missing_glyph.fitd` | 0.07 | 0.18 | 0 | 48.0 |
| `fixtures/stress_test.fitd` | 0.00 | 0.00 | 0 | fails (by design) |

`compiler.fitd` spends most of its time in the warning-driven retry (turned page). `bench/baseline.tsv` holds the checks and CPU seconds above. CI runs `scripts/bench.py --check bench/baseline.tsv`, which fails when any file needs over 25% more solver checks; checks are deterministic, so this holds on any machine. `--time` also gates CPU time, which is only meaningful on the machine that wrote the baseline (`--write`), and even there one run can vary by a third under thermal throttling, so use the default three runs.
