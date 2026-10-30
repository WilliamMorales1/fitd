//! Syntax tree. Every name and value keeps its span so later passes can point at it.

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
    /// Unparsed number with its unit, such as `7pt` or `2296x1524pt`; sema gives it a type.
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
    /// `a left-of b`, `a near b`, and the other two-node relations; `a left-of b left-of c`
    /// chains them, so `ops[i]` relates `nodes[i]` to `nodes[i + 1]`.
    Chain { nodes: Vec<Name>, ops: Vec<Name> },
    /// `align top(a, b, c)`.
    Align { side: Name, items: Vec<Name> },
    /// `a at top-left`.
    At { a: Name, place: Name },
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Title(String),
    Root(Name),
    Layout { kind: Name, attrs: Vec<Attr> },
    /// `page a1 portrait margin=24pt`: the size, then flags, then attributes.
    Page { size: Value, flags: Vec<Name>, attrs: Vec<Attr> },
    Font { size: Value, attrs: Vec<Attr> },
    Style { name: Name, attrs: Vec<Attr> },
    /// `theme dark background=#000`: an optional preset, then overrides.
    Theme { preset: Option<Name>, attrs: Vec<Attr> },
    Node { id: Name, title: Option<(String, Span)>, classes: Vec<Name>, attrs: Vec<Attr> },
    Body(String),
    Edge { from: Name, link: bool, to: Vec<Name>, label: Option<String> },
    Constraint { hard: bool, rel: Rel, attrs: Vec<Attr> },
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}
