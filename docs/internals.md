# Implementation notes

Source code is organized under `src/` by compiler stage and layout subsystem.

## Compilation pipeline

Compilation performs these stages:

1. Resolve and splice `include` files.
2. Expand macros and preserve source mappings.
3. Tokenize source and parse statements.
4. Resolve declarations and validate attribute types.
5. Construct layout variables and constraints.
6. Search candidate layouts and optimize preferences.
7. Refine selected layout.
8. Recompute geometric checks on final drawing.
9. Emit requested output formats.

Include and macro expansion preserve original file and line information for diagnostics. Output is written only after parsing, semantic analysis, layout, and checking complete, unless `--force` is supplied.

## Layout search

The solver searches font sizes within configured bounds. At each size, it checks hard constraints and optimizes weighted `prefer` constraints. Branch-and-bound minimizes total unmet preference weight. A bounded search may stop before proving satisfiability or unsatisfiability; diagnostics distinguish budget exhaustion from proven conflict.

Candidate layouts are refined and scored using readability criteria. Final geometry is passed to an independent checker, which recomputes node overlap, edge crossing, page clipping, and overflow checks.

When hard constraints are unsatisfiable, QuickXplain extracts a minimal conflicting set for E0110 where search establishes the conflict. Static ordering contradictions may be detected before layout search.

## Algorithms and references

- Prosser, P. *Hybrid Algorithms for the Constraint Satisfaction Problem*. Computational Intelligence 9(3), 1993. Conflict-directed backjumping.
- Junker, U. *QuickXplain: Preferred Explanations and Relaxations for Over-Constrained Problems*. AAAI 2004. Minimal conflict extraction.
- Sugiyama, K., Tagawa, S., and Toda, M. *Methods for Visual Understanding of Hierarchical System Structures*. IEEE Transactions on Systems, Man, and Cybernetics 11(2), 1981. Layered graph layout.

## Tests and benchmarks

`cargo test --release` runs unit tests, end-to-end tests, fixtures, a short fuzzer, and comparisons between committed documentation renders and fresh builds. Set `FITD_FUZZ_SECS` to change fuzzer duration.

`python3 bench/bench.py --check bench/baseline.tsv` checks deterministic solver-work counts against committed baseline. Benchmarks track checks rather than elapsed time to reduce machine-dependent variation.
