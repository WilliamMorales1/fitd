---
name: fitd
description: Draw diagrams with the fitd CLI (.fitd text in, SVG/PNG/PDF/TikZ out): concept and mind maps, flowcharts, architecture and data flow, trees, UML class/state/sequence, ER, timelines, Gantt, tables, heat maps, Venn, swimlanes, pseudocode. Use when asked to make or fix a diagram or a .fitd file, or to convert Graphviz, Mermaid, or D2.
---

# fitd

fitd compiles a small text language into a diagram. A constraint solver places every box and picks the largest font that fits the page, so the source never holds coordinates or sizes.

## Workflow

1. Run `fitd guide`. It prints the whole language on one page (about 600 tokens). Treat it as the syntax reference.
2. Write the source and build in one command, reading from stdin:

```sh
fitd - -o out/leaf.png --short <<'EOF'
title "Photosynthesis"
leaf "Leaf"
  sun "Light"
  | photons absorbed by chlorophyll
  sugar "Glucose"
sun ..> sugar "drives"
EOF
```

3. Exit status 0 means the file was written. Otherwise fix and rebuild (see Fix loop).

If the user wants the source kept, write it to `NAME.fitd` and build with `fitd NAME.fitd -o NAME.png --short`. For a common figure type, start from the stdlib module and its gallery file (see Standard library) instead of building by hand.

## Commands

| Command | Use |
| --- | --- |
| `fitd FILE.fitd -o out.svg\|.png\|.pdf` | Build. `-` reads stdin. |
| `fitd a.fitd b.fitd -o book.pdf` | One PDF, a page per input; `--grid 2x2` puts several per page. |
| `fitd guide` | Whole language on one page. |
| `fitd std [NAME]` | List bundled modules, or print one. |
| `fitd explain CODE` | Meaning and fix for a diagnostic code. |
| `fitd import FILE.dot\|.mmd\|.d2 [-o out.fitd]` | Translate Graphviz, Mermaid, or D2. Unmapped attributes are listed in a warning. |
| `fitd split FILE.fitd [-o DIR]` | Overview page plus one page per top-level branch; shared settings go in an included file. |
| `fitd lsp` | Language server (diagnostics, hover, rename, completion, code actions). Editor support: `editors/vscode`, `editors/tree-sitter-fitd`. |

## Language features

Syntax details are in `fitd guide`; `docs/language.md` is the full reference. What exists:

