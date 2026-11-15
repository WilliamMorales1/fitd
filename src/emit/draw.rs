//! draw: one pass that decides what every backend paints, and in what order.

use super::{decor, edge_points, groups, paints, stroke, venn_colors};
use crate::geom::{P, Rect};
use crate::graph::{Dash, Graph, Head, Image, KeyEntry};
use crate::key::KeyPlan;
use crate::layout::{EdgeStyle, FRAME_PAD, Layout, Role, Shape, TITLE_BAND, TITLE_SIZE, TextLine};
use crate::theme::Theme;
use crate::tree::Tree;

/// Prim is one painted item in layout coordinates, y up.
pub enum Prim<'a> {
    Page(Rect),
    Frame {
        r: Rect,
        color: String,
        opacity: f64,
        width: f64,
        radius: f64,
        dash: Option<(f64, f64)>,
    },
    /// Bold text: a frame title anchored at its baseline start, or a set title at its centre.
    Title {
        at: P,
        text: &'a str,
        size: f64,
        color: String,
        center: bool,
    },
    Disc {
        c: P,
        r: f64,
        color: String,
        opacity: f64,
        width: f64,
    },
    Edge {
        pts: Vec<P>,
        color: String,
        width: f64,
        start: Head,
        end: Head,
        dash: Dash,
        faint: bool,
    },
    /// Node outline; `under` is the fill opacity over an opaque background copy.
    Body {
        shape: Shape,
        r: Rect,
        fill: String,
        under: Option<f64>,
        stroke: Option<(String, f64)>,
        radius: f64,
    },
    Image {
        r: Rect,
        im: &'a Image,
    },
    Cell {
        r: Rect,
        color: String,
        shade: f64,
        width: f64,
        radius: f64,
    },
    Text {
        line: &'a TextLine,
        at: P,
        color: String,
    },
    Label {
        r: Rect,
        text: &'a str,
        size: f64,
    },
    Key {
        entry: &'a KeyEntry,
        at: P,
        size: f64,
    },
}

impl Prim<'_> {
    /// colors lists every color the item paints with.
    pub fn colors(&self) -> Vec<&str> {
        match self {
            Prim::Page(_) | Prim::Image { .. } | Prim::Label { .. } => vec![],
            Prim::Frame { color, .. }
            | Prim::Title { color, .. }
            | Prim::Disc { color, .. }
            | Prim::Edge { color, .. }
            | Prim::Cell { color, .. }
            | Prim::Text { color, .. } => vec![color],
            Prim::Body { fill, stroke, .. } => {
                let mut v = vec![fill.as_str()];
                v.extend(stroke.as_ref().map(|s| s.0.as_str()));
                v
            }
            Prim::Key { entry, .. } => [entry.color.as_deref(), entry.fill.as_deref()]
                .into_iter()
                .flatten()
                .collect(),
        }
    }
}

