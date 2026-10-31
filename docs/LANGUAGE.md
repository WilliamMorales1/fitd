# The `.cmap` language

One statement per line. `#` starts a comment, except right after `=` where it starts a color (`color=#1565C0`). Names may contain letters, digits, `_`, and inner hyphens (`left-of`, `top-left`), but a hyphen never swallows an arrow, so `a->b` lexes as three tokens.

```
title "How cmapc compiles a diagram"
layout layered dir=down          # or: layout radial root=core
page a3 portrait margin=1cm      # a0..a4 or WxH with one unit: 594x420mm, 2296x1524pt
font 7pt..40pt wrap=16em         # body font range searched; leaf wrap width
theme dark link=#FFD54F          # color preset plus overrides

style stage color=#1565C0 shape=box

node lex "Lexer" .stage          # id, optional title, styles, attributes
| tokens with byte spans         # body lines attach to the node above

lex -> parse, sema "label"       # hierarchy / flow edges (also `--`)
core ..> sema "points back"      # cross links, drawn dashed (also `..`)
core ..> failure .stage          # an edge takes a style's color, or color=#hex

group "Front end" lex, parse     # framed together; nothing else inside the frame

require bnb left-of core gap=2em # hard constraint
require a above b above c        # chain: a above b, b above c
require align row(bnb, core)
prefer checker near bnb weight=2 # soft constraint, traded off by weight
prefer emit at bottom-right
```

## Grammar

```ebnf
file      = { [ stmt ] NEWLINE } EOF .
stmt      = "title" STRING
          | "root" NAME
          | "layout" NAME { attr }
          | "page" value { NAME } { attr }
          | "font" value [ ".." value ] { attr }
          | "theme" [ NAME ] { attr }
          | "style" NAME { attr }
          | "node" NAME [ STRING ] { "." NAME | attr }
          | "|" REST_OF_LINE
          | NAME edgeop targets [ STRING ] { "." NAME | attr }
          | "group" [ STRING ] targets { attr }
          | ( "require" | "prefer" ) rel { attr } .
edgeop    = "->" | "--" | "..>" | ".." .
targets   = NAME { [ "," ] NAME } .
rel       = "align" NAME "(" NAME { "," NAME } ")"
          | NAME "at" NAME
          | NAME NAME NAME { NAME NAME } .
attr      = NAME "=" value .
value     = NAME | STRING | NUMBER | COLOR .
```

`NUMBER` is a digit run with an attached unit or size suffix (`12pt`, `2em`, `594x420mm`); semantic analysis gives it a type.

A statement that fails to parse is reported and skipped to the end of its line, so one typo never hides the errors after it.

## Semantics

Analysis runs in two passes. The first collects `node` and `style` declarations (and attaches `|` bodies), so edges and constraints may name nodes declared further down. The second resolves every reference, applies styles in order (node attributes win), and types every value.

| Statement | Attributes | Notes |
|---|---|---|
| `layout radial` | `root=NODE` | Default. Root defaults to the graph center (minimum eccentricity). |
| `layout layered` | `dir=down\|right` | Flowchart. Directed cycles are broken; back edges are drawn as links. A flow too long to reach a readable font in one line wraps into bands that run back and forth. |
| `page` | flags `portrait`, `landscape`; `margin=LEN` | Absolute lengths only. A paper name without a flag leaves the orientation open: when a layout has warnings, cmapc also tries the turned page (and, in radial maps, branches reordered toward their links) and keeps the one with fewer. |
| `font LO..HI` or `font SIZE` | `wrap=EM`, `family="NAME"` | The solver maximizes the body font within the range. `family` must be installed; it is used for measuring and drawing. Default DejaVu Serif. |
| `theme [light\|dark\|mono]` | `background`, `text`, `edge`, `link`, `label`, `root` (colors); `palette="#hex ..."` | Preset defaults to `light`; each attribute overrides one preset value. |
| `node`, `style` | `color=#hex`, `fill=#hex`, `text=#hex`, `shape=circle\|box` | `color` is the accent (border, tint, branch); `fill` replaces the tint; `text` defaults to the theme text, or black/white by contrast on a solid fill. |
| edge | `color=#hex` | Stroke and arrowhead color. A `.style` on an edge gives only its `color`. Width and arrowheads follow the layout (hierarchy lines, flow arrows, dashed links) and cannot be set. |
| `group` | `color=#hex` | See Groups below. |
| `require`, `prefer` | `gap=LEN` (orderings), `within=LEN` (`near`), `weight=N` (`prefer` only) | |

