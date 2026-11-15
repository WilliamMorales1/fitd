//! Hover and completion text for the language's fixed vocabulary.

/// Keyword is one statement or macro keyword: (word, snippet, signature, doc).
pub type Keyword = (&'static str, &'static str, &'static str, &'static str);

/// KEYWORDS lists statement keywords, then macro directives.
pub const KEYWORDS: &[Keyword] = &[
    (
        "title",
        "title \"$1\"",
        "title \"TEXT\"",
        "Sets the document title.",
    ),
    (
        "root",
        "root $1",
        "root ID",
        "Sets the root node for layouts that need one, such as `radial`.",
    ),
    (
        "layout",
        "layout ${1|layered,radial|}",
        "layout NAME [attributes]",
        "Selects the layout algorithm: `layered` (flows; `dir`, `bands`, `order`) or `radial` (mind maps; `root`).",
    ),
    (
        "page",
        "page ${1|a4,a3,a2,a1,a0,16:9,4:3,1:1|} ${2|landscape,portrait|}",
        "page SIZE [portrait|landscape] [margin=LEN] [keysize=LEN]",
        "Selects page size and orientation. Sizes are `a0`..`a4`, `WIDTHxHEIGHT` with a unit such as `297x210mm`, or a ratio `W:H` such as `16:9`, which picks the smallest page of that shape that fits at the font floor.",
    ),
    (
        "font",
        "font ${1:9pt..28pt}",
        "font SIZE|LO..HI [wrap=EM] [family=\"NAME\"]",
        "Sets the body font size, or the range the solver may search. Units: `pt`, `mm`, `cm`, `in`, `em`.",
    ),
    (
        "theme",
        "theme ${1|paper,light,dark,mono|}",
        "theme [paper|light|dark|mono] [attributes]",
        "Selects a color preset (default `paper`) and overrides single colors: `background`, `gradient`, `text`, `edge`, `link`, `label`, `root`, `palette`.",
    ),
    (
        "style",
        "style ${1:name} color=#$2",
        "style NAME attributes",
        "Declares a reusable class. A line holding only NAME (or `nn.block`) opens a block: nodes, edges, groups, and keys indented under it take it, and node lines may drop `node`. Nested blocks apply outer first.",
    ),
    (
        "node",
        "node ${1:id} \"$2\"",
        "node ID [\"TITLE\"] [attributes]",
        "Declares a node. Without a title the id is shown. Follow with `| text` body lines, `~ cells` figure rows, or `~ image \"plot.png\" width=10em`.",
    ),
    (
        "group",
        "group \"$1\" $2",
        "group [\"TITLE\"] ID, ID... [color=] [times=N]",
        "Frames the listed nodes. To nest, list inner members in the outer group too. `times=N` marks a repeated stack.",
    ),
    (
        "key",
        "key \"$1\" $2",
        "key \"LABEL\" [attributes]",
        "Adds a legend entry along the bottom of the page. `line=solid|dashed` draws an edge swatch.",
    ),
    (
        "require",
        "require ${1:a} ${2|left-of,right-of,above,below,near|} ${3:b}",
        "require RELATION [gap=LEN] [within=LEN]",
        "Hard constraint: output fails if it cannot hold. Relations: `a left-of b`, `a at top`, `align row(a, b)`.",
    ),
    (
        "prefer",
        "prefer ${1:a} ${2|left-of,right-of,above,below,near|} ${3:b}",
        "prefer RELATION [weight=N] [gap=LEN] [within=LEN]",
        "Soft constraint, traded off by `weight`. An unmet preference gives W0205.",
    ),
    (
        "include",
        "include \"$1\"",
        "include \"PATH\"",
        "Splices another file before macros run. Paths are relative to this file; a bare name like `nn` names a bundled module, whose templates are called as `nn.input(...)`.",
    ),
    (
        "for",
        "for ${1:i} in ${2:1..3}\n\t$0\nend",
        "for VAR in LO..HI | for VAR in a, b | for I, VAR in a, b",
        "Repeats the block once per value, ending at `end`. Use `${VAR}` to interpolate.",
    ),
    (
        "if",
        "if ${1:cond}\n\t$0\nend",
        "if EXPR ... [else if EXPR ...] [else ...] end",
        "Keeps the first block whose condition is true. Zero and the empty string are false.",
    ),
    (
        "else",
        "else",
        "else | else if EXPR",
        "Starts the fallback branch of an `if`.",
    ),
    ("end", "end", "end", "Closes a `for`, `if`, or `def` block."),
    (
        "def",
        "def ${1:name}(${2:id})\n\t$0\nend",
        "def NAME(PARAM, ...) ... end",
        "Defines a template at top level. A `#` comment just above becomes its documentation.",
    ),
];

/// MACRO_WORDS are the keywords that only the macro stage reads.
pub const MACRO_WORDS: &[&str] = &["for", "if", "else", "end", "def"];

pub use crate::attrs::{Attr, attr, attrs};

