
use crate::diag::Span;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shape {
    Circle,
    Rect,
}

pub struct Node {
    pub id: String,
    pub title: String,
    pub body: Vec<String>,
    pub color: Option<String>,
    pub fill: Option<String>,
    pub text: Option<String>,
    pub shape: Option<Shape>,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EdgeKind {
    Tree,
    Link,
}

pub struct Edge {
    pub a: usize,
    pub b: usize,
    pub kind: EdgeKind,
    pub directed: bool,
    pub label: Option<String>,
    pub color: Option<String>,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Length {
    Pt(f64),
    Em(f64),
}

impl Length {
    pub fn pt(self, body: f64) -> f64 {
        match self {
            Length::Pt(x) => x,
            Length::Em(x) => x * body,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
    Row,
    Column,
}

impl Side {
    pub fn vertical(self) -> bool {
        matches!(self, Side::Top | Side::Bottom | Side::Row)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Place(pub u8, pub u8);

#[derive(Clone, PartialEq, Debug)]
pub enum Relation {
    LeftOf {
        a: usize,
        b: usize,
        gap: Length,
    },
    Above {
        a: usize,
        b: usize,
        gap: Length,
    },
    Near {
        a: usize,
        b: usize,
        within: Length,
    },
    Align {
        side: Side,
        items: Vec<usize>,
    },
    At {
        a: usize,
        place: Place,
    },
    Group {
        items: Vec<usize>,
        title: Option<String>,
        color: Option<String>,
        title_em: f64,
        inner: Vec<Relation>,
    },
}

pub enum NodeIndices<'a> {
    Single(Option<usize>),
    Pair(std::array::IntoIter<usize, 2>),
    Many(std::iter::Copied<std::slice::Iter<'a, usize>>),
}

impl Iterator for NodeIndices<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Single(value) => value.take(),
            Self::Pair(values) => values.next(),
            Self::Many(values) => values.next(),
        }
    }
}

impl Relation {
    pub fn nodes(&self) -> NodeIndices<'_> {
        match self {
            Relation::LeftOf { a, b, .. }
            | Relation::Above { a, b, .. }
            | Relation::Near { a, b, .. } => NodeIndices::Pair([*a, *b].into_iter()),
            Relation::Align { items, .. } | Relation::Group { items, .. } => {
                NodeIndices::Many(items.iter().copied())
            }
            Relation::At { a, .. } => NodeIndices::Single(Some(*a)),
        }
    }

    pub fn depth(&self) -> usize {
        match self {
            Relation::Group { inner, .. } => {
                1 + inner.iter().map(Relation::depth).max().unwrap_or(0)
            }
            _ => 0,
        }
    }
}

pub struct Constraint {
    pub rel: Relation,
    pub hard: bool,
    pub weight: u32,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Dir {
    Down,
    Right,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LayoutKind {
    Radial,
    Layered(Dir),
}

#[derive(Default)]
pub struct Settings {
    pub layout: Option<LayoutKind>,
    pub page: Option<(f64, f64)>,
    pub page_fixed: bool,
    pub bands: Option<usize>,
    pub margin: Option<f64>,
    pub font: Option<(f64, f64)>,
    pub font_span: Option<Span>,
    pub wrap_em: Option<f64>,
    pub family: Option<String>,
    pub theme: Option<String>,
    pub theme_over: crate::theme::Overrides,
}

pub struct Graph {
    pub title: Option<String>,
    pub root: Option<usize>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub constraints: Vec<Constraint>,
    pub settings: Settings,
}
