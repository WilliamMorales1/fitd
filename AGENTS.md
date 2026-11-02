# AGENTS.md

Known weaknesses of fitd and the work to fix each. Ordered roughly by payoff per effort. Language reference: `docs/LANGUAGE.md`; options: `docs/OPTIONS.md`; pipeline and benchmarks: `docs/INTERNALS.md`; authoring diagrams: `skills/fitd/SKILL.md`.

Ground rules for any fix: `cargo test --release` passes, every example still builds with zero errors and no new warnings against the baseline in `tests/e2e.rs`, the two-concurrent-builds byte-identity test still holds, and `scripts/bench.py --check bench/baseline.tsv` passes (rewrite the baseline with `--write` only when a change is meant to cost more checks, and say why).

## 1. `fitd split` drops statements that span pages

`src/split.rs` writes an overview plus one page per top-level branch, all including a shared settings file. A cross link between branches, a `require` relating them, or a group across them has no page and is left out with a warning; `earth_science.fitd` loses its water-cycle edges this way.

Work:

- Draw a cross-page edge on each endpoint's page to a small stub of the other end that names its page.
- Split a group or constraint into the part each page can keep, when the relation allows it.

## 2. Helix setup is untested

`editors/tree-sitter-fitd/` has the grammar, corpus tests, and CI; the Neovim setup in its README was run headless (highlights and LSP diagnostics). The Helix `languages.toml` and `queries/helix/highlights.scm` were written from Helix's docs but never loaded in Helix.

Work:

- Install Helix, follow the README, and fix what breaks.

## 3. Import flattens nesting and covers a D2 subset

`fitd import` (`src/import.rs`) reads DOT, Mermaid flowcharts and mind maps, and D2. Groups cannot nest, so nested clusters and containers are flattened with a warning, and a D2 connection to a container is dropped. D2 `vars`, `classes`, globs, layers, and edge references (`(a -> b)[0]`) are dropped; DOT HTML-like labels keep only `<br>`.

Work:

- Nested groups in the language, then carry nesting over on import.
- D2 `classes`: apply their `style.fill` and `style.stroke` to the objects that use them.

## 4. Name is hard to search for

"fitd" reads as a typo for "fitted". Low priority; only matters if the project is published.

Work: if renamed, change the crate name, binary, file extension, skill name, editor extension, and docs together in one commit, and keep `.fitd` accepted as an alias.