/// WORDS documents names that appear in value or relation positions, by role.
pub const WORDS: &[(&str, &str, &str)] = &[
    (
        "relation",
        "left-of",
        "`a left-of b`: a's right edge is left of b's left edge. Takes `gap=`.",
    ),
    (
        "relation",
        "right-of",
        "`a right-of b`: a sits right of b. Takes `gap=`.",
    ),
    (
        "relation",
        "above",
        "`a above b`: a's bottom is above b's top. Takes `gap=`.",
    ),
    (
        "relation",
        "below",
        "`a below b`: a sits below b. Takes `gap=`.",
    ),
    (
        "relation",
        "near",
        "`a near b`: centers stay close. Takes `within=`.",
    ),
    (
        "relation",
        "at",
        "`a at PLACE`: pins a node to one of nine page positions.",
    ),
    (
        "relation",
        "align",
        "`align SIDE(a, b, ...)`: lines nodes up by `top`, `bottom`, `left`, `right`, `row`, or `column`.",
    ),
    ("side", "top", "Aligns top edges."),
    ("side", "bottom", "Aligns bottom edges."),
    ("side", "left", "Aligns left edges."),
    ("side", "right", "Aligns right edges."),
    ("side", "row", "Aligns vertical centers, one row."),
    ("side", "column", "Aligns horizontal centers, one column."),
    ("place", "top-left", "Top-left corner of the page."),
    ("place", "top", "Top edge, centered."),
    ("place", "top-right", "Top-right corner of the page."),
    ("place", "left", "Left edge, centered."),
    ("place", "center", "Page center."),
    ("place", "right", "Right edge, centered."),
    ("place", "bottom-left", "Bottom-left corner of the page."),
    ("place", "bottom", "Bottom edge, centered."),
    ("place", "bottom-right", "Bottom-right corner of the page."),
    (
        "layout",
        "layered",
        "Layered layout for flows and pipelines. Takes `dir=down|right`, `bands=N`, and `order=auto|source`.",
    ),
    (
        "layout",
        "lanes",
        "Swimlanes and sequence diagrams: each top-level group or lifeline is a lane. Takes `dir=down|right` and `order=auto|source`.",
    ),
    (
        "layout",
        "venn",
        "Venn diagram: each group is a set circle (at most three), and nodes sit in the region of their sets.",
    ),
    (
        "layout",
        "radial",
        "Radial layout for mind maps around `root=ID`.",
    ),
    (
        "theme",
        "paper",
        "Black on white with colorblind-safe tints, the default.",
    ),
    ("theme", "dark", "Dark preset."),
    ("theme", "light", "Light preset on white."),
    ("theme", "mono", "Monochrome preset for print."),
    ("flag", "portrait", "Page taller than wide."),
    ("flag", "landscape", "Page wider than tall."),
    ("page", "a0", "ISO A0, 841 x 1189 mm."),
    ("page", "a1", "ISO A1, 594 x 841 mm."),
    ("page", "a2", "ISO A2, 420 x 594 mm."),
    ("page", "a3", "ISO A3, 297 x 420 mm."),
    ("page", "a4", "ISO A4, 210 x 297 mm."),
    (
        "page",
        "16:9",
        "Widescreen shape; fitd picks the smallest size that fits at the font floor.",
    ),
    (
        "page",
        "4:3",
        "Classic screen shape; fitd picks the smallest size that fits at the font floor.",
    ),
    (
        "page",
        "1:1",
        "Square; fitd picks the smallest size that fits at the font floor.",
    ),
    ("shape", "box", "Rectangular frame."),
    ("shape", "circle", "Circular frame."),
    (
        "shape",
        "diamond",
        "Rhombus frame, for decisions and relationships.",
    ),
    ("shape", "none", "No frame or fill."),
    ("place=", "flow", "Stays in the flow (default)."),
    ("place=", "aside", "Moves to the strip after the flow."),
    ("dir", "down", "Flow runs top to bottom."),
    ("dir", "right", "Flow runs left to right."),
    ("line", "solid", "Solid line."),
    ("line", "dashed", "Dashed line, as `..>` links draw."),
    ("line", "dotted", "Dotted line."),
    ("head", "arrow", "Filled arrowhead."),
    ("head", "triangle", "Hollow triangle: UML inheritance."),
    ("head", "diamond", "Filled diamond: UML composition."),
    ("head", "odiamond", "Hollow diamond: UML aggregation."),
    (
        "head",
        "crow",
        "Crow's foot: the many end of an ER relationship.",
    ),
    ("head", "none", "No mark."),
];

/// word finds the documentation for `name` in role `role`.
pub fn word(role: &str, name: &str) -> Option<&'static str> {
    WORDS
        .iter()
        .find(|(r, n, _)| *r == role && *n == name)
        .map(|w| w.2)
}

/// words lists the names of one role.
pub fn words(role: &str) -> Vec<(&'static str, &'static str)> {
    WORDS
        .iter()
        .filter(|(r, _, _)| *r == role)
        .map(|&(_, n, d)| (n, d))
        .collect()
}

/// value_role maps an attribute key to the role its enum values document under.
pub fn value_role(key: &str) -> &str {
    match key {
        "place" => "place=",
        k => k,
    }
}

/// OPERATORS documents edge operators.
pub const OPERATORS: &[(&str, &str)] = &[
    (
        "..>",
        "Directed cross-link, drawn dashed. Does not shape the layout tree.",
    ),
    ("->", "Directed flow edge."),
    ("--", "Undirected flow edge."),
    ("..", "Undirected cross-link, drawn dashed."),
];

/// explain returns the long description of a diagnostic code.
pub fn explain(code: &str) -> Option<&'static str> {
    crate::check::EXPLAIN
        .iter()
        .find(|(c, _)| *c == code)
        .map(|e| e.1)
}
