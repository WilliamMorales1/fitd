# AGENTS.md

Known weaknesses of fitd and the work to fix each. Ordered roughly by payoff per effort. Language reference: `docs/LANGUAGE.md`; options: `docs/OPTIONS.md`; pipeline and benchmarks: `docs/INTERNALS.md`; authoring diagrams: `skills/fitd/SKILL.md`.

Ground rules for any fix: `cargo test --release` passes, every example still builds with zero errors and no new warnings against the baseline in `tests/e2e.rs`, the two-concurrent-builds byte-identity test still holds, and `scripts/bench.py --check bench/baseline.tsv` passes (rewrite the baseline with `--write` only when a change is meant to cost more checks, and say why).

## 1. Import flattens nesting and covers a D2 subset

`fitd import` (`src/import.rs`) reads DOT, Mermaid flowcharts and mind maps, and D2. Groups cannot nest, so nested clusters and containers are flattened with a warning, and a D2 connection to a container is dropped. D2 `vars`, `classes`, globs, layers, and edge references (`(a -> b)[0]`) are dropped; DOT HTML-like labels keep only `<br>`.

Work:

- Nested groups in the language, then carry nesting over on import.
- D2 `classes`: apply their `style.fill` and `style.stroke` to the objects that use them.
