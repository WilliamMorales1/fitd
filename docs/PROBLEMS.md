# Known problems

Open problems found by review, most serious first. Measured 2026-10-30 and 2026-10-31 on the `language` branch (ThinkPad X280, release build; other jobs were running, so times are upper bounds).

**Maintaining this file.** When a problem is fixed, delete its entry; git history keeps the record, so do not mark it done. After that, add any new criticism found along the way, ranked into the list by severity. Keep each entry to the symptom, how to reproduce it, and the likely fix.

## Output quality

### Links forced to cross in `earth_science`

2 W0207 remain (`evaporation -> condensation` with `climate -> ice`, `condensation -> precipitation` with `human -> storage`). With nodes fixed they are topological: `human -> storage` splits the map left to right, and `condensation` and `precipitation` sit on either side. Detours around the other link's path were tried and did not help. Fix: score link-link crossings at real routes in polish (today other links count as straight segments), so node placement can separate them.

## Speed

### Retries double the build when warnings remain

When the first layout has warnings, `main::retry` solves again with reordered radial branches and, if the page orientation is open, on the turned page (in parallel). Idle X280, 2026-10-31: `earth_science` 3.8 s (was about 2 s), `compiler` 2.1 s; clean examples are unchanged, and the largest, `math4200-v2`, takes 2.2 s. Narrowing the variants' font search to 0.6 to 1.4 times the first font was tried and was slower (probes near the edge ran their whole cap). Fix: share the first run's font probes with the variants, or stop a variant once its font search cannot beat the first merit.

## Minor

- Short layered arrows leave no room for a label: in an agent's bill flowchart, `2/3 Vote in both Chambers` sits between `Veto` and `Veto Override` with almost no margin (the layer gap is 2em). Fix if real diagrams show it: widen the gap of a layer whose arrows carry labels.
- A wrapped (snake) flow runs every other band backward, so a `require a right-of b` along the flow can make wrapping infeasible and the build falls back to one long band (seen with `cell_biology` before its constraint was changed to `below`).
- Edges from a parent outside a group still cross its frame (unavoidable when the parent is outside); only labels avoid frames.
