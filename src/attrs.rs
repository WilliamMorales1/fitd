//! attrs: the attribute schema shared by sema checks, LSP docs, and `fitd docs attrs`.

/// Attr documents one attribute key on the statements that accept it.
pub struct Attr {
    pub key: &'static str,
    pub on: &'static [&'static str],
    pub values: &'static [&'static str],
    pub doc: &'static str,
}

/// HEADS are the edge end marks, in `Head` enum order.
pub const HEADS: &[&str] = &["arrow", "triangle", "diamond", "odiamond", "crow", "none"];
/// DASHES are the line patterns, in `Dash` enum order.
pub const DASHES: &[&str] = &["solid", "dashed", "dotted"];

const LOOK: &[&str] = &["node", "style", "key"];

/// ATTRS lists every attribute; the first entry naming a statement wins.
pub const ATTRS: &[Attr] = &[
    Attr {
        key: "color",
        on: &["node", "style", "key"],
        values: &[],
        doc: "Border and accent color, `#RGB` or `#RRGGBB`.",
    },
    Attr {
        key: "color",
        on: &["edge"],
        values: &[],
        doc: "Line color of the edge, `#RGB` or `#RRGGBB`.",
    },
    Attr {
        key: "color",
        on: &["group"],
        values: &[],
        doc: "Frame and title color of the group.",
    },
    Attr {
        key: "fill",
        on: LOOK,
        values: &[],
        doc: "Fill color inside the frame.",
    },
    Attr {
        key: "text",
        on: LOOK,
        values: &[],
        doc: "Text color.",
    },
    Attr {
        key: "shape",
        on: LOOK,
        values: &["box", "circle", "diamond", "none"],
        doc: "Frame shape. `none` draws text and figure with no frame, for margin notes and lane labels.",
    },
    Attr {
        key: "stack",
        on: &["node", "style"],
        values: &["2", "3", "4"],
        doc: "Draws 1 to 8 offset copies, the usual mark for N identical layers.",
    },
    Attr {
        key: "wrap",
        on: &["node", "style"],
        values: &[],
        doc: "Wrap width for this node in `em`, overriding `font wrap=`.",
    },
    Attr {
        key: "place",
        on: &["node", "style"],
        values: &["flow", "aside"],
        doc: "`aside` moves the node into a strip after a layered flow, reached only by `..>` links. Default `flow`.",
    },
    Attr {
        key: "line",
        on: &["key"],
        values: DASHES,
        doc: "Draws an edge swatch instead of a node swatch: `solid` for `->`, `dashed` for `..>`.",
    },
    Attr {
        key: "line",
        on: &["edge", "style"],
        values: DASHES,
        doc: "Dash pattern of the edge. Default `solid`, `dashed` for `..>`.",
    },
    Attr {
        key: "head",
        on: &["edge", "style", "key"],
        values: HEADS,
        doc: "Mark at the target end. Default `arrow` when directed, else `none`.",
    },
    Attr {
        key: "tail",
        on: &["edge", "style"],
        values: HEADS,
        doc: "Mark at the source end. Default `none`.",
    },
    Attr {
        key: "lane",
        on: &["node"],
        values: &[],
        doc: "Hangs the node on ID's dashed lifeline in a `lanes` layout, below its earlier lane nodes.",
    },
    Attr {
        key: "row",
        on: &["node"],
        values: &["1", "2", "3"],
        doc: "Grid row from 1: nodes with the same `row` align in a row, ordered by `col`.",
    },
    Attr {
        key: "col",
        on: &["node"],
        values: &["1", "2", "3"],
        doc: "Grid column from 1: nodes with the same `col` align in a column, ordered by `row`.",
    },
    Attr {
        key: "times",
        on: &["group"],
        values: &["2", "3", "4"],
        doc: "Marks the frame as repeated 2 to 99 times: dashed frame, `Nx` before the title.",
    },
    Attr {
        key: "root",
        on: &["layout"],
        values: &[],
        doc: "Root node of a `radial` layout.",
    },
    Attr {
        key: "dir",
        on: &["layout"],
        values: &["down", "right"],
        doc: "Flow direction of a `layered` or `lanes` layout.",
    },
    Attr {
        key: "bands",
        on: &["layout"],
        values: &["1", "2", "3"],
        doc: "Most bands a long `layered` flow may wrap into; `bands=1` forbids wrapping.",
    },
    Attr {
        key: "order",
        on: &["layout"],
        values: &["auto", "source"],
        doc: "Sibling order in a `layered` or `lanes` layout: `auto` cuts crossings, `source` keeps declaration and edge order.",
    },
    Attr {
        key: "margin",
        on: &["page"],
        values: &[],
        doc: "Page margin, such as `12mm`.",
    },
    Attr {
        key: "keysize",
        on: &["page"],
        values: &[],
        doc: "Legend swatch and text size; default 0.85 of the body font.",
    },
    Attr {
        key: "wrap",
        on: &["font"],
        values: &[],
        doc: "Text wrap width in `em`, such as `24em`.",
    },
    Attr {
        key: "family",
        on: &["font"],
        values: &[],
        doc: "Font family, quoted: `family=\"DejaVu Sans\"`.",
    },
    Attr {
        key: "background",
        on: &["theme"],
        values: &[],
        doc: "Page color, also used behind labels.",
    },
    Attr {
        key: "gradient",
        on: &["theme"],
        values: &[],
        doc: "Paints the page with a diagonal gradient from `background` to this color.",
    },
    Attr {
        key: "text",
        on: &["theme"],
        values: &[],
        doc: "Default text color.",
    },
    Attr {
        key: "edge",
        on: &["theme"],
        values: &[],
        doc: "Default color of `->` and `--` edges.",
    },
    Attr {
        key: "link",
        on: &["theme"],
        values: &[],
        doc: "Default color of `..>` and `..` cross-links.",
    },
    Attr {
        key: "label",
        on: &["theme"],
        values: &[],
        doc: "Default color of edge labels.",
    },
    Attr {
        key: "root",
        on: &["theme"],
        values: &[],
        doc: "Fill color of the root node.",
    },
    Attr {
        key: "palette",
        on: &["theme"],
        values: &[],
        doc: "Quoted list of accent colors cycled over nodes: `palette=\"#0072B2 #D55E00\"`.",
    },
    Attr {
        key: "gap",
        on: &["require", "prefer"],
        values: &[],
        doc: "Least distance for an ordering relation, such as `gap=2em`.",
    },
    Attr {
        key: "within",
        on: &["require", "prefer"],
        values: &[],
        doc: "Greatest distance for `near`, such as `within=4em`.",
    },
    Attr {
        key: "weight",
        on: &["prefer"],
        values: &["2", "5", "10"],
        doc: "Relative weight of a soft constraint against other preferences.",
    },
];

/// attr finds the documentation for `key` on statement `stmt`.
pub fn attr(stmt: &str, key: &str) -> Option<&'static Attr> {
    ATTRS.iter().find(|a| a.key == key && a.on.contains(&stmt))
}

/// attrs lists the keys statement `stmt` accepts, without duplicates.
pub fn attrs(stmt: &str) -> Vec<&'static Attr> {
    let mut out: Vec<&Attr> = Vec::new();
    for a in ATTRS.iter().filter(|a| a.on.contains(&stmt)) {
        if !out.iter().any(|o| o.key == a.key) {
            out.push(a);
        }
    }
    out
}

/// keys lists the attribute names statement `stmt` accepts, in table order.
pub fn keys(stmt: &str) -> Vec<&'static str> {
    attrs(stmt).iter().map(|a| a.key).collect()
}

/// table renders every attribute as `STMT KEY VALUES: DOC`, one per line.
pub fn table() -> String {
    let mut out = String::new();
    for a in ATTRS {
        let vals = if a.values.is_empty() {
            String::new()
        } else {
            format!(" {}", a.values.join("|"))
        };
        out += &format!("{} {}{}: {}\n", a.on.join(","), a.key, vals, a.doc);
    }
    out
}
