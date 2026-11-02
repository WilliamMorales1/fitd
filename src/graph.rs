//! Checked intermediate form produced by sema: names resolved, values typed.

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
    /// Accent: border and tint of a box, fill of a circle, and its branch's color.
    pub color: Option<String>,
    pub fill: Option<String>,
    pub text: Option<String>,
    pub shape: Option<Shape>,
    /// Span of the id in its `node` statement.
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EdgeKind {
    /// Hierarchical: general idea to specific idea.
    Tree,
    /// Cross-reference drawn as a dashed arrow; never used for hierarchy unless needed for connectivity.
    Link,
}

pub struct Edge {
    pub a: usize,
    pub b: usize,
    pub kind: EdgeKind,
    /// False for `--` and `..`: drawn without an arrowhead; layers still follow the source order.
    pub directed: bool,
    pub label: Option<String>,
    /// Stroke color; None draws the layout's default (branch accent, theme edge or link).
    pub color: Option<String>,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Length {
    Pt(f64),
    /// Multiple of the body font, which is only known once the solver picks it.
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
    /// Shared center y.
    Row,
    /// Shared center x.
    Column,
}

impl Side {
    pub fn vertical(self) -> bool {
        matches!(self, Side::Top | Side::Bottom | Side::Row)
    }
}

/// Place is one cell of the page split into thirds; (column, row) with (0, 0) at top-left.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Place(pub u8, pub u8);

#[derive(Clone, PartialEq, Debug)]
pub enum Relation {
    /// a's right edge plus gap is at most b's left edge.
    LeftOf { a: usize, b: usize, gap: Length },
    /// a's bottom edge is at least gap above b's top edge.
    Above { a: usize, b: usize, gap: Length },
    /// The space between the two boxes is at most `within`.
    Near { a: usize, b: usize, within: Length },
    Align { side: Side, items: Vec<usize> },
    At { a: usize, place: Place },
    /// A frame, with the title on top, around these nodes and no others, on the page.
    /// `title_em` is the title's width in ems of the body font, measured once fonts are
    /// loaded (`layout::measure_groups`).
    Group { items: Vec<usize>, title: Option<String>, color: Option<String>, title_em: f64 },
}

impl Relation {
    pub fn nodes(&self) -> Vec<usize> {
        match self {
            Relation::LeftOf { a, b, .. } | Relation::Above { a, b, .. } | Relation::Near { a, b, .. } => vec![*a, *b],
            Relation::Align { items, .. } | Relation::Group { items, .. } => items.clone(),
            Relation::At { a, .. } => vec![*a],
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
    /// The page's orientation was given (a `portrait` or `landscape` flag, or `WxH`), so the
    /// layout may not turn it.
    pub page_fixed: bool,
    /// Most bands a layered flow may wrap into; 1 never wraps. None leaves it to the layout.
    pub bands: Option<usize>,
    pub margin: Option<f64>,
    pub font: Option<(f64, f64)>,
    /// Span of the `font` statement that set `font`.
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
