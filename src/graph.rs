use crate::diag::Span;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shape {
    Circle,
    Rect,
    /// Rhombus whose inscribed text rect is half its width and height.
    Diamond,
    /// Text with no frame, for margin annotations and lane labels.
    Plain,
}

/// Cell is one square of a node figure row: shaded, outlined, or an invisible gap.
#[derive(Clone, Debug)]
pub struct Cell {
    pub shade: f64,
    pub gap: bool,
    pub span: usize,
    pub label: Option<String>,
    /// Set by an `~ image "PATH"` row, which holds this one cell.
    pub image: Option<std::sync::Arc<Image>>,
}

/// Image is a PNG or JPEG drawn in a node, sized by its pixel aspect ratio.
#[derive(Debug, Default)]
pub struct Image {
    pub path: String,
    pub width_em: f64,
    pub span: Span,
    pub mime: &'static str,
    pub data: Vec<u8>,
    pub px: (u32, u32),
}

impl Image {
    /// decode reads the pixel size from a PNG or JPEG header and records the bytes.
    pub fn decode(&mut self, data: Vec<u8>) -> Result<(), &'static str> {
        let be16 = |b: &[u8]| u16::from_be_bytes([b[0], b[1]]) as u32;
        let be32 = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        let (mime, px) = if data.len() >= 24 && data.starts_with(b"\x89PNG\r\n\x1a\n") {
            ("image/png", (be32(&data[16..]), be32(&data[20..])))
        } else if data.starts_with(&[0xFF, 0xD8]) {
            let mut i = 2;
            let mut px = None;
            while i + 9 < data.len() && data[i] == 0xFF {
                let m = data[i + 1];
                let len = be16(&data[i + 2..]) as usize;
                // SOF markers carry the frame size; C4, C8, and CC are other tables.
                if (0xC0..=0xCF).contains(&m) && !matches!(m, 0xC4 | 0xC8 | 0xCC) {
                    px = Some((be16(&data[i + 7..]), be16(&data[i + 5..])));
                    break;
                }
                i += 2 + len;
            }
            ("image/jpeg", px.ok_or("JPEG has no frame header")?)
        } else {
            return Err("not a PNG or JPEG file");
        };
        if px.0 == 0 || px.1 == 0 {
            return Err("image has no pixels");
        }
        (self.mime, self.px, self.data) = (mime, px, data);
        Ok(())
    }
}

pub struct Node {
    pub id: String,
    pub title: String,
    pub body: Vec<String>,
    pub figure: Vec<Vec<Cell>>,
    pub color: Option<String>,
    pub fill: Option<String>,
    pub text: Option<String>,
    pub shape: Option<Shape>,
    pub stack: Option<u8>,
    /// Overrides the page-wide `font wrap=` for this node.
    pub wrap_em: Option<f64>,
    /// Set by `place=aside`: in a layered layout, sits in a strip after the flow.
    pub aside: bool,
    /// Set by `lane=ID`: hangs on ID's dashed lifeline and shares its lane.
    pub lane: Option<usize>,
    pub span: Span,
}

/// Head is the mark drawn at one end of an edge.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Head {
    Arrow,
    Triangle,
    Diamond,
    ODiamond,
    Crow,
    None,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Dash {
    Solid,
    Dashed,
    Dotted,
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
    /// Mark at `b`; unset means an arrow when directed.
    pub head: Option<Head>,
    /// Mark at `a`; unset means none.
    pub tail: Option<Head>,
    pub dash: Option<Dash>,
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
        dashed: bool,
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
    /// page_ratio is a `W:H` page shape whose size the layout picks.
    pub page_ratio: Option<(f64, f64)>,
    pub page_fixed: bool,
    pub bands: Option<usize>,
    /// fold forces the band count; ratio pages set it after comparing band counts.
    pub fold: Option<usize>,
    /// Set by `layout layered order=source`: siblings keep declaration order.
    pub source_order: bool,
    /// Set by `layout lanes`: groups and lifelines become side-by-side lanes.
    pub lanes: bool,
    /// Set by `layout venn`: groups become overlapping set circles.
    pub venn: bool,
    pub margin: Option<f64>,
    pub key_size: Option<f64>,
    pub font: Option<(f64, f64)>,
    pub font_span: Option<Span>,
    pub wrap_em: Option<f64>,
    pub family: Option<String>,
    pub theme: Option<String>,
    pub theme_over: crate::theme::Overrides,
}

pub struct KeyEntry {
    pub label: String,
    pub color: Option<String>,
    pub fill: Option<String>,
    pub shape: Option<Shape>,
    /// Set for a line swatch.
    pub line: Option<Dash>,
    pub head: Option<Head>,
    /// Set when the entry's styles only frame groups, so its swatch is an open frame.
    pub frame: bool,
}

/// VennSet is one circle of a `layout venn` diagram, made from a top-level group.
pub struct VennSet {
    pub items: Vec<usize>,
    pub title: Option<String>,
    pub color: Option<String>,
    pub span: Span,
}

pub struct Graph {
    pub key: Vec<KeyEntry>,
    pub title: Option<String>,
    pub root: Option<usize>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub constraints: Vec<Constraint>,
    pub sets: Vec<VennSet>,
    pub settings: Settings,
}

#[cfg(test)]
mod tests {
    use super::Image;

    #[test]
    fn decode_reads_png_and_jpeg_sizes() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend([0, 0, 1, 64, 0, 0, 0, 200]);
        let mut im = Image::default();
        im.decode(png).unwrap();
        assert_eq!((im.mime, im.px), ("image/png", (320, 200)));
        // SOI, an APP0 segment, then SOF0 with height 30 and width 40.
        let jpg = vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC0, 0, 11, 8, 0, 30, 0, 40, 1, 0, 0, 0,
        ];
        im.decode(jpg).unwrap();
        assert_eq!((im.mime, im.px), ("image/jpeg", (40, 30)));
        assert!(im.decode(b"GIF89a".to_vec()).is_err());
    }
}
