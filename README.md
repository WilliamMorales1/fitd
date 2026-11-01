# fitd

A small diagram language with a constraint solver for layout. Write nodes, edges, and layout constraints in a plain text file; fitd finds the largest font that fits a fixed page, satisfies every `require`, trades off every `prefer`, then verifies the result before writing SVG, PNG, or TikZ.

```
title "How fitd compiles a diagram"
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

![examples/compiler.fitd rendered to PNG](docs/compiler.png)

```fish
cargo build --release
./target/release/fitd examples/compiler.fitd --tikz out/compiler.tex
./target/release/fitd examples/compiler.fitd -o docs/compiler.png
./target/release/fitd examples/group_theory.fitd --page 2296x1524pt
./target/release/fitd explain E0110
./target/release/fitd guide          # the language on one page
```

## For AI agents

`skills/fitd/SKILL.md` is an agent skill with the workflow below, layout choice, and the fix loop; copy the `skills/fitd` folder into an agent's skills directory (for Claude Code, `~/.claude/skills/`).

An agent can draw a described diagram with two commands and a few hundred tokens of context:

1. `fitd guide` prints the whole language on one page, with a concept map and a flowchart example: about 2.2 KB (roughly 600 tokens), against 8 KB for `LANGUAGE.md`. An e2e test builds both examples and caps the size.
2. Write and build in one command, reading the source from stdin:

```fish
printf '%s\n' 'node leaf "Leaf"' 'node sun "Light"' 'node sugar "Glucose"' 'leaf -> sun, sugar' \
    | fitd - -o out/leaf.png --short
```

`--short` prints one line per error or warning (`<stdin>:2:6: error[E0001]: unknown node ...`, plus the other lines involved and the fix hint) and a one-line summary on success, so each mistake in a fix loop costs one line instead of a rustc-style block. Exit status 0 means the file was written.

Measured with pi and `gemma4:cloud`, given only a one-sentence request that names `fitd guide`: a flowchart and a concept map each built on the first try, in 2 tool calls and about 4.7k tokens total.

## Pipeline

| Stage   | File                      | What it does                                                                                                                                                                                                                                                                            |
| ------- | ------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Lex     | `lex.rs`                  | Tokens with byte spans; newlines are tokens (one statement per line).                                                                                                                                                                                                                   |
| Parse   | `parse.rs`, `ast.rs`      | Recursive descent; a bad line is reported and skipped, so later errors still show.                                                                                                                                                                                                      |
| Analyze | `sema.rs`                 | Two passes: declarations, then references. Styles, typed values, units (`pt mm cm in em`), "did you mean" suggestions, and a static check that hard orderings and alignments do not contradict (union-find plus cycle search).                                                          |
| Plan    | `radial.rs`, `layered.rs` | Measure every box with real font metrics, propose candidate positions and structural constraints.                                                                                                                                                                                       |
| Solve   | `csp.rs`, `layout/`       | Small CSP engine: forward checking with conflict-directed backjumping, a spatial index for overlap pruning, branch and bound for `prefer`. Binary search on the font, then a trade-off: the top font and 5% and 10% smaller ones are polished in parallel and the best readability-plus-font merit wins. With nodes fixed, each dashed link picks a bend or a detour around boxes. If nothing fits, QuickXplain extracts a minimal set of conflicting `require`s. |
| Polish  | `quality.rs`, `layout/`   | Simulated annealing moves nodes to improve readability without breaking any constraint: fewer link crossings, related nodes close, branches grouped, edges fanned out, ink spread evenly (straight flow for layered). |
| Check   | `check.rs`                | Recompute every property from the finished layout. Any error stops output.                                                                                                                                                                                                              |
| Emit    | `emit.rs`                 | SVG, PNG (via resvg, same font files as measurement), and TikZ with absolute positions; none re-flows text.                                                                                                                                                                             |

References:

- P. Prosser. Hybrid algorithms for the constraint satisfaction problem. *Computational Intelligence* 9(3), 1993. (conflict-directed backjumping)
- U. Junker. QuickXplain: preferred explanations and relaxations for over-constrained problems. AAAI 2004. (unsat cores)
- K. Sugiyama, S. Tagawa, M. Toda. Methods for visual understanding of hierarchical system structures. *IEEE SMC* 11(2), 1981. (layered layout)

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

## Options

An input of `-` reads the source from stdin and then needs `-o`. `-o FILE` picks the format by extension: `.svg` (default, `<input>.svg`) is page-sized; `.png` is cropped to the drawing plus `--margin` of padding, at `--scale PX` pixels per point (default 2). `--tikz FILE` also writes a standalone LaTeX file.

`--page a0..a4 | WxH[pt|mm]`, `--portrait`/`--landscape` (fix the orientation; otherwise a named paper may be turned when that clears warnings), `--bands N` (most bands a long flowchart wraps into; 1 never wraps), `--margin PT`, `--min-font PT` (hard floor; also raises the legibility floor of the font trade-off, default 16pt per meter of page diagonal), `--max-font PT`, `--wrap EM`, `--budget CHECKS` (default 20M per probe), `--force` (write output despite checker errors), `--short` (one line per diagnostic, for agents and editors), `--draft` (coarser font search, one font, short polish: 2 to 6 times faster for editing). `--theme light|dark|mono`, `--font-family NAME`. Flags override settings in the file. `FITD_DEBUG=1` traces each probe.

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
| `color` | edge (`a -> b color=#hex` or `.style`) | branch color, theme `edge` or `link` | that edge's stroke and arrowhead |
| `color` | `group` | theme `edge` | frame border, tint, and title |
| `fill` | `node`, `style` | tint of `color` | solid fill instead of the tint |
| `text` | `node`, `style` | theme `text`, or black/white by contrast on filled shapes | text color |

`dark` suits screens; `mono` prints in grayscale. `--theme` swaps the preset but keeps the file's overrides. Colors are `#RGB` or `#RRGGBB`. The font family changes measured widths, so the solver may pick a different font size and layout.

## Tests

`cargo test --release`: unit tests for the lexer, parser, semantic checks, geometry, the solver (queens, pigeonhole, branch and bound, spatial index equivalence), layering, and QuickXplain; end-to-end runs of every example (zero errors, a warning baseline, a CPU-time budget, and two concurrent builds that must match byte for byte), unsat cores, static contradictions, and parse errors. A fuzzer feeds mutated sources to the front end for 2 seconds; `FITD_FUZZ_SECS=1800 cargo test --release fuzz` runs it longer.

## Docs

- `docs/LANGUAGE.md`: grammar, semantics, relations, diagnostics.
- `skills/fitd/SKILL.md`: agent skill for drawing diagrams with fitd.

## Examples

| File | Shows |
| --- | --- |
| `group_theory.fitd` | group theory notes with no settings at all: a large radial map on the defaults, with bodies, math markup, and cross links |
| `french_history.fitd` | real course notes: a layered timeline (down) on a portrait page, colored `group` frames per period, styles, a colored link, `require ... at` and `prefer ... at` |
| `earth_science.fitd` | radial map: `root=`, theme overrides (`root=`, `link=`), styles on nodes and edges, a node `fill`, labeled links, `require` and `prefer` |
| `compiler.fitd` | layered flowchart (down): `theme dark`, `page ... margin=`, `shape=circle` terminals, two groups, a row alignment, `gap=`, `near ... within=` |
| `cell_biology.fitd` | layered flowchart (right) that wraps into two bands: a group frame, circle molecules, a solid `fill` (text turns white), labeled arrows, an edge `color=` |

`tests/fixtures/` holds inputs that fail or strain the solver on purpose.
