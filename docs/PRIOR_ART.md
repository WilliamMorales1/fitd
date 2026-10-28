# Prior art

These are tools that overlap with cmapc, grouped by approach. The notes say what each does well and what it lacks compared with cmapc's goal: exact text measurement, a maximal font on a fixed page, hard readability constraints, and an independent verifier. Check the details against current docs before relying on them.

## Graph layout engines

- **Graphviz** (`dot`, `twopi`, `neato`, `fdp`, `sfdp`, `circo`). `twopi` is a radial layout close to what this map needed. `neato`/`sfdp` have `overlap=` removal modes (for example `prism`, `scale`) and `sep` for padding. Node sizes come from label size at a *fixed* font; nothing maximizes the font for a page, and there is no checker. It was not installed in the session that motivated this project (`pacman -S graphviz`).
- **ELK (Eclipse Layout Kernel) / elkjs.** Layered, radial, force, "rectpacking", and many layout options, with good port and label handling. Configured with many options rather than stated constraints; no font objective.
- **OGDF** (C++) and **Tulip**: large algorithm libraries (planarization, orthogonal, energy-based).
- **yFiles** (commercial): strong organic/radial/hierarchic layouts with label placement.
- **Rust crates**: `petgraph` (graph structures and algorithms), `layout-rs` (a Graphviz-like layered layout in Rust). Neither solves readability constraints.

## Constraint-based diagramming (closest relatives)

- **Adaptagrams**: `libcola` (constrained stress majorization with separation, alignment, and non-overlap constraints), `libvpsc` (rectangle overlap removal), `libavoid` (object-avoiding connector routing). **WebCoLa** is the JavaScript port. Closest in spirit; it optimizes stress under constraints, not font size.
- **Penrose** (CMU): describe *what* a diagram means (Domain/Substance/Style programs) and a numerical optimizer satisfies constraints and objectives. Very close to the "spec, then solve" idea, aimed at math diagrams.
- **Bluefish**: a research framework for relation-based diagrams (alignment, spacing, and containment as first-class relations).
- **SetCoLa**: a high-level constraint language compiled to WebCoLa.
- **Cytoscape.js** layouts `fcose` (supports placement constraints) and `cola`.

## Text-to-diagram languages

- **Mermaid**: the closest in spirit for syntax (one statement per line, `-->` edges). Layout is delegated to dagre/ELK with no user constraints beyond direction.
- **D2** (Terrastruct): a newer diagram language with `near` placement, grid diagrams, and pluggable layout engines (dagre, ELK, the commercial TALA). Constraints are positional hints, not a solved constraint set; no unsat explanation.
- **Graphviz DOT**: `rank=same` and `rankdir` are the only layout constraints.

## Mind map and concept map tools

- **CmapTools** (IHMC): the classic concept-map editor from Novak's concept-mapping tradition, with labeled propositions on links. Manual layout.
- **Mermaid `mindmap`**, **PlantUML mindmap**, **markmap** (Markdown to an interactive mind map). Text-to-map pipelines with automatic layout, little control over density or font, and no verification.
- **XMind**, **Freeplane**/**FreeMind**: interactive editors with auto-arrange.
- **TikZ `mindmap` library**: pretty, but angles are chosen by hand (the original MATH 4200 map started here). The **TikZ graph drawing library** (`\usegdlibrary{trees,force,layered}`) does automatic layout but needs LuaLaTeX, so it doesn't run under tectonic's XeTeX.

## Solvers that could replace the hand-written CSP

- **OR-Tools CP-SAT**: `AddNoOverlap2D` over interval variables models region ownership directly. Optional intervals give alternative wrap widths.
- **MiniZinc** (with Gecode, Chuffed, or OR-Tools backends): a declarative model of the same constraints; good for prototyping objective variants.
- **Z3 / OptiMathSAT**: SMT with optimization; workable for small maps, slower for geometry.

## Relevant research areas

- Rectangle overlap removal (VPSC, PRISM).
- Map labeling / label placement (NP-hard in general; greedy plus local search is standard).
- Radial tree drawing and balloon layouts.
- Conflict-directed backjumping (Prosser) and nogood learning in CSP search.
- Readability metrics for node-link diagrams (crossings, angular resolution, edge length uniformity).

## Gap cmapc targets

None of the above combine (1) real font metrics at layout time, (2) a fixed canvas with the *font size* as the objective, (3) hard "no edge through a box" and planarity constraints, and (4) a separate checker that refuses to emit a broken layout and points at source lines.
