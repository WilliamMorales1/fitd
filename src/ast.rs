use crate::diag::Span;

#[derive(Clone, Debug)]
pub struct Name {
    pub text: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Value {
    Name(Name),
    Str(String, Span),
    Num(String, Span),
    Color(String, Span),
    Range(Box<Value>, Box<Value>),
}

impl Value {
    pub fn span(&self) -> Span {
        match self {
            Value::Name(n) => n.span,
            Value::Str(_, s) | Value::Num(_, s) | Value::Color(_, s) => *s,
            Value::Range(a, b) => a.span().to(b.span()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Attr {
    pub key: Name,
    pub value: Value,
}

#[derive(Clone, Debug)]
pub enum Rel {
    Chain { nodes: Vec<Name>, ops: Vec<Name> },
    Align { side: Name, items: Vec<Name> },
    At { a: Name, place: Name },
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Title(String),
    Root(Name),
    Layout {
        kind: Name,
        attrs: Vec<Attr>,
    },
    Page {
        size: Value,
        flags: Vec<Name>,
        attrs: Vec<Attr>,
    },
    Font {
        size: Value,
        attrs: Vec<Attr>,
    },
    Style {
        name: Name,
        attrs: Vec<Attr>,
    },
    Theme {
        preset: Option<Name>,
        attrs: Vec<Attr>,
    },
    Node {
        id: Name,
        title: Option<(String, Span)>,
        classes: Vec<Name>,
        attrs: Vec<Attr>,
        /// Set for a lone `ID`, which reopens the node when it is already declared.
        bare: bool,
    },
    /// Body is a `|` text line of the node whose braces hold it.
    Body {
        text: String,
        owner: Option<Name>,
    },
    /// Row is a `~` figure row of the node whose braces hold it.
    Row {
        row: Row,
        owner: Option<Name>,
    },
    Key {
        label: String,
        classes: Vec<Name>,
        attrs: Vec<Attr>,
    },
    Edge {
        from: Name,
        link: bool,
        /// None for a nested child; sema makes it directed unless the layout is radial.
        directed: Option<bool>,
        to: Vec<Name>,
        label: Option<String>,
        classes: Vec<Name>,
        attrs: Vec<Attr>,
    },
    Group {
        /// Set by `group ID "Title" ...`, so edges can name the group.
        id: Option<Name>,
        title: Option<String>,
        items: Vec<Name>,
        classes: Vec<Name>,
        attrs: Vec<Attr>,
    },
    Constraint {
        hard: bool,
        rel: Rel,
        attrs: Vec<Attr>,
    },
    Include(String),
    /// Open is a `with STYLE` block; the nodes, edges, keys, and groups inside it take the style.
    Open(Name),
}

#[derive(Clone, Debug)]
pub enum Row {
    Cells(Vec<crate::graph::Cell>),
    Image { path: String, attrs: Vec<Attr> },
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}
