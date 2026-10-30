# Plans: beyond concept maps

## Status (2026-10-28)

Priority is real use: anything kept only for demonstration is removed.

Done: language front end, FC-CBJ engine with spatial-index pruning and branch and bound, `require`/`prefer`, radial and layered strategies, QuickXplain unsat cores. Tried and removed because they did not help real diagrams: alternative search algorithms (BT, MAC, dom/wdeg ordering, nogood learning, restarts, min-conflicts) and the `bench` command; see git history (commit d474132) for code and measurements.

Next:

- **Speed.** About 6 to 8 s for a 39-node map (four plans per probe and three trade-off fonts, each in parallel). Fewer font probes (start the binary search from a size estimate), and cheaper `Clear`/`NoCross` pruning.
- **Obstacle-avoiding link routes.** Links choose among 8 Bezier bends, which in dense maps all cross some box (W0201). Routing around boxes (shortest path on a visibility graph, then smoothing) would clear them without moving nodes.
- **Groups**: `group` blocks with a containment constraint.
- **Watch mode** for live diagnostics.

## 1. Split the core from the strategy

Define an intermediate representation (a "scene"):

- **Region**: a box or circle that owns part of the canvas. It holds measured text lines and a role (the hierarchy level).
- **Connector**: borrows a corridor between regions (straight, curved, orthogonal).
- **Constraint**: ownership (no overlap), corridor (no pass-through), planarity, containment, ordering (A left of B, A outside B), alignment, and reading order.
- **Objective**: maximize the minimum font, minimize area, match an aspect ratio, minimize total connector length, keep positions stable.

The checker works only on the scene, so every strategy gets the same diagnostics for free. Strategies only generate variables, domains, and value orderings.

## 2. Strategies to add

| Strategy | Output | Variables | Notes |
|---|---|---|---|
| Radial (current) | concept map, mind map | angle and radius per node | per-depth ideal rings |
| Layered | flowchart, dependency DAG, proof outline | layer and in-layer slot | Sugiyama layering, then the CSP for slots and the font |
| Column flow | cheatsheet, poster, study guide | column, order, wrap width | 2D packing of text blocks; binary search on the font |
| Grid / matrix | comparison tables, theorem/definition grids | cell spans | row/column constraints, equal heights |
| Paginated | slide decks, handouts | page per subtree | each page solved separately; consistent font across pages |
| Timeline | proofs as sequences, history | x-order, lanes | lane assignment as graph coloring |

**Column flow** would have done the MATH 4200 cheatsheet in one step. Today pandoc picks a fixed 10pt font, so the page count is whatever falls out. Inverting that means fixing the pages and maximizing the font, which is the same binary search used here.

## 3. Solver upgrades

- **Wrap width as a variable.** A node can choose its wrap width (for example 14, 18, 24 em), trading height for width. That is a bigger domain but often a much bigger font.
- **Hybrid search.** CBJ for feasibility, then min-conflicts or simulated annealing to polish soft objectives (balance, symmetry, edge length).
- **CP-SAT backend.** OR-Tools CP-SAT has `AddNoOverlap2D`, which is exactly region ownership. Positions become integer variables on a grid. Good for exact optima on small maps.
- **Incremental re-layout.** Seed from the previous solution and penalize movement, so small content edits give small visual diffs. This is the "stable layout" property, useful for living study notes.
- **Speed.** A spatial hash for overlap checks (currently O(n) per candidate), precomputed angular sectors per node, and parallel binary-search probes.

## 4. Front ends

The `.cmap` DSL (now), Markdown outlines (headings become depth, bullets become bodies), Graphviz DOT, JSON/YAML, and Mermaid `mindmap`. Each should keep the source location of every node, since diagnostics point at source lines.

## 5. Back ends

SVG, PNG via `resvg`, and TikZ (now), PDF via TikZ or `svg2pdf`, Typst, HTML with hover detail, PPTX for slide strategies. All of them consume the same solved scene, so nothing reflows.

## 6. Readability model

- Physical size: `--print-width` turns canvas points into real millimeters and enforces a minimum *printed* font size.
- Color: WCAG contrast checks for text on fills. Palette assignment as a CSP so adjacent branches differ in hue.
- Line length, number of lines per box, edge length variance, and crossing counts for cross links (currently only warnings).
- Text shaping with `rustybuzz` for kerning and ligatures; hyphenation for narrow wraps.

## 7. Checker as a product

- `cmapc check scene.json`: verify layouts made by *other* tools (Graphviz, hand-made TikZ) with the same diagnostics.
- LSP or editor integration: diagnostics inline in the `.cmap` source.
- `--explain` pages for each code (started).
