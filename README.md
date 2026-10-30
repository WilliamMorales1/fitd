# cmapc

A small diagram language with a constraint solver for layout. Write nodes, edges, and layout constraints in a plain text file; cmapc finds the largest font that fits a fixed page, satisfies every `require`, trades off every `prefer`, then verifies the result before writing SVG, PNG, or TikZ.

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

![examples/compiler.cmap rendered to PNG](docs/compiler.png)

```fish
cargo build --release
./target/release/cmapc build examples/compiler.cmap --tikz out/compiler.tex
./target/release/cmapc build examples/compiler.cmap -o docs/compiler.png
./target/release/cmapc build examples/math4200.cmap --page 2296x1524pt
./target/release/cmapc explain E0110
```

## Pipeline

| Stage   | File                      | What it does                                                                                                                                                                                                                                                                            |
| ------- | ------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Lex     | `lex.rs`                  | Tokens with byte spans; newlines are tokens (one statement per line).                                                                                                                                                                                                                   |
| Parse   | `parse.rs`, `ast.rs`      | Recursive descent; a bad line is reported and skipped, so later errors still show.                                                                                                                                                                                                      |
| Analyze | `sema.rs`                 | Two passes: declarations, then references. Styles, typed values, units (`pt mm cm in em`), "did you mean" suggestions, and a static check that hard orderings and alignments do not contradict (union-find plus cycle search).                                                          |
| Plan    | `radial.rs`, `layered.rs` | Measure every box with real font metrics, propose candidate positions and structural constraints.                                                                                                                                                                                       |
| Solve   | `csp.rs`, `layout/`       | Small CSP engine: forward checking with conflict-directed backjumping, a spatial index for overlap pruning, branch and bound for `prefer`. Binary search on the font, then a trade-off: the top font and 5% and 10% smaller ones are polished in parallel and the best readability-plus-font merit wins. If nothing fits, QuickXplain extracts a minimal set of conflicting `require`s. |
| Polish  | `quality.rs`, `layout/`   | Simulated annealing moves nodes to improve readability without breaking any constraint: fewer link crossings, related nodes close, branches grouped, edges fanned out, ink spread evenly (straight flow for layered). |
| Check   | `check.rs`                | Recompute every property from the finished layout. Any error stops output.                                                                                                                                                                                                              |
| Emit    | `emit.rs`                 | SVG, PNG (via resvg, same font files as measurement), and TikZ with absolute positions; none re-flows text.                                                                                                                                                                             |

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
   = note: 7pt is the default font floor; a `font` statement can lower it
   = note: drop any one of them and the rest can be satisfied (within the search budget)
```

## Options

`-o FILE` picks the format by extension: `.svg` (default, `<input>.svg`) is page-sized; `.png` is cropped to the drawing plus `--margin` of padding, at `--scale PX` pixels per point (default 2). `--tikz FILE` also writes a standalone LaTeX file.

`--page a0..a4 | WxH[pt|mm]`, `--portrait`, `--margin PT`, `--min-font PT` (hard floor; also raises the legibility floor of the font trade-off, default 16pt per meter of page diagonal), `--max-font PT`, `--wrap EM`, `--budget CHECKS` (default 20M per probe), `--force` (write output despite checker errors), `--draft` (coarser font search, one font, short polish: 2 to 6 times faster for editing). `--theme light|dark|mono`, `--font-family NAME`. Flags override settings in the file. `CMAPC_DEBUG=1` traces each probe.

## Theming

Every option has a default; leave all of them out and the output is the `light` theme in DejaVu Serif.

```
theme dark                                    # preset: light (default), dark, mono
theme dark background=#102030 link=#FFD54F    # preset plus overrides
theme palette="#1565C0 #C62828 #2E7D32"       # branch colors, cycled; base preset is light
font 7pt..40pt family="Inter"                 # measured and drawn in this family

style warm color=#EF6C00 fill=#FFE0B2 text=#3E2723
node a "Alpha" .warm
```

| Setting | Where | Default (`light`) | Controls |
| --- | --- | --- | --- |
| `background` | `theme` | `#FFFFFF` | page, box interiors, label backing |
| `text` | `theme` | `#111111` | text in boxes; root outline |
| `edge` | `theme` | `#555555` | layered arrows |
| `link` | `theme` | `#777777` | dashed cross links |
| `label` | `theme` | `#555555` | edge label text |
| `root` | `theme` | `#EEEEEE` | radial root circle fill |
| `palette` | `theme` | 8 colors | branch colors, quoted, space or comma separated |
| `family` | `font` | DejaVu Serif | body and title font; must be installed (`fc-list : family`); fallbacks still cover missing glyphs |
| `color` | `node`, `style` | branch color | box border and tint, circle fill, branch lines |
| `fill` | `node`, `style` | tint of `color` | solid fill instead of the tint |
| `text` | `node`, `style` | theme `text`, or black/white by contrast on filled shapes | text color |

`dark` suits screens; `mono` prints in grayscale. `--theme` swaps the preset but keeps the file's overrides. Colors are `#RGB` or `#RRGGBB`. The font family changes measured widths, so the solver may pick a different font size and layout.

## Tests

`cargo test --release`: unit tests for the lexer, parser, semantic checks, geometry, the solver (queens, pigeonhole, branch and bound, spatial index equivalence), layering, and QuickXplain; end-to-end runs of every example (zero errors, a warning baseline, a time budget, and two concurrent builds that must match byte for byte), unsat cores, static contradictions, and parse errors.

## Docs

- `docs/LANGUAGE.md`: grammar, semantics, relations, diagnostics.
- `docs/SOLVER.md`: the engine, the diagram encoding, unsat cores, and what to do when a build fails.
- `docs/PLANS.md`: what comes next.
- `docs/PROBLEMS.md`: known problems; delete entries when fixed, add new criticism after.
- `docs/PRIOR_ART.md`: related tools.
- `docs/SESSION_RETRO.md`: the hand-made map that motivated this.

## Examples

| File | Shows |
| --- | --- |
| `math4200.cmap`, `math4200-v2.cmap` | real course notes: a large radial map with bodies, math markup, and cross links |
| `earth_science.cmap` | radial map with styles, labeled links, `require` and `prefer` |
| `compiler.cmap` | layered flowchart (down) with a row alignment and links |
| `cell_biology.cmap` | layered flowchart (right) |

`tests/fixtures/` holds inputs that fail or strain the solver on purpose.
