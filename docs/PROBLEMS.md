# Known problems

Open problems found by review, most serious first. Measured 2026-10-30 and 2026-10-31 on the `language` branch (ThinkPad X280, release build; other jobs were running, so times are upper bounds).

**Maintaining this file.** When a problem is fixed, delete its entry; git history keeps the record, so do not mark it done. After that, add any new criticism found along the way, ranked into the list by severity. Keep each entry to the symptom, how to reproduce it, and the likely fix.

## Output quality

### Branch order and page orientation are left to the user

Two of the three real-use diagrams (2026-10-31) needed a manual layout fix to reach 0 warnings. `HNRS2000/Justice/justice-map.cmap` had 2 W0207 with `freedom -- nozick, kant, rawls`; writing `kant` first (toward the welfare branch that `kant ..> welfare` points at) cleared both. `CSC4343/TransformerNotes/generative.cmap` had W0201 and W0207 on landscape a4 and none on portrait. The fixes are mechanical: a link pulls its endpoint's subtree toward the other end, and orientation is one bit. Likely fix: order each node's children so cross links point toward their targets (barycenter on link ends), and try the other orientation in the trade-off when the page was not given explicitly. This is the same missing piece as the `at` entry below: branch order chosen by merit, not by source order.

### Radial `prefer ... at` cannot turn a branch

In `earth_science.cmap`, `prefer condensation at top`, `prefer precipitation at bottom`, and `prefer balance near human` are unmet (3 W0205). With the source branch order, none of condensation's or precipitation's candidate spots meets its `at` (measured: 0 of about 2000 at every probe rotation). `at top` means the middle third of the top row, and the two nodes sit about 70 degrees apart in branch order, so no rotation helps; trying 90, 180, and 270 degree turns in the trade-off was measured and never won.

Reordering the root's branches (scored on wedge centers over all probe rotations) gives about 35% satisfying spots. But the parent `atmosphere` takes its cheapest spot first, and forward checking then removes every satisfying spot of `condensation`. Putting satisfying spots first therefore changed nothing, and the reordering added 7 link crossings (12 warnings). Likely fix: pull a constrained node's ancestors toward the target too (order their spots by the descendant's `at`), and pick the branch order by real merit (full solve per order) rather than a point estimate, so link crossings count.

### Links forced to cross

`earth_science.cmap` keeps 2 W0207 (`evaporation -> condensation` with `human -> storage`, `runoff -> storage` with `climate -> ice`). With nodes fixed, the routing search is exhaustive over bends and detours, so the crossings come from node placement: polish scores each link at its best plain bend, not its detour, so it does not see that moving one node would separate them. Fix: score detours in polish, or offer detours that also avoid other links. A second detour around the other side of the first box was tried and did not help.

### Layered flows cannot wrap

`cell_biology.cmap` (13 nodes, `dir=right`, a2) builds at 11.2pt: nine layers of 16em boxes plus arrow gaps must fit across the page, every font above about 11.4pt is statically infeasible, and the drawing is a strip using a third of the page height. A long chain needs a snake or multi-row flow (or an automatic `dir` and wrap choice); today the only remedies are `dir=down`, `portrait`, or a narrower `wrap`. When even the floor does not fit, the error now names the layer count and the length needed.

### Links ignore tree edges and graze boxes

In `justice-map.cmap` the `macintyre ..> nozick` link crosses the `freedom -- rawls` edge and passes within a hair of Aristotle's corner, and the `talents are morally arbitrary` label touches Nozick's border. None of these is a warning: W0207 counts only link-link crossings, W0201 only paths inside a box, and label clearance is 0.15em. Fix: count link-edge crossings in routing and the readability penalty, and keep links and labels a small margin (about 0.4em) off boxes.

### Detours kink and straight chains drift

`generative.cmap` routes `vae ..> stable` with a detour that has a sharp S-kink beside `Pixel groups as tokens`; a smooth curve through the same corridor would read better (smooth the via polyline, or prefer a larger bow). In `transformer.cmap` (layered, one chain of nine nodes) the column steps right by about 70pt after `Embedding + position` and back before `Next token`, although every box fits on one axis. The cause is likely the `sample ..> embed` back link pushing nodes away; the straight-flow term in polish should hold a single chain on one axis.

## Speed

## Minor

- Layered fan-in: edges into one node all end at its center line, so in an agent's bill flowchart four arrows into `Veto` met in one point, and the labels `2/3 Vote in both Chambers` and `Failed` sat edge to edge. Fix: spread arrival points along the target's side, and give labels the same 0.4em margin suggested above.
- Radial layout does not put group members first in the variable order (layered does), so a group in a tight radial map may still search slowly; no example shows it.
- Group frames are not scored by polish or routing: edges, links, and edge labels may cross a frame or its title. Fix if real diagrams show it: a `LabelClear`-style constraint for titles, and frame crossings in the readability penalty.
- `--draft` gives up on its first font probe at a twentieth of the budget instead of a quarter, so on a hard map it may settle on a smaller font than a full build would.
- The default build of `math4200.cmap` on 2296x1524pt picks 17.8pt for fewer crossings although 19.8pt fits (`--min-font 19.8` gets it). By design (font is one criterion of the merit), but the only hint is the trade-off line in the log.
