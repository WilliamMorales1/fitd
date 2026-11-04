# Command-line reference

## Invocation

```text
fitd INPUT [-o OUTPUT] [OPTIONS]
fitd INPUT... -o OUTPUT.pdf
fitd import INPUT.dot|INPUT.mmd|INPUT.d2 [-o OUTPUT.fitd]
fitd split INPUT.fitd [-o DIRECTORY]
fitd explain CODE
fitd guide
fitd lsp
```

`INPUT` is a `.fitd` path or `-` for standard input. Standard input requires `-o`. Multiple inputs are supported for PDF output; each input becomes one page in argument order. Options override settings in source.

Exit status is zero when output is written. Diagnostics go to standard error. Errors normally prevent output; `--force` writes output despite checker errors. `--short` prints one line per diagnostic.

## Build options

| Option | Description |
| --- | --- |
| `-o PATH` | Output path. Extension selects SVG, PNG, or PDF. PNG is cropped to drawing bounds; SVG and PDF retain page dimensions. |
| `--tikz PATH` | Write TikZ source to `PATH`. |
| `--page SIZE` | Override page size. Accepts `a0` through `a4` or `WIDTHxHEIGHT` with optional unit. |
| `--portrait` | Set portrait orientation. |
| `--landscape` | Set landscape orientation. |
| `--margin PT` | Set page margin in points. |
| `--min-font PT` | Set lower font-size search bound in points. |
| `--max-font PT` | Set upper font-size search bound in points. |
| `--wrap EM` | Set text wrapping width in em units. |
| `--bands N` | Set maximum bands for layered layout. |
| `--theme NAME` | Select `dark`, `light`, or `mono` theme. |
| `--font-family NAME` | Select installed font family. |
| `--budget CHECKS` | Set layout search check budget. Default: 20,000,000. |
| `--expand-budget STEPS` | Set macro expansion budget. Default: 100,000. |
| `--scale PX` | Set PNG raster scale in pixels per point. Default: 2. |
| `--stable PATH` | Read or write layout lock file at `PATH`. |
| `--force` | Write output despite checker errors. |
| `--draft` | Use reduced search effort. |
| `--short` | Print diagnostics in one-line form. |
| `--watch` | Rebuild after input changes. Accepts one file input, not standard input. |
| `--grid COLSxROWS` | Arrange multiple diagrams in a PDF grid. |

`FITD_DEBUG=1` prints solver probes. CLI values override corresponding source settings.

## Output formats

Output format is selected from file extension. SVG preserves page dimensions. PDF preserves page dimensions and selectable text. PNG is cropped to drawing bounds. `--tikz` writes a separate TikZ file; compiling TikZ requires a LaTeX installation with TikZ.

For multiple input files and PDF output, fitd writes one page per input. `--grid COLSxROWS` arranges multiple diagrams on each page. PDF options apply to all inputs.

## Themes and fonts

Theme presets are `dark`, `light`, and `mono`. Source-level overrides use `theme` attributes. Supported theme keys are `background`, `text`, `edge`, `link`, `label`, `root`, and `palette`.

Node, edge, and group attributes include `color`, `fill`, and `text` where supported. `family` selects font family in source; `--font-family` overrides it. A requested family must be available through fontconfig. Without embedded fonts, fontconfig is required for font discovery.

## Stable layouts

`--stable PATH` reads prior font, page, and node-center positions and writes updated values to lock file. Search prefers locked positions and penalizes movement. A lock created for a different page size is ignored with a diagnostic.

## Watch mode

`--watch` requires one file input. Each change starts a draft build. A full build starts after input remains unchanged for 1.5 seconds. A new change cancels active full build. Output replacement is atomic.

## Split command

`fitd split INPUT [-o DIRECTORY]` writes shared settings, an overview source file, and one source file per top-level branch. Generated files include shared settings with `include`. Edges crossing page boundaries appear on both pages with endpoint stubs. Constraints spanning pages are omitted and reported as warnings. Run split again from original input to regenerate files.

## Other commands

- `fitd explain CODE` prints extended diagnostic information.
- `fitd guide` prints concise language syntax and examples.
- `fitd import INPUT` converts Graphviz DOT, Mermaid, or D2 input to `.fitd` source.
- `fitd lsp` starts language server using standard input and output.
