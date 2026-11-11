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
| UML class | 5, 6 | yes, UML heads (triangle, diamond, dashed) | `std/uml` | `14-uml-class` |
| UML state machine | 6 | yes | `std/uml` | `15-state-machine` |
| Entity-relationship | 5 | yes, relationship diamonds, cardinality labels, crow's feet | `std/er` | `16-er` |
| Timeline | 1 | yes | `std/time` | `17-timeline` |
| Gantt chart | 1 | yes | `std/grid` | `18-gantt` |
| Mind / concept map | 1 | yes | core (radial) | `19-mind-map` |
| 2x2 matrix (SWOT) | 1 | yes, as a figure grid | `std/grid` | `20-swot` |
| Heat map | 3 | yes | `std/grid` | `08`, `grid_shade` |
| Unit bar chart | 3, 4 | yes, small counts only | `std/grid` | `21-bar-chart` |
| Commutative diagram | (math typology) | partial: square drawn as diamond | core | `22-commutative` |
| Fishbone / Ishikawa | 1 | yes, as a tree into the effect | core | `23-fishbone` |
| Swimlane activity | 6 | yes | core (lanes) | `24-swimlane` |
| UML sequence | 5, 6 | yes, no activation bars or self-messages | `std/seq` | `25-sequence` |
| Venn | 3, 4, 5 | yes, up to three sets | `std/venn` | `26-venn`, `27-venn-three` |
| Euler | 3, 4, 5 | **no** | planned | |
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

### 2. Node shapes: diamond (done)

`shape=diamond` (`Shape::Diamond`, src/graph.rs) sizes the rhombus by scaling the padded content rect until every line corner fits, so it is at most twice the content box and smaller for ragged text. `inside_box` (src/layout/path.rs) tests the rhombus, so edge clipping, routing, and `check` follow the outline. SVG, PDF, TikZ, and key swatches draw it. `std/flow` uses it for `decide`, `std/er` has `er_relation(id, name, a, b, card)`, and the Mermaid `{}`, Graphviz `diamond`, and D2 `diamond` importers map to it. `diamond_edges_end_on_the_outline` checks the clipping.

### 3. Edge heads and edge styles (done)

Edges and edge styles take `head=` and `tail=` (`arrow|triangle|diamond|odiamond|crow|none`) and `line=solid|dashed|dotted` (`Head`, `Dash`, src/graph.rs). Nodes reject them with E0005. `decor` (src/emit.rs) resolves the marks at the drawn ends, swapping them when the layout draws an edge reversed. SVG uses one `<marker>` per mark and colour, TikZ uses `arrows.meta` tips, and PDF follows the SVG. Key swatches take `head=` too. Marks sit in the arrow clearance, so layout is unchanged.

- `uml_is` is a hollow triangle, `uml_has` a diamond at the whole, `uml_uses` dashed. The relation colours are gone; `uml_key()` shows the heads.
- `er_rel` and `er_relation` put a crow's foot at every end whose cardinality is not `1`.
- `edge_heads_and_dashes_reach_svg_and_tikz` checks both outputs.
- Left out: Graphviz `arrowhead=`/`style=` import.

### 4. Lane layout: swimlanes and UML sequence diagrams (done)

`layout lanes dir=down|right order=source` runs `lanes::plan` (src/lanes.rs) beside `layered::plan`. Each top-level group is a lane, then each loose node, in declaration order. Rows come from `layered::layering`, or with `order=source` from lane heads first and then one row per `align row` set in declaration order. A node alone in its lane and row sits on the lane center, and emit stretches the top-level frames to one extent along the flow.

- `lane=ID` on a node (src/sema.rs `lifelines`) hangs it on ID's lane and adds a dashed, headless edge from the previous node on that lane, which is the lifeline. E0007 rejects a node as its own lane and a head that is itself on a lane.
- An empty `shape=none` node measures as a point, so messages meet the lifeline.
- `std/seq` gives `seq_actor`, `seq_msg(step, a, b, text)`, and `seq_reply`; each message is two points and an `align row`.
- `sequence_messages_keep_source_order`, `swimlanes_keep_declaration_order`, and `lane_rejects_itself_and_nested_heads` check it.
- Left out: activation bars, self-messages, and lane-relative `group` frames inside a sequence.

### 5. Overlapping regions: Venn diagrams (done)

`layout venn` (src/venn.rs) turns every group into a set circle, so groups may overlap, and E0010 now only rejects a fourth set. `venn::shape` places one, two, or three circles by closed form, tries four center spreads, and keeps the one with the most room in the most crowded region. Titles sit outside the circles and count as obstacles. `venn::plan` offers each node grid spots inside exactly its sets' circles and outside the rest, nearest the region's middle first. The geometry is stored on `Layout`, so SVG and TikZ draw the circles and `check` gives E0108 for a node outside its region. Nodes default to `shape=none`.

- `std/venn` gives `venn_two` and `venn_three`, which take each region's items as one string. Gallery 26 and 27 use them.
- `venn_items_land_in_their_regions` and `venn_rejects_a_fourth_set` check it.
- Left out: Euler diagrams (disjoint or nested sets drawn apart or inside) and area-proportional circles.

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
