---
name: fitd
description: Draw concept maps, mind maps, and flowcharts with the fitd CLI (.fitd text in, SVG/PNG/TikZ out). Use when asked to make a diagram, concept map, mind map, flowchart, or timeline, or when editing or fixing a .fitd file.
---

# fitd

fitd compiles a small text language into a diagram. A constraint solver places every box and picks the largest font that fits the page, so the source never holds coordinates or sizes.

## Workflow

1. Run `fitd guide`. It prints the whole language on one page (about 600 tokens). Treat it as the reference; this skill does not repeat it.
2. Write the source and build in one command, reading from stdin:

```sh
fitd - -o out/leaf.png --short <<'EOF'
title "Photosynthesis"
node leaf "Leaf"
node sun "Light"
| photons absorbed by chlorophyll
node sugar "Glucose"
leaf -> sun, sugar
sun ..> sugar "drives"
EOF
```

3. Exit status 0 means the file was written. Otherwise fix and rebuild (see Fix loop).

If the user wants the source kept, write it to `NAME.fitd` and build with `fitd NAME.fitd -o NAME.png --short`.

## Choosing a layout

- Concept map or mind map (one topic branching out): the default, `layout radial`. Pick the center with `root=ID`; otherwise fitd picks it.
- Process, pipeline, timeline, decision flow: `layout layered dir=down` or `dir=right`. Long flows wrap into bands; `bands=1` forbids wrapping.
- `->` edges build the hierarchy or flow. `..>` draws a dashed cross link that does not change the structure. Use cross links for "relates to" connections. `--` and `..` are the same without an arrowhead, for relations with no direction.

## Repetition

When many nodes share one shape (numbered steps, one box per service, layers, recursion trees), write a loop or a template instead of copying lines: `for i in 1..n` or `for i, x in a, b, c` ... `end`, `def NAME(args)` ... `end` with `use NAME(...)`, and `${expr}` inside any line. Errors on generated lines are reported at the template line.

## Start minimal

Only `node` lines and edges are required. Leave out `page`, `font`, `theme`, and constraints at first: the defaults (A1 page, light theme, font searched) usually work. Add a `require` only when the user asks for a specific placement, because each one can make the layout unsatisfiable. Prefer `prefer` over `require` when the placement is a wish, not a rule.

## Fix loop

`--short` prints one line per diagnostic:

```
<stdin>:3:14: error[E0001]: unknown node `sugar`
```

- Fix the first error only, then rebuild. Later errors are often caused by the first.
- `fitd explain CODE` prints the meaning and fix for any code.
- E0110 (constraints cannot all hold): the message lists a minimal conflicting set. Drop one of them, turn it into `prefer`, or use a larger `page`.
- E0110 in a radial map with `left-of`/`right-of`/`above`/`below` between nodes on different branches: each branch stays within 30 degrees of its own wedge, so such a relation may be impossible at any font. Switch to `layout layered`, which places nodes freely within layers (but the flow fixes the other axis: with `dir=down`, two nodes in the same layer can never be `above` one another).
- No layout fits (page too small): lower the font floor (`font 6pt..30pt` or `--min-font`), narrow the text with `font ... wrap=12em` (or `--wrap`), use a larger page, or follow the printed help.
- Font smaller than expected: near the largest font that fits, search gets much harder; `--budget 100000000` (default 20M checks per probe) sometimes finds a slightly larger font. A layered layout of a long chain is limited by its number of layers; try `dir=right` or `page ... portrait`.
- Warnings (W codes) do not block output. W0205 means a `prefer` was not met; usually fine.

## Useful flags

| Flag | Use |
| --- | --- |
| `-o FILE.svg` / `.png` | Output format by extension. PNG is cropped to the drawing. |
| `--tikz FILE.tex` | Also write standalone LaTeX. |
| `--short` | One line per diagnostic. Always use it. |
| `--draft` | 2 to 6 times faster; use while iterating, drop it for the final build. |
| `--stable FILE.lock` | Keep nodes and font in place when editing a map someone already saw. |
| `--page a0..a4` or `WxHmm` | Page size; smaller pages give smaller fonts. |
| `--portrait` / `--landscape` | Fix orientation. |
| `--theme light\|dark\|mono` | Color preset; `mono` for print. |

Flags override settings in the file.

## Checking the result

Open the PNG (read the image) and check that labels are legible and nothing important is missing. The success line reports the chosen font and page, e.g. `body font 48.0pt on 2384x1684pt; 0 errors, 0 warnings`. A very small font means too much content for the page: split the diagram (`fitd split map.fitd` writes an overview plus one page per top-level branch), shorten bodies, or use a larger page.

The chosen font is often not the largest that fits. fitd also lays out at 5% and 10% below it and keeps the one that reads best (fewer crossings, related nodes closer), as long as it stays above a legibility floor of 16pt per meter of page diagonal. A font a little under the maximum is intended, not a failure; `--min-font` raises that floor.

## Examples

`examples/` in the fitd repo holds full diagrams: `compiler.fitd` and `cell_biology.fitd` (layered flowcharts with groups and styles), `group_theory.fitd` and `earth_science.fitd` (radial maps), `french_history.fitd` (a layered timeline with group frames), `coffee.fitd` (radial with a palette and a group), `release_pipeline.fitd` (banded flow, `include`, theme overrides, chained constraints), `debugging.fitd` (mono decision flow at a fixed font), `deepseek_v4_1_flash.fitd` (model architecture, undirected edges), and four that use macros (`for`, `if`, `def`/`use`, `${expr}`; see "Macros" in `docs/LANGUAGE.md`): `semester.fitd` (a timeline from a list loop), `services.fitd` (a template per service), `fib_calls.fitd` (a recursive template), `neural_net.fitd` (dense layers from loops). `docs/EXAMPLES.md` ends with a feature index. Read one when the user asks for a feature the guide only names.
