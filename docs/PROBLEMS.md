# Known problems

Open problems found by review, most serious first. Measured 2026-10-29 on the `language` branch (ThinkPad X280, release build).

**Maintaining this file.** When a problem is fixed, delete its entry; git history keeps the record, so do not mark it done. After that, add any new criticism found along the way, ranked into the list by severity. Keep each entry to the symptom, how to reproduce it, and the likely fix.

## Speed

Route caching, bounding-box prefilters, and a bend-pair table for link routing cut earth_science from 10.7 s to about 5.5 s with byte-identical output. Radial builds still take 5 to 8 s (astronomy-sized maps, math4200-v2): font probes that fail run until their budget is gone, and polish rescores the whole layout on every one of 150k moves. Next: incremental readability scoring in `polish`, cheaper per-check cost in the CSP. A box-density cap on the starting font was tried and rejected (little gain, worse penalties). `--draft` (coarse font search, one font, short polish) exists but was measured slower once, because a larger font made routing harder; re-measure on an idle machine. The `PLANS.md` figure (6 to 8 s) is stale.

## Tests

`every_example_builds_within_baseline` has loose time budgets (10 to 30 s) set while another job loaded the machine. Re-measure idle and tighten to about 2x.

## Output quality

### Links cross and tunnel through boxes

In `earth_science.cmap` dashed links pass through boxes (W0201), cross or run together (W0207), and labels float away from their link. Obstacle-avoiding routing (already in `PLANS.md`) is the main fix.

## Docs

- README "Example gallery" lists eleven deleted examples and defends them; rewrite for the five kept. README test line says "both examples".
- `PLANS.md` says overlap checks are "currently O(n) per candidate", but the README and `SOLVER.md` describe a spatial index.
- The Cargo `description` mentions only radial layout.
- `--draft` is undocumented.

## Minor

- A diagnostic's primary label message ends its row, so chain steps after it still go on a second row. Rendering labels in column order, message last, would fit one row.
- Language gaps: no `group` blocks, no per-edge style (color, width, arrowhead).
