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
    },
    Body(String),
    Edge {
        from: Name,
        link: bool,
        directed: bool,
        to: Vec<Name>,
        label: Option<String>,
        classes: Vec<Name>,
        attrs: Vec<Attr>,
    },
    Group {
        title: Option<String>,
        items: Vec<Name>,
        attrs: Vec<Attr>,
    },
    Constraint {
        hard: bool,
        rel: Rel,
        attrs: Vec<Attr>,
    },
    Include(String),
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}
