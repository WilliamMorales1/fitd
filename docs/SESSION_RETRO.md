# Retro: the hand-made MATH 4200 concept map

The session that motivated fitd produced, in order, a theorem/definition cheatsheet (Markdown to PDF), a radial concept map in TikZ, and then "increase the font as much as possible without overlap." These notes cover what happened, what it cost, and what to do instead. Counts are approximate and were reconstructed from the transcript.

## What happened

### Cheatsheet PDF (cheap, mostly fine)

- The first tectonic build reported missing glyphs: DejaVu Serif has no `≅`/`≇` and no bold `ℤℚℝℂ`. That cost one extra build plus a `newunicodechar` fix.
- The Markdown had been written with LaTeX-ish `a^{φ(n)}` and `p_k`. These rendered literally, so every occurrence was converted to pandoc `^..^` / `~..~` syntax and the page images were checked.
- **Avoidable:** list the document's non-ASCII characters and check font coverage *before* the first build (`fc-query` / a 5-line script), and write the target markup syntax from the start.

### Concept map (expensive)

About 150 tectonic compiles and about 20 page-image reads (images are the costliest thing to put in context).

| Iteration | Problem | Root cause |
|---|---|---|
| Hand polar coordinates | very sparse, then overlapping | positions guessed, sizes unknown |
| Greedy auto-layout script | twigs behind boxes | no edge-through-box constraint |
| Greedy plus crossing check | ran forever, then hit a 300 s timeout | greedy with no backtracking; unreachable condition |
| `pkill -f layout.py` | killed its own shell (exit 144) | `-f` matched the command line of the shell that ran it |
| Tree rewiring | fixed most crossings | should have been the model from the start |
| TikZ `bend` on long links | `Dimension too large` | pgf's fixed-point math overflows on long distances |
| `\\ [G : H]` in node text | `Missing number` | `\\` treats a following `[` as its optional argument |
| `\fpeval` | undefined control sequence | `xfp` not loaded (older LaTeX in tectonic) |
| Size estimates from character counts | wrong boxes | estimated instead of measured |
| Log-based measurement | circles under-measured by 29% for about 20 runs | `south west`/`north east` anchors of a circle sit at 45°, not on the bounding box |
| Cayley's Theorem box | three rounds of manual reordering, then reparenting | greedy order; a backtracking solver does this automatically |
| `text width` on circles | hub label spilled outside the circle | TikZ sizes circles from content, not `text width`; found only by looking |
| "As large as possible" | objective invented mid-way (keep page, then an "effective" ratio) | objective never pinned down |

Result: 16pt body font on a 2296×1524pt page.

**fitd on the same input and page:** 19.8pt body font, 0 errors, about 5 s, one preview image. Its constraints are also stricter, because it forbids crossing tree edges and edges through *any* box.

## What would have saved tokens

1. **Pin the objective first.** "Fixed page, maximize the minimum font, with hard non-overlap and no edge through a box." One question (screen or print? which page size?) settles it. The mid-session "effective ratio" detour came from not having it.
2. **Measure exactly, once.** Read glyph advances from the TTF (`ttf-parser`), or do one measuring compile and *unit-test the measurement* against a known box. The 45° anchor bug would have shown up in the first run instead of after two grid searches.
3. **Keep TeX out of the inner loop.** Do geometry and layout in a real language and emit absolute coordinates, so TeX only typesets. That removes the `bend` overflow, the `\\ [` parse, and the circle-sizing surprises, and it turns each iteration from seconds into milliseconds.
4. **Constraints and a checker before eyes.** Overlap, edge-through-box, crossing, overflow, and page bounds are cheap to compute. A checker that prints "`ftcg` overlaps `gens`" replaces most image reads. Look at the image once, at low resolution, for aesthetics only.
5. **Backtracking, not greedy.** Each "move Cayley, reorder siblings, reparent" step was a manual backjump. Conflict-directed backjumping does it in the solver.
6. **Search in one process.** Grid searches launched one tectonic build per point. A binary search on the font inside the solver does about 12 probes per run.
7. **Use the standard tool for a first draft.** `pacman -S graphviz` (`twopi` with `overlap=prism`) would have given a usable radial draft in one command. It can't maximize the font, but it is a better start than hand-picked angles.
8. **Shell hygiene.** Use `pgrep`/`kill PID`, never `pkill -f` on a pattern that appears in the current command. Wrap any search loop in `timeout`. Run long searches in the background and only read their summary line.
9. **Batch questions about scope early.** "Prove font size maximal or just bigger?" and "is the page size fixed?" are cheap to ask and expensive to discover.

## Recipe for next time

```fish
# 1. Write the graph (nodes, bodies, edge/link lines) — usually generated from notes by a script.
# 2. Lay out, verify, emit:
~/Projects/fitd/target/release/fitd build map.fitd --page a1 --tikz map.tex
# 3. Only if 0 errors: one low-res look.
rsvg-convert -z 0.3 map.svg -o /tmp/map.png
```

The same principles apply to any "fit text into a fixed space" request (cheatsheets, posters, slides): exact metrics, hard constraints, a binary search on the font, a checker, and one look at the end.