Lengths need a unit: `pt`, `mm`, `cm`, `in`, or `em`. An `em` is the body font, which is only known once the solver picks it, so `gap=2em` scales with the font. Command-line flags override settings written in the file.

### Relations

Coordinates have y pointing up. Each relation is a predicate on the nodes' final boxes.

| Relation | Holds when |
|---|---|
| `a left-of b [gap=1em]` | `a.right + gap <= b.left` (`right-of` swaps) |
| `a above b [gap=1em]` | `a.bottom >= b.top + gap` (`below` swaps) |
| `a near b [within=4em]` | the space between the boxes is at most `within` |
| `align top\|bottom\|left\|right(a, b, ...)` | that edge is shared, within the strategy's grid tolerance |
| `align row(a, b, ...)` / `align column(...)` | centers share y / x |
| `a at top-left\|top\|...\|bottom-right\|center` | a's center lies in that third of the page |

Two-node relations chain: `a left-of b left-of c` means `a left-of b` and `b left-of c`. Steps may mix relations (`a above b near c`). Attributes apply to every step. Each step is its own constraint, so diagnostics point at the step that fails.

`require` is hard: the layout must satisfy it, or compilation fails. `prefer` is soft: the solver minimizes the total weight of unmet preferences after fixing the font, and the checker reports each unmet one as `W0205`.

### Groups

`group "Title" a, b, c` draws a frame, title on top, around its members. It is a hard constraint, like a `require`: the frame stays on the page and keeps clear of every other node and of every other group's frame. Edges and links may cross it. A node belongs to at most one group (E0010), so groups never share or nest. When a group cannot fit, it shows up in the E0110 core like any `require`.

In radial layout a frame is a rectangle, so it fits a branch's hub with its children far better than nodes from different branches. In layered layout the strategy keeps room for frames at both ends of the flow.

### Static contradiction check (E0008)

Before any search, hard orderings and alignments are checked for contradictions on each axis. Alignments that constrain the vertical axis (`top`, `bottom`, `row`) merge nodes into classes; so do `left`, `right`, and `column` horizontally. An ordering (`above`, `left-of`) between two nodes of one class, or a cycle of orderings between classes, can never hold, because boxes have positive size and gaps are non-negative. The search for such a cycle is a BFS over (node, used-an-ordering) states, so the reported cycle is short and every constraint on it gets a label.

```
error[E0008]: contradictory vertical constraints
  --> bad.cmap:10:1
   |
 9 | require align row(left, core)
   | -----------------------------
10 | require core above left
   | ^^^^^^^^^^^^^^^^^^^^^^^ these constraints cannot all hold
```

Contradictions that depend on geometry (sizes, page bounds, the strategy's candidate positions) are found later by search and reported as a minimal core (E0110, see `SOLVER.md`).

## Diagnostics

Codes run `E00xx` (front end), `E01xx` (layout), `W02xx` (warnings). `cmapc explain CODE` prints the long form.

| Code | Meaning |
|---|---|
| E0001 | unknown node (with a "did you mean" suggestion by edit distance) |
| E0002 | syntax error |
| E0003 | node or style declared twice (points at both) |
| E0004 | unknown style |
| E0005 | attribute not valid on this statement |
| E0006 | wrong value type or unit |
| E0007 | constraint names a node twice, or `align` with one node |
| E0008 | contradictory hard orderings (static) |
| E0009 | unknown layout, relation, side, place, shape, or flag |
| E0010 | node listed in two groups |
| E0101-E0107 | checker: overlap, edge through node, crossing, off page, text overflow, font too small, child nearer center |
| E0108 | layout breaks a `require` or a `group` (a solver bug if it ever appears) |
| E0109 | an edge label is covered by a node, overlaps another label, or is clipped (a solver bug if it ever appears) |
| E0110 | minimal set of `require` constraints that cannot hold together |
| W0201-W0207 | link through node, missing glyph, unused style, repeated setting, unmet preference, layered crossing, links crossing or running together |
