# Known problems

Open problems found by review, most serious first. Measured 2026-10-30 on the `language` branch (ThinkPad X280, release build, idle).

**Maintaining this file.** When a problem is fixed, delete its entry; git history keeps the record, so do not mark it done. After that, add any new criticism found along the way, ranked into the list by severity. Keep each entry to the symptom, how to reproduce it, and the likely fix.

## Output quality

### Links cross and tunnel through boxes

In `earth_science.cmap` dashed links pass through boxes (W0201), cross or run together (W0207), and labels float away from their link (8 warnings). Obstacle-avoiding routing (already in `PLANS.md`) is the main fix.

## Speed

Builds take 0.8 to 6.2 s (cell_biology 0.8, compiler 1.0, earth_science 2.7, math4200 3.6, math4200-v2 6.2); `--draft` takes 0.2 to 2.6 s. math4200-v2 did not improve: its time is font probes that fail and run their whole budget, plus polish, which rescores the whole layout on each of 150k moves. Next: incremental readability scoring in `polish` and cheaper per-check cost in the CSP. A box-density cap on the starting font was tried and rejected (little gain, worse penalties).

## Tests

The per-example time budgets in `tests/e2e.rs` are wall-clock, so a busy machine fails them. They are about 4 times idle time, since `cargo test` runs other builds alongside; raise them rather than delete them if they flake.

## Minor

- Language gaps: no `group` blocks, no per-edge style (color, width, arrowhead).
