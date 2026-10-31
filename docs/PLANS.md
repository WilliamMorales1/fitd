# Plans: beyond concept maps

## Status (2026-10-30)

Priority is real use: anything kept only for demonstration is removed.

Done: language front end, FC-CBJ engine with spatial-index pruning and branch and bound, `require`/`prefer`, radial and layered strategies, QuickXplain unsat cores, arc consistency at the root for `require` orderings and alignments (chains such as `a above b above c` were reported as false unsat cores under forward checking alone), incremental readability scoring in polish, and links that detour around boxes. Tried and removed because they did not help real diagrams: alternative search algorithms (BT, MAC, dom/wdeg ordering, nogood learning, restarts, min-conflicts) and the `bench` command; see git history (commit d474132) for code and measurements.

Next:

- **Branch direction for `at` preferences.** The source branch order leaves no spot that meets the preference, and a parent placed first blocks the spots that a reordering opens (see `PROBLEMS.md`).
- **Watch mode** for live diagnostics.

## Definition of done

cmapc is done when every item below holds. Each is measurable and tied to real use; anything not listed here (including sections 1 to 7 below) is v2.

1. **Real use.** Three new diagrams from actual coursework or notes, each built start to finish without changing cmapc source and without hand-editing the SVG or TikZ. A tool change forced by a diagram resets that diagram's count.
2. **Original job beaten.** The MATH 4200 map builds at 19.8pt body font or larger with 0 errors, locked in by an e2e test.
3. **Clean output.** Every example builds with 0 warnings, or each remaining warning has a documented reason it is accepted. Now: 5, all in earth_science (see `PROBLEMS.md`).
4. **Edit-loop speed.** On the largest example, idle X280: `--draft` under 1 s, full build under 3 s. Last measured under load: about 0.6 to 0.8 s and 1.6 to 2.5 s; re-measure idle.
5. **No hangs, no panics.** 30 minutes of fuzzing the lexer, parser, and sema with 0 panics. Every unsatisfiable input returns an unsat core or a static contradiction in bounded time.
6. **Language frozen.** `group` and per-edge color are implemented; per-edge width and arrowheads are cut for good (the layout decides them). `LANGUAGE.md` matches the parser, and every diagnostic code has an `explain` page.
7. **Trustworthy tests.** `cargo test --release` passes 10 runs in a row. Time budgets use CPU time or check counts instead of wall-clock time.
8. **Fresh-clone install.** `cargo install --path .` and the README quickstart work from a clean clone.
9. **Agent use.** `cmapc guide` fits in 2.5 KB and its examples build (e2e test); an agent given only the guide and a one-sentence description writes a concept map and a flowchart that build on the first or second try, through stdin and `--short`.
10. **No open problems.** The Output quality and Speed sections of `PROBLEMS.md` are empty; anything left under Minor is marked won't-fix.

Real-use log (item 1). 2026-10-31, no cmapc source change for any of them: `CSC4343/TransformerNotes/transformer.cmap` (decoder flowchart, 0 warnings first build), `CSC4343/TransformerNotes/generative.cmap` (generative-model map; 2 warnings on landscape, 0 after switching to portrait), `HNRS2000/Justice/justice-map.cmap` (Sandel's three approaches; 2 warnings, 0 after reordering one branch line). Visual flaws the checker misses are in `PROBLEMS.md`.

Agent log (item 9). 2026-10-31, pi with `ollama/gemma4:cloud`, bash tool only, no context files, prompt naming only `cmapc guide`: a bill-to-law flowchart and a WWI causes concept map each built on the first try in 2 tool calls (about 4.3k input and 0.4k output tokens). An earlier run of the flowchart failed twice with "no layout fits" before passing; the first failure was a fixed `font 12pt`, and the second was a cmapc bug (layer gaps that exactly fill the page put the last layer off it by rounding), both fixed. The local `qwen3.5:4b` model is too slow on the X280 CPU; run local-model evals on Colab.

Not criteria: zero crossings on every input (NP-hard in general; the warning budget covers it), beating Graphviz in general (the benchmark is real diagrams), and watch mode.

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
