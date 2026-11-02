# AGENTS.md

Known weaknesses of fitd and the work to fix each. Ordered roughly by payoff per effort. Language reference: `docs/LANGUAGE.md`; options: `docs/OPTIONS.md`; pipeline and benchmarks: `docs/INTERNALS.md`; authoring diagrams: `skills/fitd/SKILL.md`.

Ground rules for any fix: `cargo test --release` passes, every example still builds with zero errors and no new warnings against the baseline in `tests/e2e.rs`, the two-concurrent-builds byte-identity test still holds, and `scripts/bench.py --check bench/baseline.tsv` passes (rewrite the baseline with `--write` only when a change is meant to cost more checks, and say why).

## 1. Import covers a D2 subset

`fitd import` (`src/import.rs`) reads DOT, Mermaid flowcharts and mind maps, and D2. Clusters, subgraphs, and containers carry over as nested groups, and D2 `classes` carry their label, shape, fill, and stroke to objects. A D2 connection to a container is drawn to the container's first member, and top-level D2 `vars` substitute `${name}` in labels. Dropped: D2 globs, layers, classes on connections, edge references (`(a -> b)[0]`), and `vars` inside containers; DOT HTML-like labels keep only `<br>`.

No work planned; extend only when a real import needs it.
