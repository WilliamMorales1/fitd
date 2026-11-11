# AGENTS.md

Notes for people and coding agents working on fitd. It covers the goals every change is judged against, which common diagram types fitd covers, and the planned work for the types it cannot draw yet.

## Goals

These are the design goals from the [README](README.md#design-goals). Each one has a check.

| Goal | How to check it |
| --- | --- |
| Minimal code gives a professional result | A gallery file should be under ~30 lines and use no `require`/`prefer` unless the figure type needs them |
| Sane defaults, optional overrides | New attributes need a default that renders well when left out |
| Deterministic | Same source gives byte-identical output; `committed_docs_match_a_fresh_build` enforces this |
| One second at most for reasonable inputs | `for f in examples/gallery/*.fitd; do time fitd $f -o /tmp/x.svg; done` (release build) |
| Expansive, reliable, easy standard library | Every common figure type below has a `std/` module and a gallery file; `gallery_and_every_std_module_compile_cleanly` passes |
| Readable source | Templates take lists as one string (`"a; b; c"`, `"90 5 5"`), not one call per cell |
| Minimal text in diagrams | Stdlib templates add no captions or decoration beyond what the figure type requires |

## Working rules

- Run `cargo test --release` before finishing. The e2e suite takes about a minute.
- Commit when a part of a plan is done and tests pass, one commit per logical change. Don't wait to batch several plans into one commit.
- A change to layout or rendering can make committed renders stale. Rebuild them, then look at the changed images before committing:
  ```sh
  for f in docs/*/*.fitd docs/pipeline.fitd; do fitd "$f" -o "${f%.fitd}.png"; done
  for f in examples/gallery/*.fitd; do fitd "$f" -o examples/gallery/out/$(basename "${f%.fitd}").png; done
  ```
- `src/guide.txt` is capped at 2900 bytes (tested). Trim wording to make room; never drop syntax.
- Stdlib modules:
  - A module lives at `src/std/NAME.fitd` and is registered in `MODULES` in `src/stdlib.rs`.
  - The first comment line is the summary printed by `fitd std`.
  - Style names are global, so prefix every style and template with the module name (`uml_class`, `er_rel`).
  - Every exported `def` gets a one-line comment above it in the form `# name(args): what it draws`.
  - Each module needs a gallery file and a row in the `docs/language.md` Includes table.
- Macro helpers that recurse should accumulate a string and emit once, as `grid_cells` does. Partial lines cannot be emitted.

## What diagrams people use

The coverage table is built from these surveys of figures in papers and practice:

1. Lohse, Biolsi, Walker & Rueter, "A Classification of Visual Representations", *Communications of the ACM* 37(12), 1994. Sorting studies give six clusters: graphs, tables, maps, diagrams, networks, icons.
2. Lee, West & Howe, "Viziometrics: Analyzing Visual Information in the Scientific Literature", [arXiv:1605.04951](https://arxiv.org/abs/1605.04951). This study classifies 8M+ PubMed figures into diagrams, plots, photos, tables, and equations, and finds that diagram-heavy papers are cited more.
3. Jobin, Mondal & Jawahar, "DocFigure: A Dataset for Scientific Document Figure Classification", ICDAR Workshops 2019. It has 33K figures in 28 classes, including flow chart, block diagram, tree, Venn, table, confusion matrix, heat map, algorithm, and many plot types.
4. Karishma et al., "ACL-Fig: A Dataset for Scientific Figure Classification", [arXiv:2301.12293](https://arxiv.org/abs/2301.12293). It has 19 classes for NLP papers: trees, architecture diagrams, neural networks, graphs, confusion matrices, tables, algorithms, text/grammar examples, Venn, and plots.
5. Nickerson et al., "The Meaning of Arrows: Diagrams and Other Facets in System Sciences Literature", HICSS. It covers 983 diagrams from 495 papers:
   - The common genres are architecture (212), research models (101), flow (80), and hybrid sequence (25).
   - Layer stacks and stage/cycle diagrams also recur.
   - Among UML types, class diagrams come first, then sequence.
   - Only 37% of diagrams fit a tight genre, and 19% are unclassifiable.
6. Reggio, Leotta, Ricca & Clerissi, "What are the used UML diagrams? A Preliminary Survey", EESSMod 2013. Class, activity, sequence, state machine, and use case diagrams dominate.

## Coverage

Each row lists the figure type, the surveys (by number above) that name it, and what fitd has for it.

| Figure type | Surveys | fitd | Module | Gallery |
| --- | --- | --- | --- | --- |
| Flowchart / activity | 3, 5, 6 | yes | `std/flow` | `01-flowchart` |
| Architecture / block diagram | 3, 4, 5 | yes | `std/arch` | `02-architecture` |
| Neural network | 4 | yes | `std/nn` | `03-neural-network` |
| Org chart / hierarchy | 1 | yes, `order=source` keeps sibling order | `std/tree` | `04-org-chart` |
| Syntax / parse tree | 4 | yes, `order=source` keeps word order | `std/tree` | `05-syntax-tree` |
| Decision tree | 3, 4 | yes | `std/tree` | `06-decision-tree` |
| Network / graph | 1, 4 | yes | core | `07-network` |
| Confusion matrix | 3, 4 | yes | `std/grid` | `08-confusion-matrix` |
| Table | 1, 2, 3, 4 | yes, small tables | `std/grid` | `09-table` |
| Research model | 5 | yes | `std/model` | `10-research-model` |
| Data flow | 5 | yes | `std/arch` | `11-data-flow` |
| Layer stack | 5 | yes | `std/arch` | `12-layer-stack` |
| Stage / cycle | 5 | yes, drawn as a chain plus a back link | core | `13-cycle` |
| UML class | 5, 6 | yes, relations told apart by colour | `std/uml` | `14-uml-class` |
| UML state machine | 6 | yes | `std/uml` | `15-state-machine` |
| Entity-relationship | 5 | yes, cardinality as labels | `std/er` | `16-er` |
| Timeline | 1 | yes | `std/time` | `17-timeline` |
| Gantt chart | 1 | yes | `std/grid` | `18-gantt` |
| Mind / concept map | 1 | yes | core (radial) | `19-mind-map` |
| 2x2 matrix (SWOT) | 1 | yes, as a figure grid | `std/grid` | `20-swot` |
| Heat map | 3 | yes | `std/grid` | `08`, `grid_shade` |
| Unit bar chart | 3, 4 | yes, small counts only | `std/grid` | `21-bar-chart` |
| Commutative diagram | (math typology) | partial: square drawn as diamond | core | `22-commutative` |
| Fishbone / Ishikawa | 1 | yes, as a tree into the effect | core | `23-fishbone` |
| Venn / Euler | 3, 4, 5 | **no** | planned | |
| UML sequence | 5, 6 | **no** | planned | |
| Swimlane activity | 6 | **no** | planned | |
| Algorithm / pseudocode | 3, 4 | **no** | planned | |
| UML use case | 6 | partial: no actor glyph | planned | |
| Dependency arcs (NLP) | 4 | **no** | planned, low priority | |
| Sankey / flow widths | 1 | **no** | out of scope for now | |

### Out of scope

Data plots belong to plotting libraries (matplotlib, ggplot2, pgfplots, Vega-Lite), which fit them to data axes; fitd places boxes by constraints.

- Plots: line, scatter, bar (beyond unit counts), histogram, box, pie, area, polar, radar, bubble, Pareto, surface, contour, vector field.
- Images: photos, natural and medical images, masks, sketches, screenshots, 3D renders.
- Maps and geographic figures.
- Word clouds.
- Circuit schematics, which need typed symbols and orthogonal wiring; CircuiTikZ covers them.

These cover a large share of the figures in surveys 2 to 4. fitd should not grow a plotting engine. A figure that mixes a plot and a diagram should put the plot in as an image once image nodes exist (plan 10).

## Plans

The plans are ordered by how many common figure types each one unlocks for the effort. Each plan gives the change, where it goes, and when it is done.

### 1. Ordered siblings (done)

`layout layered order=source` makes `layered::order` (src/layered.rs) run one downward pass: roots keep declaration order, and each node sorts by its parents' mean position, then by the index of its first incoming edge. `Kind::Before` constraints then hold that row order through the solver and polish. Gallery 04, 05, and 23 use it, and `source_order_keeps_children_in_edge_order` checks the x order of leaves.

- Left out: the per-group `ordered` flag. A group draws a frame, so it is a poor switch for ordering alone. Add one only if a figure needs source order in one part and crossing reduction in another.

### 2. Node shapes: diamond, and per-node shape on templates

Flowchart decisions and ER relationships use diamonds.

- Change:
  - Add `Shape::Diamond` (src/graph.rs).
  - Measure it as a box whose inscribed text rect is half the width and height.
  - Clip edges at the diamond outline in emit and check.
  - Accept `shape=diamond` in style attributes (src/sema.rs).
- Then `std/flow` uses it for `decide`, and `std/er` gains `er_relation` diamonds.
- Done when the 01 and 16 gallery files use diamonds and edges end on the outline.

### 3. Edge heads and edge styles

UML needs hollow triangles (inheritance), diamonds (composition and aggregation), and dashed dependencies. ER needs crow's foot ends. Today edges accept only `color` (src/sema.rs:693), so `std/uml` tells relations apart by colour.

- Change: add `head=arrow|triangle|diamond|odiamond|crow|none`, `tail=` with the same values, and `line=solid|dashed|dotted` to edge attributes and edge styles.
  - Draw heads in emit (SVG, TikZ, PDF).
  - Heads are drawn inside the existing arrow clearance, so layout does not change.
- Then:
  - `uml_is` becomes a hollow triangle and `uml_has` a filled diamond.
  - `er_rel` gets crow's feet at the "many" end.
  - `uml_key()` and the colour coding become optional.
- Done when gallery 14 and 16 read correctly in `theme mono`.

### 4. Lane layout: swimlanes and UML sequence diagrams

Survey 6 ranks sequence diagrams second among UML types. Activity diagrams with swimlanes are common too. Both need nodes pinned to lanes, with order along the lane given by source order or by time.

- Change: add `layout lanes dir=down|right`.
  - Each `group` is a lane, and lanes sit side by side in declaration order.
  - Inside a lane, nodes stack in flow order.
  - Edges between lanes route straight across.
  - Built as a new planner beside `layered::plan`, which can reuse its layering for rank (time) and assign the cross axis from lane membership.
- A sequence diagram then sets every lane's head node as the participant and messages as edges with rank = statement order. A new `std/seq.fitd` gives `seq_actor`, `seq_msg(a, b, text)`, `seq_reply`, and draws the lifeline as a dashed frame of the lane.
- Done when gallery files `24-swimlane` and `25-sequence` exist, messages keep source order, and lanes keep declaration order.

### 5. Overlapping regions: Venn and Euler diagrams

Venn diagrams appear in surveys 3, 4, and 5. Groups cannot overlap today (E0010, src/sema.rs:902).

- Change: add `venn A, B [, C]` blocks.
  - Each set is a circle placed by a closed form: two or three circles at fixed offsets.
  - Node membership (`a in A, B`) assigns a node to a region.
  - The solver only places region members inside their region cell, with the cells precomputed as polygons.
  - Four or more sets are out of scope (unreadable).
- `std/venn.fitd` is then thin: styles for the region fills.
- Done when a gallery Venn of 2 and 3 sets places items in the right regions and passes `check`.

### 6. Preformatted bodies: algorithm and pseudocode blocks

DocFigure and ACL-Fig both have an "algorithm" class. Body lines are trimmed, which loses indentation.

- Change: add `style NAME mono=true`, or a `|>` body line that keeps leading spaces and uses a monospace font metric (src/metrics.rs needs a mono width table).
- Then `std/algo.fitd` gives `algo_block(id, title, lines)` with `;`-separated lines and keyword bolding through existing markup.
- Done when a gallery algorithm keeps 2-level indentation in SVG and PDF.

### 7. Multi-source edges

`a, b, c -> d` gives E0002. Fishbones and fan-in flows need it.

- Change: in src/parse.rs, accept a comma list before the operator and expand it to one edge per source, keeping the label on each.
- Done when `23-fishbone` uses one line and an e2e test parses `a, b -> c, d` into 4 edges.

### 8. Grid placement

Commutative squares and 2x2 node matrices need nodes on a grid. `require a above b` combined with `left-of` fails for unconnected nodes (E0110), so `22-commutative` draws a diamond.

- Change: add `at row=R col=C` to node statements (or `align grid(a, b; c, d)`). It lowers to `align row` and `align column` constraints, which the solver already supports, plus an order constraint per row and column.
- Done when `22-commutative` is a square and `20-swot` can use real nodes with bodies.

### 9. Ratio-aware folding of long chains

Timelines and state machines on `page 16:9` come out thin, and `bands=N` does not help, so the gallery uses `page 3:1`.

- Change: in `fit_ratio` (src/main.rs), try `bands = 1..=3` for layered layouts and keep the one with the largest font at the requested ratio.
- Done when `17-timeline` at `page 16:9` uses two bands with a larger font than one band.

### 10. Image nodes

Mixed figures (a plot inside an architecture diagram) are common in surveys 2 to 4.

- Change: add `~ image "path.png"` as a figure row. It is sized by aspect ratio and embedded in SVG and PDF, with TikZ using `\includegraphics`.
- This keeps plots out of scope while letting diagrams contain them.

### 11. Dependency arcs and Sankey widths (low priority)

- Dependency arcs: words on one baseline with arcs above them. This needs a new `layout arcs` that places nodes in source order on one row and draws edges as nested arcs. It is small once plan 1 exists, but the figure type is mostly NLP-specific.
- Sankey: edge width proportional to a value (`width=N`). The layout is fine, but readable Sankeys need crossing-free ribbons and node heights tied to flow, which is closer to a plotting tool. Revisit after plan 3.

### Performance

The 1-second goal held for every gallery file after two changes:

- `fit_ratio` runs the first full probe alone and only fans out when it misses.
- Annealing in polish scales with movable nodes (`POLISH_PER_NODE`) instead of a fixed 150k moves.

The slowest gallery file now takes about 0.3 s. Watch these when adding features:

- Ratio pages run up to 4 quick solves plus 1 to 6 full solves. Plan 9 multiplies that by the band count, so it should reuse the quick solve per band.
- Large docs pages (35 nodes) still spend most of their time in the CSP search, not polish.
