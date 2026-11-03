# Internals

Source lives in `src/`. The front end is about a quarter of the code; the solver and checker are the rest.

## Pipeline

Ten stages, each in its own file: include, expand (macros), lex, parse, analyze, plan, solve, polish, check, emit. A diagnostic always maps back to the original source line, through macro expansion and includes. The checker recomputes every property from the finished layout and any error stops output.

![From source to drawing](internals/01-pipeline.png)

Source: [01-pipeline.fitd](internals/01-pipeline.fitd)

## Solver

A binary search over font size, with a constraint probe at each step (forward checking, backjumping, spatial index). Branch and bound then minimizes unmet `prefer` weight. The top font and two smaller ones are polished by annealing, and the best readability-plus-font score wins. If nothing fits, QuickXplain extracts a minimal conflicting set of `require`s (E0110).

References: Prosser 1993 (conflict-directed backjumping), Junker, AAAI 2004 (QuickXplain), Sugiyama, Tagawa, Toda 1981 (layered layout).

![How the solver picks a font](internals/02-solver.png)

Source: [02-solver.fitd](internals/02-solver.fitd)

## Tests and benchmarks

`cargo test --release` runs unit tests, end-to-end tests over every example and fixture, a fuzzer (`FITD_FUZZ_SECS` sets its length), and a check that every committed image matches a fresh build. `python3 bench/bench.py --check bench/baseline.tsv` fails when any file needs over 25% more solver checks; checks are deterministic, so the gate holds on any machine.

![Tests and performance](internals/03-tests.png)

Source: [03-tests.fitd](internals/03-tests.fitd)
