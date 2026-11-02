# AGENTS.md

Known weaknesses of fitd and the work to fix each. Ordered roughly by payoff per effort. Language reference: `docs/LANGUAGE.md`; options: `docs/OPTIONS.md`; pipeline and benchmarks: `docs/INTERNALS.md`; authoring diagrams: `skills/fitd/SKILL.md`.

Ground rules for any fix: `cargo test --release` passes, every example still builds with zero errors and no new warnings against the baseline in `tests/e2e.rs`, the two-concurrent-builds byte-identity test still holds, and `scripts/bench.py --check bench/baseline.tsv` passes (rewrite the baseline with `--write` only when a change is meant to cost more checks, and say why).

## 1. macOS and Windows CI has never run

`.github/workflows/ci.yml` runs the full test suite on Linux, macOS, and Windows, but no run has happened. Windows is the risk: the atomic `write` in `src/main.rs` renames over an existing file (with a plain-write fallback when a viewer holds it open), `--watch` polls modification times, and `src/lsp.rs` maps `file:///C:/...` URIs by hand. `fitd_cpu` in `tests/e2e.rs` falls back to wall time off Unix. `.gitattributes` pins LF so fixtures read the same everywhere. None of this has been compiled for Windows or macOS.

Work:

- Push and fix whatever fails.

## 2. `--watch` and the LSP block on long builds

`watch` in `src/main.rs` runs each build in the loop, so an edit during a slow full build (`earth_science.fitd` takes about 4 s wall) is seen only when it ends. The LSP starts a child process per save and never cancels an older one.

Work:

- Run builds on a worker thread with a cancel flag (the solver already takes `AtomicBool` stops) and restart on change.
- Kill the previous layout child in `src/lsp.rs` when a newer save arrives.

## 3. LSP ignores names declared in included files

`src/lsp.rs` reports front-end diagnostics only for the open file and resolves definitions and completions from its own text. A node declared in an `include`d file has no go-to-definition and is not offered for completion; layout diagnostics that point into an included file are dropped.

Work:

- Build `declared` from the spliced `Source` and return locations in the included file's URI.
- Publish diagnostics for included files under their own URIs.

## 4. No multi-page output

A map too big for one page fails with E0110 or NoRoom; fitd now names the largest branch to split out (`layout::largest_branch`), but the user does the split by hand.

Work:

- One page per top-level branch, with a stub node on the overview that names its page.
- Reuse `include` so each page can also be built alone.

## 5. Editor support beyond VS Code

`editors/vscode/` has a TextMate grammar only.

Work:

- tree-sitter grammar for Helix and Neovim.

## 6. Import covers flowcharts only

`fitd import` (`src/import.rs`) reads DOT and Mermaid flowcharts. D2, Mermaid mind maps, and DOT HTML-like labels beyond `<br>` are not handled; nested clusters are flattened with a warning, since groups cannot nest.

Work:

- Mermaid `mindmap` to `layout radial`.
- D2 import.

## 7. Name is hard to search for

"fitd" reads as a typo for "fitted". Low priority; only matters if the project is published.

Work: if renamed, change the crate name, binary, file extension, skill name, editor extension, and docs together in one commit, and keep `.fitd` accepted as an alias.
