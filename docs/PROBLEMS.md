# Known problems

Open problems found by review, most serious first. Measured 2026-10-29 on the `language` branch (ThinkPad X280, release build).

**Maintaining this file.** When a problem is fixed, delete its entry; git history keeps the record, so do not mark it done. After that, add any new criticism found along the way, ranked into the list by severity. Keep each entry to the symptom, how to reproduce it, and the likely fix.

## Speed

Builds take 8 to 11 s for 20 to 40 nodes (math4200-v2 8.5 s, astronomy 9.0 s, earth_science 10.8 s), too slow for an edit loop and for the planned watch mode. Costs stack: binary-search probes with four plans each, three trade-off fonts, 150k annealing moves, then an exhaustive route search. Options: start the font search from an area estimate, skip trade-off fonts when the merit gap is small, scale annealing moves with node count, and add a draft mode. The figure in `PLANS.md` (6 to 8 s) is stale.

## Output quality

### Links cross and tunnel through boxes

In `earth_science.cmap` dashed links pass through boxes (W0201 x4), cross or run together (W0207), a pair runs side by side between "Climate feedback" and "Surface storage", the "reservoir" label floats away from its link, and a dashed stub leaves "Precipitation" toward nothing. Warnings are now routine (earth_science 10, calculus 8), so they no longer signal anything. Obstacle-avoiding routing (already in `PLANS.md`) is the main fix.

### Greedy wrapping leaves one-word lines

Bodies end in lone words ("storage", "snow", "radiation", "energy", "moisture"). `wrap` in `layout.rs` is greedy; minimum-raggedness wrapping (a small dynamic program over break points) costs little and needs no change to measurement.

## Structure

### `layout.rs` does too much

1260 lines covering measurement, relations, scene building, CSP setup, annealing, QuickXplain, and the solve driver. `solve()` alone mixes nested closures, scoped threads, and borrowed temporaries. Split into measurement, relations, scene, polish, and explain modules. This is also step one of "split the core from the strategy" in `PLANS.md`.

### Examples do not match the real-use goal

Eleven new science examples repeat the same few patterns with different topics, and the README defends them ("intentionally text-authored rather than copied images"). `stress_test.cmap` and `complex_constraints.cmap` are test fixtures that fail by design and belong in `tests/fixtures/`. Keep three or four examples that each show a distinct feature.

## Tests

The e2e tests build only `math4200.cmap` plus inline sources; fourteen example files are never built by `cargo test` (`builds_are_byte_identical` covers only `calculus.cmap`). Add: every example builds with zero errors and no more warnings than a recorded baseline; a per-example time budget (catches speed regressions).

`builds_are_byte_identical` is probabilistic: with the `BTreeMap` fix reverted it failed two runs in three. It relies on per-process hash seeds differing. A stronger check would build every example twice, which costs time until builds get faster.

## Docs

- README says "end-to-end runs for both examples"; there are seventeen.
- `PLANS.md` says overlap checks are "currently O(n) per candidate", but the README and `SOLVER.md` describe a spatial index.
- `PLANS.md` speed figure is stale (see Speed).
- The Cargo `description` mentions only radial layout.

## Minor

- The solver line in the build log is not reproducible (physics: 29512490 then 29504943 checks). `attempt` adds the stats of rotation threads that a lower rotation cancelled mid-search, and how far they get depends on timing. Count only the winner and the plans before it.
- Clippy: 6 warnings (3 collapsible `if`, 2 index loops, 1 complex type).
- Ten `unwrap()` calls in non-test code; the ones in `metrics.rs` font loading panic on systems without the expected fonts instead of reporting a diagnostic.
- E0110 says "at 6pt" without pointing at the `font` range that set the floor.
- E0008 on a chain (`require a left-of b left-of c left-of a`) draws one underline per step, and adjacent steps share a node, so the labels overlap and stack on separate rows. Underline only the relation word of each step.
- `--scale` has no upper bound: `--scale 600` on an A4 page tries to allocate 720 GB and aborts. Cap the pixel count and report a diagnostic.
- Language gaps: no `group` blocks, no per-edge style (color, width, arrowhead).
