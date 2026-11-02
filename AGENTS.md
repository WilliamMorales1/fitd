# AGENTS.md

Known weaknesses of fitd and the work to fix each. Ordered roughly by payoff per effort. Language reference: `docs/LANGUAGE.md`; options: `docs/OPTIONS.md`; pipeline and benchmarks: `docs/INTERNALS.md`; authoring diagrams: `skills/fitd/SKILL.md`.

Ground rules for any fix: `cargo test --release` passes, every example still builds with zero errors and no new warnings against the baseline in `tests/e2e.rs`, the two-concurrent-builds byte-identity test still holds, and `scripts/bench.py --check bench/baseline.tsv` passes (rewrite the baseline with `--write` only when a change is meant to cost more checks, and say why).

## 1. macOS and Windows CI has never run

`.github/workflows/ci.yml` runs the full test suite on Linux, macOS, and Windows, but no run has happened. Windows is the risk: the atomic `write` in `src/main.rs` renames over an existing file (with a plain-write fallback when a viewer holds it open), `--watch` polls modification times, and `src/lsp.rs` maps `file:///C:/...` URIs by hand. `fitd_cpu` in `tests/e2e.rs` falls back to wall time off Unix. `.gitattributes` pins LF so fixtures read the same everywhere. None of this has been compiled for Windows or macOS.

Work:

- Push and fix whatever fails.

## 2. `fitd split` drops statements that span pages

`src/split.rs` writes an overview plus one page per top-level branch, all including a shared settings file. A cross link between branches, a `require` relating them, or a group across them has no page and is left out with a warning; `earth_science.fitd` loses its water-cycle edges this way.

Work:

- Draw a cross-page edge on each endpoint's page to a small stub of the other end that names its page.
- Split a group or constraint into the part each page can keep, when the relation allows it.

## 3. Editor support beyond VS Code

`editors/vscode/` has a TextMate grammar only.

Work:

- tree-sitter grammar for Helix and Neovim.

## 4. Import covers flowcharts only

`fitd import` (`src/import.rs`) reads DOT and Mermaid flowcharts. D2, Mermaid mind maps, and DOT HTML-like labels beyond `<br>` are not handled; nested clusters are flattened with a warning, since groups cannot nest.

Work:

- Mermaid `mindmap` to `layout radial`.
- D2 import.

## 5. Name is hard to search for

"fitd" reads as a typo for "fitted". Low priority; only matters if the project is published.

Work: if renamed, change the crate name, binary, file extension, skill name, editor extension, and docs together in one commit, and keep `.fitd` accepted as an alias.
