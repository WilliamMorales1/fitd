# Known problems

Open problems found by review, most serious first. Measured 2026-10-30 and 2026-10-31 on the `language` branch (ThinkPad X280, release build; other jobs were running, so times are upper bounds).

**Maintaining this file.** When a problem is fixed, delete its entry; git history keeps the record, so do not mark it done. After that, add any new criticism found along the way, ranked into the list by severity. Keep each entry to the symptom, how to reproduce it, and the likely fix.

## Output quality

### Two layered groups that share a layer fail on any page

In `cell_biology.fitd`, making `glycolysis -> anaerobic` a flow edge and adding `group "Cytosol" glycolysis, anaerobic` next to the existing Mitochondrion group gives E0110 on the Mitochondrion group, even on `--page a0`. `anaerobic` and `pyruvate` land in the same layer, one in each frame, and the frames cannot be separated across the flow; each group alone builds. The example keeps fermentation as a link and only the Mitochondrion group. Fix: let layered candidates split a shared layer between frames (order the layer by group), or report which groups collide instead of a one-group core.

### Open orientation keeps a tiny landscape page for tall flows

An open page (`page a2`, no orientation) is solved landscape first and turned only when warnings remain (`main::retry`). A clean but cramped layout is kept: `french_history.fitd` (17 layers, `dir=down`) builds at 8.2pt landscape with 0 warnings, but 11.7pt on `page a2 portrait`. Reproduce: drop `portrait` from its `page` line. The example states `portrait` to work around it. Fix: start `dir=down` flows portrait and `dir=right` landscape, or solve both orientations when the page is open and keep the better merit.

### Link labels and edges cover group titles

In `french_history.fitd` the labels of `palsgrave ..> vaugelas` and `dubellay ..> malherbe` sit on the top edge of the "Français moderne" frame and hide its title, and the edge `francien -> relatin` runs through the "Moyen français" title. The checker does not treat frame titles as obstacles, so neither warns. Fix: add each frame's title box to the label placement obstacles, and penalize edges through it in polish.

### Links forced to cross in `earth_science`

2 W0207 remain (`evaporation -> condensation` and `human -> storage` each with `climate -> ice`, since edge labels are drawn on radial tree edges). Some crossing is forced: the graph of all 23 edges is not planar (a Kuratowski subgraph runs through `ground`, `human`, `storage`, `evaporation`, `condensation`, `precipitation`, `infiltration`, `runoff`, `ocean`, `balance`), and removing any one of `condensation -> precipitation`, `precipitation -> runoff`, `runoff -> ocean`, `storage -> evaporation`, `human ..> ground`, `human ..> storage` makes it planar, so the minimum is one crossing, which could fall on a tree edge (no warning). Tried 2026-10-31 without a lasting gain: all 24 root branch orders (best 1 W0207, but a different tree), and adding chord interleavings of links to `radial::arrange` (1 W0207 before labels were drawn, 2 after). Fix: score link-link crossings at real routes in polish (today other links count as straight segments), or let `arrange` see routed links.

## Speed

### Retries double the build when warnings remain

When the first layout has warnings, `main::retry` solves again with reordered radial branches and, if the page orientation is open, on the turned page (in parallel). The variants now start with a draft's probe cap (`Config::first_cap`), which cut `earth_science` from 105M to 81M checks with identical output. Idle X280, 2026-10-31: `earth_science` 3.8 s (first build 1.9 s), `compiler` 2.2 s; clean examples are unchanged, and the largest, `group_theory`, takes 2.1 s. Narrowing the variants' font search to 0.6 to 1.4 times the first font was tried and was slower (probes near the edge ran their whole cap). Fix: stop a variant once its font search cannot beat the first merit, or skip its second trade-off round.

## Minor

- A link that attaches a node to the radial tree (`human ..> ground "pumping"` when `human` has no tree edge) is drawn as a solid hierarchy line without its arrowhead; the label is kept, and a note says the node was attached. Fix if a real diagram reads wrong: keep the link's dash and arrow on that tree edge.
- Won't fix: a wrapped (snake) flow runs every other band backward, so a `require a right-of b` along the flow can make wrapping infeasible and the build falls back to one long band (seen with `cell_biology` before its constraint was changed to `below`). The fallback still builds, and `bands=1` states it.
- Won't fix: edges from a parent outside a group cross its frame, which is unavoidable when the parent is outside; only labels avoid frames.