- Nodes: `ID "Title"`, `| body line` under it (leading spaces indent, `**bold**`), `wrap=24em`, `shape=box|circle|diamond|none`, `stack=N` (N copies, e.g. repeated layers), `color= fill= text=`, `row= col=` (grid placement: squares, 2x2 matrices, commutative diagrams), `place=aside` (layered: note strip beside the flow).
- Children by indent: parent links to each indented node (`--` radial, `->` layered). Under a node, `-> x "label"` is an edge from it; a lone declared id reopens it.
- Figure rows: `~ # %60 . _ LBL "two words" #X*3` draws a grid of cells below the node text (strips, masks, heat maps, tensor bars, unit bars). `~ image "plot.png" width=10em` embeds PNG/JPEG, for plots inside diagrams.
- Edges: `->` flow, `--` undirected, `..>` dashed cross link that does not shape the layout, `..` undirected cross link. Chains and fans: `a, b -> c, d "label" -> e`. Attributes `color=`, `line=solid|dashed|dotted`, `head=` and `tail=` (`arrow|triangle|diamond|odiamond|crow|none`).
- Groups: `group G "Title" a, b color=#hex` draws a frame; list inner members in the outer group to nest. A named group is an edge end (`x -> G`). `times=N` draws a dashed frame titled `Nx`.
- Keys: `key "Label" color=... line=dashed head=...` adds a legend row at the bottom; `page ... keysize=20pt`.
- Layouts: `radial root=ID` (default, mind maps), `layered dir=down|right bands=N order=source` (flows, trees; `order=source` keeps sibling order), `lanes dir= order=source` (each top-level group is a swimlane; `lane=ID` hangs a node on ID's lifeline, for sequence diagrams), `venn` (groups are set circles, up to three).
- Page and font: `page a0..a4|WxHmm|16:9 portrait|landscape margin=1cm` (a ratio picks the smallest page of that shape), `font 8pt..30pt wrap=16em family="DejaVu Sans"`, `title "Text"`.
- Theme: `theme paper|light|dark|mono` plus overrides `background text edge link label root palette gradient`. `paper` is black text on white with Okabe-Ito borders and light tints.
- Styles: `style NAME attrs` declares; a line with only `NAME` opens a block, and every node, edge, group, and key indented under it takes the style (no `end`). Blocks nest.
- Constraints: `require REL` (hard, E0110 if impossible) and `prefer REL weight=2` (soft, W0205 if unmet). REL: `a left-of|right-of|above|below b gap=2em` (chainable), `a near b within=4em`, `a at top-left|top|...|center|...|bottom-right`, `align row(a, b)|column(..)|top(..)|bottom(..)|left(..)|right(..)`.
- Includes: `include "file.fitd"`, `include "MOD"` (bundled), `include "user/LIB"` (`LIB.fitd` beside the file or in a `FITD_PATH` directory; call as `LIB.name ARGS`).
- Macros: `for i in 1..n` / `for i, x in a, b, c` ... `end`, `if EXPR` ... `end`, `def NAME(a, b)` ... `end` called as `NAME x "two words"` on its own line, `${expr}` in any line, `count(s, ";")`, `part(s, ";", i)`. Recursive templates should build a string and emit once. Errors on generated lines are reported at the call.

## Choosing a layout

- Concept map or mind map (one topic branching out): the default, `layout radial`. Pick the center with `root=ID`.
- Process, pipeline, timeline, decision flow, tree: `layout layered dir=down` or `dir=right`. Long flows wrap into bands; `bands=1` forbids wrapping. Trees (org chart, syntax tree): add `order=source` to keep children in edge order.
- Swimlanes and UML sequence: `layout lanes order=source`, or `include "seq"`.
- Venn: `layout venn`, or `include "venn"`.
- Cycle: a chain plus one back link. Fishbone: a tree into the effect. 2x2 or square: nodes with `row=`/`col=`.
- `->` edges build the hierarchy or flow. Use `..>` for "relates to" links that should not change structure.

## Repetition

When many nodes or edges share one style, write the style once as a block: `style step color=#0072B2`, then a line `step` and the lines indented under it. Declaration order does not change the layout (except with `order=source`), so put each style's nodes in one block. Module styles open blocks too (`nn.block`); so do templates (`tree.unit`, one call per indented row; a deeper row nests under the row above). Node lines need `node` only for keyword ids (`node font`). When many nodes share one shape (numbered steps, layers, recursion trees), use `for` or `def` instead of copying lines.

## Standard library

`fitd std` lists modules, `fitd std NAME` prints one. Include by name and call templates with the module prefix: `include "grid"`, then `grid.row "a; b; c"`. Bare words are text, digits are numbers, quotes group words; lists go in one string (text split on `;`, numbers on spaces).

| Module | Draws | Gallery |
| --- | --- | --- |
| `flow` | Flowchart roles `io step decide done drop`, `flow.key` | 01 |
| `arch` | `arch.client service store queue ext layers`, `arch.zone` frames, `arch.key` | 02, 11, 12 |
| `nn` | `nn.input layer step out mask mask_rows mask_pad_rows note add`, styles `nn.block nn.data` | 03 |
| `tree` | `tree.cat word` (syntax), `tree.q leaf` (decision), `tree.unit kids` (org) | 04, 05, 06 |
| `grid` | `grid.head row title_row` (tables), `grid.heat shade` (heat maps, confusion), `grid.bar` (unit bars), `grid.task scale` (Gantt), `grid.layers`, `grid.quad` (2x2) | 08, 09, 18, 20, 21 |
| `model` | Research models: `model.iv dv med mod ctl hyp key` | 10 |
| `uml` | `uml.class iface state start final`, relations `uml.is has uses`, `uml.key` | 14, 15 |
| `er` | `er.entity weak rel relation`, cardinality and crow's feet | 16 |
| `time` | `time.event mile chain` | 17 |
| `seq` | `seq.actor msg reply` for `layout lanes` | 25 |
| `venn` | `venn.two three items set` for `layout venn` | 26, 27 |
| `algo` | `algo.block` pseudocode, `;` lines, extra spaces indent, `**` keywords | 28 |
| `palette` | Okabe-Ito colors | |

Gallery sources: `examples/gallery/NN-*.fitd` (also 07 network, 13 cycle, 19 mind map, 22 commutative, 23 fishbone, 24 swimlane, 29 image). `examples/showcase/` mixes modules in one figure (arch+time+flow, nn+grid, uml+arch, tree+flow). Not drawable yet: Euler diagrams, UML use-case actor glyph, dependency arcs, Sankey. Out of scope: data plots (embed as `~ image`), maps, photos, circuits.

## Start minimal

Only node lines and edges are required. Leave out `page`, `font`, `theme`, and constraints at first: the defaults (A1 page, paper theme, font searched) usually work. Add `require` only when the user asks for a specific placement, because each one can make the layout unsatisfiable. Prefer `prefer` when the placement is a wish, not a rule.

## Fix loop

`--short` prints one line per diagnostic. With `sun ..> sugr` on line 6 of the example above:

```
<stdin>:6:9: error[E0001]: unknown node `sugr`; did you mean `sugar`?
```

- Fix the first error only, then rebuild. Later errors are often caused by the first.
- `fitd explain CODE` prints the meaning and fix for any code.
- E0110 (constraints cannot all hold): the message lists a minimal conflicting set. Drop one, turn it into `prefer`, or use a larger `page`.
- E0110 in a radial map with `left-of`/`right-of`/`above`/`below` between nodes on different branches: each branch stays within its own wedge, so the relation may be impossible. Switch to `layout layered` (with `dir=down`, two nodes in one layer can never be `above` one another).
- E0016: edge from a group to itself, a member, or a nested group, or a group edge under `venn`/`lanes`.
- No layout fits (page too small): lower the font floor (`font 6pt..30pt` or `--min-font`), narrow text with `font ... wrap=12em` (or `--wrap`), use a larger page, or follow the printed help.
- Font smaller than expected: `--budget 100000000` (default 20M checks per probe) sometimes finds a larger font. A layered chain is limited by its number of layers; try `dir=right` or `page ... portrait`.
- Warnings (W codes) do not block output. W0205: a `prefer` was not met. W0208: a `place=aside` node stayed in the flow.
- `--force` writes output despite checker errors, for a quick look only.

## Useful flags

| Flag | Use |
| --- | --- |
| `-o FILE.svg` / `.png` / `.pdf` | Format by extension. PNG is cropped to the drawing; SVG and PDF keep the page; PDF text is selectable. |
| `--tikz FILE.tex` | Also write standalone LaTeX. |
| `--short` | One line per diagnostic. Always use it. |
| `--draft` | 2 to 6 times faster; use while iterating, drop it for the final build. |
| `--watch` | Rebuild on save (draft first, full after 1.5 s idle). One file input. |
| `--stable FILE.lock` | Keep nodes and font in place when editing a map someone already saw. |
| `--page a0..a4`, `WxHmm`, or `16:9` | Page size; smaller pages give smaller fonts. |
| `--portrait` / `--landscape`, `--margin PT` | Orientation and margin. |
| `--min-font PT`, `--max-font PT`, `--wrap EM` | Font search bounds and text width. |
| `--bands N` | Max bands for layered layouts. |
| `--theme paper\|light\|dark\|mono`, `--font-family NAME` | Color preset (`mono` for grayscale print) and installed font. |
| `--scale PX` | PNG pixels per point (default 2). |
| `--budget N`, `--expand-budget N` | Layout search and macro expansion limits. |

Flags override settings in the file. `FITD_DEBUG=1` prints solver probes.

## Checking the result

Open the PNG (read the image) and check that labels are legible and nothing important is missing. The success line reports the chosen font and page, e.g. `body font 48.0pt on 2384x1684pt; 0 errors, 0 warnings`. A very small font means too much content: split the diagram (`fitd split`), shorten bodies, or use a larger page.

The chosen font is often not the largest that fits. fitd also lays out at 5% and 10% below it and keeps the one that reads best (fewer crossings, related nodes closer), as long as it stays above a legibility floor of 16pt per meter of page diagonal. A font a little under the maximum is intended; `--min-font` raises the floor.

## Examples

`examples/` in the fitd repo holds worked diagrams with renders in `examples/out/`: `fitd_language.fitd` (every statement, relation, and macro form once, with an `include`), `coffee.fitd` and `group_theory.fitd` (radial maps; palette, undirected edges, math markup), `compiler.fitd` (dark layered flow with groups, styles, chained constraints), and `library/` (a `user/LIB` library). Read one when the user asks for a feature the guide only names.
