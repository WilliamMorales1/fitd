# cmapc

A small diagram language with a constraint solver for layout. Write nodes, edges, and layout constraints in a plain text file; cmapc finds the largest font that fits a fixed page, satisfies every `require`, trades off every `prefer`, then verifies the result before writing SVG or TikZ.

```
title "How cmapc compiles a diagram"
layout layered
page a3

node lex "Lexer"
| tokens with byte spans
node parse "Parser"
node bnb "Branch and bound"
node core "QuickXplain"

lex -> parse
parse -> bnb, core
core ..> lex "points back at source"

require bnb left-of core
require align row(bnb, core)
prefer core at bottom-right weight=2
```

```fish
cargo build --release
./target/release/cmapc build examples/compiler.cmap --tikz out/compiler.tex
./target/release/cmapc build examples/math4200.cmap --page 2296x1524pt
./target/release/cmapc bench                 # every search algorithm on classic CSPs
./target/release/cmapc explain E0110
```

## Pipeline

| Stage | File | What it does |
|---|---|---|
| Lex | `lex.rs` | Tokens with byte spans; newlines are tokens (one statement per line). |
| Parse | `parse.rs`, `ast.rs` | Recursive descent; a bad line is reported and skipped, so later errors still show. |
| Analyze | `sema.rs` | Two passes: declarations, then references. Styles, typed values, units (`pt mm cm in em`), "did you mean" suggestions, and a static check that hard orderings and alignments do not contradict (union-find plus cycle search). |
| Plan | `radial.rs`, `layered.rs` | Measure every box with real font metrics, propose candidate positions and structural constraints. |
| Solve | `csp.rs`, `layout.rs` | Generic CSP engine (BT, CBJ, FC, FC-CBJ, MAC; static, dom, dom/wdeg ordering; nogood learning and restarts; branch and bound; min-conflicts). Binary search on the font, then soft-constraint optimization. If nothing fits, QuickXplain extracts a minimal set of conflicting `require`s. |
| Check | `check.rs` | Recompute every property from the finished layout. Any error stops output. |
| Emit | `emit.rs` | SVG and TikZ with absolute positions; neither re-flows text. |

Errors point into the source, rustc style:

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

## Options

`--page a0..a4 | WxH[pt|mm]`, `--portrait`, `--margin PT`, `--min-font PT`, `--max-font PT`, `--wrap EM`, `--algo bt|cbj|fc|fc-cbj|mac|mac-cbj[+dom|+wdeg][+learn|+restart]` (default `fc-cbj`), `--budget CHECKS` (default 20M per probe), `--force` (write output despite checker errors). Flags override settings in the file. `CMAPC_DEBUG=1` traces each probe.

## Tests

`cargo test --release`: unit tests for the lexer, parser, semantic checks, geometry, every search algorithm (queens, pigeonhole, branch and bound), layering, and QuickXplain; end-to-end runs for both examples, unsat cores, static contradictions, parse errors, and `bench`.

## Docs

- `docs/LANGUAGE.md`: grammar, semantics, relations, diagnostics.
- `docs/SOLVER.md`: the engine, the diagram encoding, unsat cores, benchmarks and what they show.
- `docs/PLANS.md`: what comes next.
- `docs/PRIOR_ART.md`: related tools.
- `docs/SESSION_RETRO.md`: the hand-made map that motivated this.