/// draw lists the items of the figure in paint order on page `frame`.
pub fn draw<'a>(
    g: &'a Graph,
    t: &Tree,
    lay: &'a Layout,
    th: &'a Theme,
    key: Option<&KeyPlan>,
    frame: Rect,
) -> Vec<Prim<'a>> {
    let b = lay.body;
    let paint = paints(g, t, lay, th);
    let mut o = vec![Prim::Page(frame)];
    for f in groups(g, lay) {
        o.push(Prim::Frame {
            r: f.rect,
            color: f.color.unwrap_or(&th.edge).to_string(),
            opacity: th.tint / 2.0,
            width: 0.08 * b,
            radius: 0.5 * b,
            dash: f.dashed.then_some((0.7 * b, 0.5 * b)),
        });
        if let Some(text) = f.title {
            o.push(Prim::Title {
                at: (
                    f.rect.x0 + FRAME_PAD * b,
                    f.rect.y1 - (TITLE_BAND / 2.0 + 0.3) * b,
                ),
                text,
                size: TITLE_SIZE * b,
                color: f.color.unwrap_or(&th.label).to_string(),
                center: false,
            });
        }
    }
    if let Some(v) = &lay.venn {
        let colors = venn_colors(g, lay, th);
        for (c, col) in v.circles.iter().zip(&colors) {
            o.push(Prim::Disc {
                c: c.c,
                r: c.r,
                color: col.clone(),
                opacity: th.tint,
                width: 0.12 * b,
            });
        }
        for (t, col) in v.titles.iter().zip(&colors) {
            let Some(t) = t else { continue };
            o.push(Prim::Title {
                at: t.at,
                text: &t.text,
                size: t.size,
                color: col.clone(),
                center: true,
            });
        }
    }
    for e in &lay.edges {
        let (start, end, dash) = decor(g, e);
        let color = stroke(g, e)
            .unwrap_or(match e.style {
                EdgeStyle::Link => &th.link,
                EdgeStyle::Arrow => &th.edge,
                EdgeStyle::Line => &paint[e.b].accent,
            })
            .to_string();
        o.push(Prim::Edge {
            pts: edge_points(lay, e),
            color,
            width: e.width * b,
            start,
            end,
            dash,
            faint: e.style == EdgeStyle::Line,
        });
    }
    for (v, p) in paint.iter().enumerate() {
        let bx = &lay.boxes[v];
        let (cx, cy) = lay.pos[v];
        // A stacked node grows up and to the right: the front copy holds the content.
        let (bw, bh) = (bx.w - bx.shift, bx.h - bx.shift);
        let (x0, ytop) = (cx - bx.w / 2.0, cy + bx.h / 2.0 - bx.shift);
        let front = |d: f64| Rect {
            x0: x0 + d,
            y0: ytop + d - bh,
            x1: x0 + d + bw,
            y1: ytop + d,
        };
        if bx.shape != Shape::Plain {
            for k in (1..bx.stack).rev() {
                let d = bx.shift / (bx.stack - 1) as f64 * k as f64;
                o.push(Prim::Body {
                    shape: bx.shape,
                    r: front(d),
                    fill: th.background.clone(),
                    under: None,
                    stroke: Some((p.accent.clone(), 0.08 * b)),
                    radius: 0.35 * b,
                });
            }
        }
        let (under, line) = match (bx.shape, bx.role) {
            (Shape::Plain, _) => (None, None),
            (Shape::Circle, Role::Root) => (None, Some(0.12 * b)),
            (Shape::Circle, _) => (None, None),
            (Shape::Diamond | Shape::Rect, _) => (Some(p.opacity), Some(0.1 * b)),
        };
        if bx.shape != Shape::Plain {
            o.push(Prim::Body {
                shape: bx.shape,
                r: front(0.0),
                fill: p.fill.clone(),
                under,
                stroke: line.map(|w| (p.accent.clone(), w)),
                radius: 0.35 * b,
            });
        }
        let (left, top) = (cx - bx.w / 2.0, cy + bx.h / 2.0);
        let cell = |c: &crate::layout::CellBox| Rect {
            x0: left + c.x,
            y0: top - c.y - c.h,
            x1: left + c.x + c.w,
            y1: top - c.y,
        };
        for c in &bx.cells {
            if let Some(im) = &c.image {
                o.push(Prim::Image { r: cell(c), im });
                continue;
            }
            o.push(Prim::Cell {
                r: cell(c),
                color: p.accent.clone(),
                shade: c.shade,
                width: 0.07 * b,
                radius: 0.12 * b,
            });
        }
        for l in &bx.lines {
            o.push(Prim::Text {
                line: l,
                at: (left + l.x, top - l.baseline),
                color: p.text.clone(),
            });
        }
        for c in &bx.cells {
            if let Some(l) = &c.label {
                o.push(Prim::Text {
                    line: l,
                    at: (left + l.x, top - l.baseline),
                    color: if c.shade >= 0.55 {
                        &th.background
                    } else {
                        &p.text
                    }
                    .clone(),
                });
            }
        }
    }
    for e in &lay.edges {
        if let (Some(text), Some(r)) = (&e.label, lay.label_rect(e)) {
            o.push(Prim::Label {
                r,
                text,
                size: 0.8 * b,
            });
        }
    }
    if let Some(k) = key {
        let n = k.rows.len() as f64;
        let mid = (frame.x0 + frame.x1) / 2.0;
        for (r, row) in k.rows.iter().enumerate() {
            let cy = frame.y0 + k.pad + (n - r as f64 - 0.5) * k.row_h;
            for &(dx, i) in &row.items {
                o.push(Prim::Key {
                    entry: &g.key[i],
                    at: (mid - row.width / 2.0 + dx, cy),
                    size: k.size,
                });
            }
        }
    }
    o
}
