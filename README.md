# fitd

A small diagram language with a constraint solver for layout. Write nodes, edges, and layout constraints in a plain text file; fitd finds the largest font that fits a fixed page, satisfies every `require`, trades off every `prefer`, then verifies the result before writing SVG, PNG, or TikZ.

```
title "How fitd compiles a diagram"
layout layered
page a3

node lex "Lexer"
| tokens with byte spans
node parse "Parser"
node bnb "Branch and bound"
node core "QuickXplain"

lex -> parse
parse -> bnb, core
core ..> lex "points back at source"

require bnb left-of core
require align row(bnb, core)
prefer core at bottom-right weight=2
```

![examples/compiler.fitd rendered to PNG](docs/compiler.png)

## Usage

```fish
cargo build --release
./target/release/fitd examples/compiler.fitd -o docs/compiler.png
./target/release/fitd examples/compiler.fitd --tikz out/compiler.tex
./target/release/fitd guide          # the language on one page
./target/release/fitd explain E0110  # explain an error code
```

Tests: `cargo test --release`.

## Docs

- [Language](docs/LANGUAGE.md): grammar, semantics, relations, diagnostics.
- [Options and theming](docs/OPTIONS.md): command-line flags, colors, fonts.
- [Agent skill](skills/fitd/SKILL.md): drawing diagrams with fitd from an AI agent.
- [Internals](docs/INTERNALS.md): pipeline, solver, references, tests, examples.
