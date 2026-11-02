# Options and theming

## Command-line options

An input of `-` reads the source from stdin and then needs `-o`. `-o FILE` picks the format by extension: `.svg` (default, `<input>.svg`) is page-sized; `.png` is cropped to the drawing plus `--margin` of padding, at `--scale PX` pixels per point (default 2). `--tikz FILE` also writes a standalone LaTeX file.

`--page a0..a4 | WxH[pt|mm]`, `--portrait`/`--landscape` (fix the orientation; otherwise a named paper may be turned when that clears warnings), `--bands N` (most bands a long flowchart wraps into; 1 never wraps), `--margin PT`, `--min-font PT` (hard floor; also raises the legibility floor of the font trade-off, default 16pt per meter of page diagonal), `--max-font PT`, `--wrap EM`, `--budget CHECKS` (default 20M per probe), `--force` (write output despite checker errors), `--short` (one line per diagnostic, for agents and editors), `--draft` (coarser font search, one font, short polish: 2 to 6 times faster for editing). `--theme light|dark|mono`, `--font-family NAME`, `--stable LOCK` (keep the layout steady across edits; see below), `--watch` (rebuild whenever the input changes: a `--draft` build at once, then a full build after 1.5 s without changes; a change kills the build in progress, so a slow full layout never delays the next draft; outputs are replaced atomically, so a viewer never reads half a file). Flags override settings in the file. `FITD_DEBUG=1` traces each probe.

## Theming

Every option has a default; leave all of them out and the output is the `light` theme in DejaVu Serif.

```
theme dark                                    # preset: light (default), dark, mono
theme dark background=#102030 link=#FFD54F    # preset plus overrides
theme palette="#1565C0 #C62828 #2E7D32"       # branch colors, cycled; base preset is light
font 7pt..40pt family="Inter"                 # measured and drawn in this family

style warm color=#EF6C00 fill=#FFE0B2 text=#3E2723
node a "Alpha" .warm
```

| Setting | Where | Default (`light`) | Controls |
| --- | --- | --- | --- |
| `background` | `theme` | `#FFFFFF` | page, box interiors, label backing |
| `text` | `theme` | `#111111` | text in boxes; root outline |
| `edge` | `theme` | `#555555` | layered arrows |
| `link` | `theme` | `#777777` | dashed cross links |
| `label` | `theme` | `#555555` | edge label text |
| `root` | `theme` | `#EEEEEE` | radial root circle fill |
| `palette` | `theme` | 8 colors | branch colors, quoted, space or comma separated |
| `family` | `font` | DejaVu Serif | body and title font; must be installed (`fc-list : family`); fallbacks still cover missing glyphs. Without fontconfig the family is ignored with a note |
| `color` | `node`, `style` | branch color | box border and tint, circle fill, branch lines |
| `color` | edge (`a -> b color=#hex` or `.style`) | branch color, theme `edge` or `link` | that edge's stroke and arrowhead |
| `color` | `group` | theme `edge` | frame border, tint, and title |
| `fill` | `node`, `style` | tint of `color` | solid fill instead of the tint |
| `text` | `node`, `style` | theme `text`, or black/white by contrast on filled shapes | text color |

`dark` suits screens; `mono` prints in grayscale. `--theme` swaps the preset but keeps the file's overrides. Colors are `#RGB` or `#RRGGBB`. The font family changes measured widths, so the solver may pick a different font size and layout.

## Stable layouts

The solver is global, so one new node can move every other one and change the font. `--stable map.fitd.lock` keeps a layout steady while editing: after each build fitd writes the font, page, and every node center to the lock; the next build reads it, tries each node's locked spot first, charges the polish for every node that leaves its spot (about one edge crossing per node, plus a pull per em of distance), and, while the locked font still fits, polishes both it and the new largest font and keeps the one with the better merit, which counts those moves. A larger font therefore wins only when its gain outweighs the nodes it moves: adding one leaf keeps the font and every spot, while deleting a large branch lets the font grow. The build log says how many locked nodes kept their spots. Nodes added since the lock are placed freely; nodes removed are dropped from it. A lock made for another page size is ignored with a note, and with a lock the warning-driven retry (reordered branches, turned page) is skipped, since it would move everything. Delete the lock to lay out afresh. The lock is plain text and diffs well, so it can be committed next to the source.

## Multi-page maps

A map too big for one page can be split with `fitd split map.fitd [-o DIR]` (default `map-pages/` next to the input). It writes `map-shared.fitd` with the layout, page, font, theme, and styles; `map.fitd`, the overview, with the root and a stub for each top-level branch whose body names that branch's page; and `map-BRANCH.fitd` for each branch, rooted at its top node. Every page includes the shared file, so each builds alone with plain `fitd`, and a style edited once changes every page. An edge between nodes on different pages is drawn on both pages, each time to a stub of the far end whose body names its page. A group across pages keeps each page's members; a chain such as `require a left-of b left-of c` keeps each run of links within one page; an `align` keeps each page's part of two or more nodes. What no page can show (a `near` or ordering between pages, or a node left alone in an `align`) is left out and listed as a warning. Comments are not carried over. The split files are ordinary source, so edit them freely afterwards; split again only from the original.
