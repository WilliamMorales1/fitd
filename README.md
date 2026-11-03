# fitd

A small diagram language with a constraint solver for layout. Write nodes, edges, and layout constraints in a plain text file; fitd finds the largest font that fits a fixed page, satisfies every `require`, trades off every `prefer`, then verifies the result before writing SVG, PNG, PDF, or TikZ.

```
title "How fitd compiles a diagram"
layout layered dir=right
page 297x210mm margin=1cm
theme dark link=#D29922

style io color=#1F883D shape=circle

node src "Source .fitd" .io
node parse "Parser"
node csp "CSP search"
| largest font that fits
node bnb "Branch and bound"
node core "QuickXplain"
| minimal unsat core
node checker "Checker" color=#F85149
node out "SVG, PNG, TikZ" .io

src -> parse -> csp -> bnb, core
bnb -> checker -> out
core ..> src "points back at source"

group "Solver" csp, bnb, core color=#AB7DF7

require bnb above core gap=2em
prefer out at right weight=2
```

![docs/pipeline.fitd rendered to PNG](docs/pipeline.png)

That is the whole file. [examples/](examples/) holds larger ones, each rendered in the format that suits it (PNG, SVG, or PDF in [examples/out/](examples/out/)), including [the full pipeline](examples/compiler.fitd). Longer docs: [language](docs/language.md), [options](docs/options.md), [internals](docs/internals.md).

## Dependencies

Rust 1.85+ (edition 2024) and Cargo. Crates, fetched by Cargo: `resvg` (SVG/PNG rendering), `ttf-parser` (font metrics), `serde_json` (LSP and lock files). DejaVu fonts are compiled in; a custom `family` needs fontconfig (`fc-match`). Compiling `--tikz` output needs a LaTeX install with TikZ.

## Usage

```fish
cargo build --release
./target/release/fitd docs/pipeline.fitd -o pipeline.png
./target/release/fitd docs/pipeline.fitd -o pipeline.pdf     # selectable text
./target/release/fitd examples/*.fitd -o all.pdf                 # one page per diagram
./target/release/fitd a.fitd b.fitd c.fitd d.fitd --grid 2x2 --page a1 -o sheet.pdf  # four to a page
./target/release/fitd examples/compiler.fitd --tikz out/compiler.tex
./target/release/fitd guide          # the language on one page
./target/release/fitd explain E0110  # explain an error code
./target/release/fitd examples/compiler.fitd --watch   # rebuild on save: draft, then full
./target/release/fitd split examples/group_theory.fitd  # overview plus one page per branch
./target/release/fitd examples/fib_calls.fitd -o fib.png  # macros: a recursive template
```

Editors: `fitd lsp` is a language server (front-end diagnostics as you type, full layout on save, cancelled by a newer save, go-to-definition and completion for node and style names, including those from included files, whose diagnostics are published under their own names; names built by macros, such as `s${i}`, have no definition to jump to). `editors/vscode/` holds a TextMate grammar packaged as a VS Code extension; copy or symlink it into `~/.vscode/extensions/fitd` for highlighting. `editors/tree-sitter-fitd/` is a tree-sitter grammar with setup for Neovim and Helix.

Tests: `cargo test --release`.

## Docs

- [Language](docs/language.md): grammar, semantics, relations, diagnostics.
- [Options and theming](docs/options.md): command-line flags, colors, fonts.
- [Agent skill](skills/fitd/SKILL.md): drawing diagrams with fitd from an AI agent.
- [Internals](docs/internals.md): pipeline, solver, references, tests.

## Compared with DOT, Mermaid, and D2

Those languages have far wider adoption and tooling; `fitd import` translates Graphviz (`.dot`), Mermaid flowcharts and mind maps (`.mmd`), and D2 (`.d2`): nodes, labels, edges, and clusters, subgraphs, or containers (as nested `group`s), D2 `classes` and `vars`, with a mind map's indentation becoming a radial tree; it lists whatever has no equivalent. What fitd adds:

- Hard constraints: `require a left-of b`, `align row(...)`, `a at top-left` hold in the output or the build fails.
- Weighted preferences: `prefer ... weight=N` trades soft wishes against each other explicitly.
- A fixed page, filled: the solver maximizes the font that fits the page you name, instead of growing the canvas.
- Verified output: an independent checker re-derives overlaps, crossings, clipping, and overflow from the finished drawing.
- Macros: `for` loops, `if`, recursive `def`/`use` templates, and `${expr}` interpolation expand before parsing, so numbered steps, repeated components, and recursion trees take a few lines ([semester](examples/semester.fitd), [neural_net](examples/neural_net.fitd), [fib_calls](examples/fib_calls.fitd)). Errors on generated lines point at the template line.
- Unsat cores at source lines: when `require`s conflict, a minimal conflicting set is reported, rustc style, at the lines that wrote them.

Why not a layout plugin for one of them instead? D2, Mermaid (v11 layouts), and Graphviz all accept external layout engines, but each measures text before layout and renders after it. A plugin receives boxes already sized, so it cannot search for the largest font that fits the page, and the checker would verify a drawing other than the one written. D2 also rejects unknown keywords and Mermaid has no attribute syntax, so `require` and `prefer` would live in comments. The language front end is about a quarter of the code; the solver and checker, which a plugin would cripple, are the rest.
