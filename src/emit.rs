use crate::geom::{P, Rect};
use crate::graph::{Dash, Dir, Graph, Head, KeyEntry, LayoutKind, Relation};
use crate::key::KeyPlan;
use crate::layout::{Drawn, EdgeStyle, Layout, Role, Shape, TextLine, frame};
use crate::metrics::{Fonts, Script};
use crate::theme::{self, Theme};
use crate::tree::Tree;
use std::collections::BTreeMap;
use std::fmt::Write;

mod draw;
use draw::{Prim, draw};

struct Paint {
    accent: String,
    fill: String,
    opacity: f64,
    text: String,
}

fn accents(g: &Graph, t: &Tree, th: &Theme, radial: bool) -> Vec<String> {
    let mut hub_color = BTreeMap::new();
    for (i, &h) in t.children[t.root].iter().enumerate() {
        hub_color.insert(
            h,
            g.nodes[h]
                .color
                .clone()
                .unwrap_or_else(|| th.palette[i % th.palette.len()].clone()),
        );
    }
    (0..g.nodes.len())
        .map(|v| match t.branch(v).filter(|_| radial) {
            Some(h) => g.nodes[v]
                .color
                .clone()
                .unwrap_or_else(|| hub_color[&h].clone()),
            None => g.nodes[v].color.clone().unwrap_or_else(|| th.text.clone()),
        })
        .collect()
}

fn paints(g: &Graph, t: &Tree, lay: &Layout, th: &Theme) -> Vec<Paint> {
    let acc = accents(g, t, th, lay.kind == LayoutKind::Radial);
    (0..lay.pos.len())
        .map(|v| {
            let n = &g.nodes[v];
            let (fill, opacity, auto) = match (lay.boxes[v].shape, lay.boxes[v].role, &n.fill) {
                (_, _, Some(f)) => (f.clone(), 1.0, theme::on(f).to_string()),
                (Shape::Circle, Role::Root, None) => {
                    (th.root.clone(), 1.0, theme::on(&th.root).to_string())
                }
                (Shape::Circle, _, None) => (acc[v].clone(), 1.0, theme::on(&acc[v]).to_string()),
                (Shape::Rect | Shape::Diamond, _, None) => {
                    (acc[v].clone(), th.tint, th.text.clone())
                }
                (Shape::Plain, _, None) => (th.background.clone(), 0.0, th.text.clone()),
            };
            let accent = if lay.boxes[v].role == Role::Root && n.color.is_none() {
                th.text.clone()
            } else {
                acc[v].clone()
            };
            Paint {
                accent,
                fill,
                opacity,
                text: n.text.clone().unwrap_or(auto),
            }
        })
        .collect()
}

/// rhombus gives the corners of a diamond centred on (cx, cy), top first in SVG order.
fn rhombus(cx: f64, cy: f64, w: f64, h: f64) -> [P; 4] {
    [
        (cx, cy - h / 2.0),
        (cx + w / 2.0, cy),
        (cx, cy + h / 2.0),
        (cx - w / 2.0, cy),
    ]
}

fn svg_points(pts: &[P]) -> String {
    let s: Vec<String> = pts
        .iter()
        .map(|p| format!("{:.1},{:.1}", p.0, p.1))
        .collect();
    s.join(" ")
}

fn tikz_points(pts: &[P]) -> String {
    let s: Vec<String> = pts
        .iter()
        .map(|p| format!("({:.1},{:.1})", p.0, p.1))
        .collect();
    format!("{} -- cycle", s.join(" -- "))
}

fn edge_points(lay: &Layout, e: &Drawn) -> Vec<P> {
    match e.style {
        EdgeStyle::Link => lay.path(e),
        EdgeStyle::Arrow => {
            let (s, t) = lay.segment(e);
            vec![s, t]
        }
        EdgeStyle::Line if e.framed() => {
            let (s, t) = lay.segment(e);
            vec![s, t]
        }
        EdgeStyle::Line => vec![lay.pos[e.a], lay.pos[e.b]],
    }
}

fn stroke<'a>(g: &'a Graph, e: &Drawn) -> Option<&'a str> {
    let find = |a: usize, b: usize| g.edges.iter().find(|x| (x.a, x.b) == (a, b));
    find(e.a, e.b)
        .or_else(|| find(e.b, e.a))
        .and_then(|x| x.color.as_deref())
}

/// Marks at the drawn start and end of `e`, and its dash pattern.
fn decor(g: &Graph, e: &Drawn) -> (Head, Head, Dash) {
    let find = |a: usize, b: usize| g.edges.iter().find(|x| (x.a, x.b) == (a, b));
    let (x, flip) = match find(e.a, e.b) {
        Some(x) => (Some(x), false),
        None => (find(e.b, e.a), true),
    };
    let arrow = e.style != EdgeStyle::Line && x.is_none_or(|x| x.directed);
    let head = x
        .and_then(|x| x.head)
        .unwrap_or(if arrow { Head::Arrow } else { Head::None });
    let tail = x.and_then(|x| x.tail).unwrap_or(Head::None);
    let dash = x
        .and_then(|x| x.dash)
        .unwrap_or(if e.style == EdgeStyle::Link {
            Dash::Dashed
        } else {
            Dash::Solid
        });
    if flip {
        (head, tail, dash)
    } else {
        (tail, head, dash)
    }
}

/// SVG path in a 10x10 box pointing at +x, its tip x, and how it is painted.
fn head_path(h: Head) -> (&'static str, f64, Ink) {
    match h {
        Head::Arrow => ("M0,0 L10,5 L0,10 z", 9.0, Ink::Fill),
        Head::Triangle => ("M1,1 L9,5 L1,9 z", 9.0, Ink::Open),
        Head::Diamond => ("M0,5 L5,2 L10,5 L5,8 z", 10.0, Ink::Fill),
        Head::ODiamond => ("M1,5 L5.5,2.5 L10,5 L5.5,7.5 z", 10.0, Ink::Open),
        Head::Crow => ("M0,5 L10,0 M0,5 L10,5 M0,5 L10,10", 10.0, Ink::Stroke),
        Head::None => ("", 0.0, Ink::Stroke),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Ink {
    Fill,
    Open,
    Stroke,
}

fn head_svg(h: Head, c: &str, bg: &str) -> String {
    let (d, _, paint) = head_path(h);
    match paint {
        Ink::Fill => format!(r#"<path d="{d}" fill="{c}"/>"#),
        Ink::Open => format!(
            r#"<path d="{d}" fill="{bg}" stroke="{c}" stroke-width="1.5" stroke-linejoin="round"/>"#
        ),
        Ink::Stroke => format!(r#"<path d="{d}" fill="none" stroke="{c}" stroke-width="1.5"/>"#),
    }
}

fn marker_id(h: Head, c: &str) -> String {
    let k = ["a", "t", "d", "o", "c", "n"][h as usize];
    format!("m{k}{}", theme::rgb(c))
}

fn svg_dash(d: Dash, sw: f64, b: f64) -> String {
    match d {
        Dash::Solid => String::new(),
        Dash::Dashed => format!(r#" stroke-dasharray="{:.1} {:.1}""#, 0.5 * b, 0.3 * b),
        Dash::Dotted => format!(
            r#" stroke-dasharray="0 {:.2}" stroke-linecap="round""#,
            (2.5 * sw).max(0.25 * b)
        ),
    }
}

fn tikz_tip(h: Head) -> &'static str {
    match h {
        Head::Arrow => "{Stealth}",
        Head::Triangle => "{Triangle[open]}",
        Head::Diamond => "{Diamond}",
        Head::ODiamond => "{Diamond[open]}",
        Head::Crow => "{Straight Barb[reversed]}",
        Head::None => "",
    }
}

fn tikz_decor(start: Head, end: Head, dash: Dash) -> String {
    let mut s = String::new();
    if start != Head::None || end != Head::None {
        s += &format!(", {}-{}", tikz_tip(start), tikz_tip(end));
    }
    s += match dash {
        Dash::Solid => "",
        Dash::Dashed => ", dashed",
        Dash::Dotted => ", dotted",
    };
    s
}

struct Group<'a> {
    rect: Rect,
    title: Option<&'a str>,
    color: Option<&'a str>,
    dashed: bool,
}

fn groups<'a>(g: &'a Graph, lay: &Layout) -> Vec<Group<'a>> {
    let mut rels: Vec<&Relation> = g
        .constraints
        .iter()
        .map(|c| &c.rel)
        .filter(|r| matches!(r, Relation::Group { .. }))
        .collect();
    rels.sort_by_key(|r| std::cmp::Reverse(r.depth()));
    let mut out: Vec<Group> = rels
        .iter()
        .map(|&r| {
            let Relation::Group {
                title,
                color,
                dashed,
                ..
            } = r
            else {
                unreachable!()
            };
            Group {
                rect: frame(r, &|v| lay.rect(v), lay.body),
                title: title.as_deref(),
                color: color.as_deref(),
                dashed: *dashed,
            }
        })
        .collect();
    if let (true, Some(LayoutKind::Layered(dir))) = (g.settings.lanes, g.settings.layout) {
        stretch_lanes(&rels, &mut out, dir);
    }
    out
}

/// stretch_lanes gives every outermost lane frame the same extent along the flow.
fn stretch_lanes(rels: &[&Relation], out: &mut [Group], dir: Dir) {
    let items = |r: &Relation| match r {
        Relation::Group { items, .. } => items.clone(),
        _ => Vec::new(),
    };
    let top: Vec<usize> = (0..rels.len())
        .filter(|&i| {
            let a = items(rels[i]);
            !rels.iter().enumerate().any(|(j, r)| {
                let b = items(r);
                j != i && b.len() > a.len() && a.iter().all(|v| b.contains(v))
            })
        })
        .collect();
    let down = matches!(dir, Dir::Down);
    let span = |r: &Rect| if down { (r.y0, r.y1) } else { (r.x0, r.x1) };
    let (lo, hi) = top.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &i| {
        let (a, b) = span(&out[i].rect);
        (lo.min(a), hi.max(b))
    });
    for &i in &top {
        let r = &mut out[i].rect;
        if down {
            (r.y0, r.y1) = (lo, hi);
        } else {
            (r.x0, r.x1) = (lo, hi);
        }
    }
}

/// venn_colors gives each set circle its own color, or the palette's in turn.
fn venn_colors(g: &Graph, lay: &Layout, th: &Theme) -> Vec<String> {
    let n = lay.venn.as_ref().map_or(0, |v| v.circles.len());
    (0..n)
        .map(|i| {
            g.sets
                .get(i)
                .and_then(|s| s.color.clone())
                .unwrap_or_else(|| th.palette[i % th.palette.len()].clone())
        })
        .collect()
}

pub fn bounds(g: &Graph, lay: &Layout) -> Rect {
    let mut r = Rect {
        x0: f64::MAX,
        y0: f64::MAX,
        x1: f64::MIN,
        y1: f64::MIN,
    };
    let mut add = |b: Rect| {
        r = Rect {
            x0: r.x0.min(b.x0),
            y0: r.y0.min(b.y0),
            x1: r.x1.max(b.x1),
            y1: r.y1.max(b.y1),
        }
    };
    for (v, bx) in lay.boxes.iter().enumerate() {
        add(Rect::centered(lay.pos[v], bx.w, bx.h));
    }
    for e in &lay.edges {
        for p in edge_points(lay, e) {
            add(Rect::centered(p, 0.0, 0.0));
        }
        if let Some(l) = lay.label_rect(e) {
            add(l);
        }
    }
    for f in groups(g, lay) {
        add(f.rect);
    }
    if let Some(v) = &lay.venn {
        for c in &v.circles {
            add(Rect::centered(c.c, 2.0 * c.r, 2.0 * c.r));
        }
        for t in v.titles.iter().flatten() {
            add(t.rect);
        }
    }
    r
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn svg_line(out: &mut String, fonts: &Fonts, l: &TextLine, x: f64, y: f64, fill: &str) {
    let _ = write!(
        out,
        r#"<text x="{x:.2}" y="{y:.2}" font-size="{:.2}" fill="{fill}" xml:space="preserve">"#,
        l.size
    );
    let mut shift = 0.0;
    for r in &l.runs {
        let f = fonts.font(r.bold, r.font);
        let target = -r.script.shift() * l.size;
        let _ = write!(
            out,
            r#"<tspan dy="{:.2}" font-family="{}" font-weight="{}" font-size="{:.2}">{}</tspan>"#,
            target - shift,
            xml(&f.family),
            if f.bold { "bold" } else { "normal" },
            l.size * r.script.scale(),
            xml(&r.text)
        );
        shift = target;
    }
    out.push_str("</text>\n");
}

/// View for bitmap output: layout bounds plus margin, grown to hold the key below.
pub fn png_view(g: &Graph, lay: &Layout, key: Option<&KeyPlan>, margin: f64) -> Rect {
    let mut v = bounds(g, lay).inflate(margin);
    if let Some(k) = key {
        v.y0 -= k.height();
        let w = k.width() + 2.0 * margin;
        let c = (v.x0 + v.x1) / 2.0;
        v.x0 = v.x0.min(c - w / 2.0);
        v.x1 = v.x1.max(c + w / 2.0);
    }
    v
}

fn key_swatch(e: &KeyEntry, th: &Theme, x: f64, y: f64, s: f64) -> String {
    let c = e.color.as_deref().unwrap_or(&th.edge);
    if let Some(d) = e.line {
        let dash = match d {
            Dash::Solid => String::new(),
            Dash::Dashed => format!(r#" stroke-dasharray="{:.2} {:.2}""#, s * 0.3, s * 0.2),
            Dash::Dotted => format!(
                r#" stroke-dasharray="0 {:.2}" stroke-linecap="round""#,
                s * 0.4
            ),
        };
        let mut out = format!(
            r#"<line x1="{x:.2}" y1="{y:.2}" x2="{:.2}" y2="{y:.2}" stroke="{c}" stroke-width="{:.2}"{dash}/>"#,
            x + s,
            s / 5.0
        );
        if let Some(h) = e.head.filter(|&h| h != Head::None) {
            let k = s * 0.09;
            let _ = write!(
                out,
                r#"<g transform="translate({:.2},{:.2}) scale({k:.3}) translate({:.1},-5)">{}</g>"#,
                x + s,
                y,
                -head_path(h).1,
                head_svg(h, c, &th.background)
            );
        }
        return out;
    }
    if e.frame {
        return format!(
            r#"<rect x="{x:.2}" y="{:.2}" width="{s:.2}" height="{s:.2}" rx="{:.2}" fill="{c}" fill-opacity="{:.2}" stroke="{c}" stroke-width="1"/>"#,
            y - s / 2.0,
            s * 0.35,
            th.tint / 2.0
        );
    }
    match (e.shape, &e.fill) {
        (_, Some(f)) if e.shape == Some(Shape::Circle) => format!(
            r#"<circle cx="{:.2}" cy="{:.2}" r="{:.2}" fill="{f}" stroke="{c}" stroke-width="1"/>"#,
            x + s / 2.0,
            y,
            s / 2.0
        ),
        (Some(Shape::Circle), None) => format!(
            r#"<circle cx="{:.2}" cy="{:.2}" r="{:.2}" fill="{c}" stroke="{c}" stroke-width="1"/>"#,
            x + s / 2.0,
            y,
            s / 2.0
        ),
        (Some(Shape::Diamond), f) => {
            let (fill, op) = f.as_deref().map_or((c, th.tint), |f| (f, 1.0));
            format!(
                r#"<polygon points="{}" fill="{fill}" fill-opacity="{op:.2}" stroke="{c}" stroke-width="1" stroke-linejoin="round"/>"#,
                svg_points(&rhombus(x + s / 2.0, y, s, s))
            )
        }
        (_, f) => {
            let (fill, op) = f.as_deref().map_or((c, th.tint), |f| (f, 1.0));
            format!(
                r#"<rect x="{x:.2}" y="{:.2}" width="{s:.2}" height="{s:.2}" rx="{:.2}" fill="{fill}" fill-opacity="{op:.2}" stroke="{c}" stroke-width="1"/>"#,
                y - s / 2.0,
                s * 0.15
            )
        }
    }
}

pub fn svg(
    g: &Graph,
    t: &Tree,
    fonts: &Fonts,
    lay: &Layout,
    view: Rect,
    th: &Theme,
    key: Option<&KeyPlan>,
) -> String {
    let prims = draw(g, t, lay, th, key, view);
    let b = lay.body;
    let (w, h) = (view.x1 - view.x0, view.y1 - view.y0);
    let tx = |p: (f64, f64)| (p.0 - view.x0, view.y1 - p.1);
    let corners = |r: &Rect| (tx((r.x0, r.y1)), tx((r.x1, r.y0)));
    let family = xml(&fonts.regular[0].family);
    let mut o = String::new();
    let _ = writeln!(
        o,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}pt" height="{h}pt" viewBox="0 0 {w} {h}">"#
    );
    if let Some(title) = &g.title {
        let _ = writeln!(o, "<title>{}</title>", xml(title));
    }
    let _ = writeln!(
        o,
        "<!-- fitd fonts: {} -->",
        fonts.fingerprint().replace("--", "- -")
    );
    o.push_str("<defs>");
    let mut markers: Vec<String> = vec![];
    for p in &prims {
        let Prim::Edge {
            start, end, color, ..
        } = p
        else {
            continue;
        };
        for &h in [start, end] {
            let id = marker_id(h, color);
            if h == Head::None || markers.contains(&id) {
                continue;
            }
            // UML and ER marks carry meaning in their outline, so they draw larger than arrows.
            let m = if h == Head::Arrow { 6 } else { 9 };
            let _ = write!(
                o,
                r#"<marker id="{id}" viewBox="0 0 10 10" refX="{}" refY="5" markerWidth="{m}" markerHeight="{m}" orient="auto-start-reverse">{}</marker>"#,
                head_path(h).1,
                head_svg(h, color, &th.background)
            );
            markers.push(id);
        }
    }
    o.push_str("</defs>\n");
    for p in &prims {
        match p {
            Prim::Page(_) => match &th.gradient {
                Some(end) => {
                    let _ = writeln!(
                        o,
                        r#"<defs><linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{}"/><stop offset="1" stop-color="{end}"/></linearGradient></defs>"#,
                        th.background
                    );
                    let _ = writeln!(o, r#"<rect width="{w}" height="{h}" fill="url(#bg)"/>"#);
                }
                None => {
                    let _ = writeln!(
                        o,
                        r#"<rect width="{w}" height="{h}" fill="{}"/>"#,
                        th.background
                    );
                }
            },
            Prim::Frame {
                r,
                color: c,
                opacity,
                width,
                radius,
                dash,
            } => {
                let ((x0, y0), (x1, y1)) = corners(r);
                let dash = dash.map_or(String::new(), |(on, off)| {
                    format!(r#" stroke-dasharray="{on:.1} {off:.1}""#)
                });
                let _ = writeln!(
                    o,
                    r#"<rect x="{x0:.1}" y="{y0:.1}" width="{:.1}" height="{:.1}" rx="{radius:.1}" fill="{c}" fill-opacity="{opacity:.2}" stroke="{c}" stroke-width="{width:.2}"{dash}/>"#,
                    x1 - x0,
                    y1 - y0,
                );
            }
            Prim::Title {
                at,
                text,
                size,
                color,
                center,
            } => {
                let (x, y) = match center {
                    true => tx((at.0, at.1 - 0.35 * size)),
                    false => tx(*at),
                };
                let anchor = if *center {
                    r#" text-anchor="middle""#
                } else {
                    ""
                };
                let _ = writeln!(
                    o,
                    r#"<text x="{x:.1}" y="{y:.1}" font-size="{size:.2}" font-family="{family}" font-weight="bold"{anchor} fill="{color}">{}</text>"#,
                    xml(text)
                );
            }
            Prim::Disc {
                c,
                r,
                color,
                opacity,
                width,
            } => {
                let (x, y) = tx(*c);
                let _ = writeln!(
                    o,
                    r#"<circle cx="{x:.1}" cy="{y:.1}" r="{r:.1}" fill="{color}" fill-opacity="{opacity:.2}" stroke="{color}" stroke-width="{width:.2}"/>"#
                );
            }
            Prim::Edge {
                pts,
                color: c,
                width: sw,
                start,
                end,
                dash,
                faint,
            } => {
                let d: Vec<String> = pts
                    .iter()
                    .map(|&p| tx(p))
                    .map(|(x, y)| format!("{x:.1},{y:.1}"))
                    .collect();
                let mut marks = String::new();
                for (h, at) in [(start, "start"), (end, "end")] {
                    if *h != Head::None {
                        let _ = write!(marks, r#" marker-{at}="url(#{})""#, marker_id(*h, c));
                    }
                }
                let opacity = if *faint {
                    r#" stroke-opacity="0.7""#
                } else {
                    ""
                };
                let _ = writeln!(
                    o,
                    r#"<polyline points="{}" fill="none" stroke="{c}"{opacity} stroke-width="{sw:.2}"{}{marks}/>"#,
                    d.join(" "),
                    svg_dash(*dash, *sw, b)
                );
            }
            Prim::Body {
                shape,
                r,
                fill,
                under,
                stroke,
                radius,
            } => {
                let ((x, y), (x1, y1)) = corners(r);
                let (bw, bh) = (x1 - x, y1 - y);
                let (cx, cy) = (x + bw / 2.0, y + bh / 2.0);
                let line = stroke.as_ref().map_or(String::new(), |(c, sw)| {
                    format!(r#" stroke="{c}" stroke-width="{sw:.2}""#)
                });
                let paint = match under {
                    Some(op) => format!(r#" fill="{fill}" fill-opacity="{op}"{line}"#),
                    None => format!(r#" fill="{fill}"{line}"#),
                };
                let under = |shape: &str| {
                    if under.is_some() {
                        format!("{shape} fill=\"{}\"/>\n", th.background)
                    } else {
                        String::new()
                    }
                };
                match shape {
                    Shape::Plain => {}
                    Shape::Circle => {
                        let _ = writeln!(
                            o,
                            r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{:.1}"{paint}/>"#,
                            bw / 2.0
                        );
                    }
                    Shape::Diamond => {
                        let pts = svg_points(&rhombus(cx, cy, bw, bh));
                        o += &under(&format!(r#"<polygon points="{pts}""#));
                        let _ = writeln!(
                            o,
                            r#"<polygon points="{pts}"{paint} stroke-linejoin="round"/>"#
                        );
                    }
                    Shape::Rect => {
                        let at = format!(
                            r#"<rect x="{x:.1}" y="{y:.1}" width="{bw:.1}" height="{bh:.1}" rx="{radius:.1}""#
                        );
                        o += &under(&at);
                        let _ = writeln!(o, "{at}{paint}/>");
                    }
                }
            }
            Prim::Image { r, im } => {
                let ((x, y), (x1, y1)) = corners(r);
                let _ = writeln!(
                    o,
                    r#"<image x="{x:.1}" y="{y:.1}" width="{:.1}" height="{:.1}" preserveAspectRatio="none" href="data:{};base64,{}"/>"#,
                    x1 - x,
                    y1 - y,
                    im.mime,
                    base64(&im.data)
                );
            }
            Prim::Cell {
                r,
                color,
                shade,
                width,
                radius,
            } => {
                let ((x, y), (x1, y1)) = corners(r);
                let at = format!(
                    r#"<rect x="{x:.1}" y="{y:.1}" width="{:.1}" height="{:.1}" rx="{radius:.1}""#,
                    x1 - x,
                    y1 - y
                );
                if *shade > 0.0 {
                    let _ = writeln!(o, r#"{at} fill="{color}" fill-opacity="{shade:.2}"/>"#);
                }
                let _ = writeln!(
                    o,
                    r#"{at} fill="none" stroke="{color}" stroke-width="{width:.2}"/>"#
                );
            }
            Prim::Text { line, at, color } => {
                let (x, y) = tx(*at);
                svg_line(&mut o, fonts, line, x, y, color);
            }
            Prim::Label { r, text, size } => {
                let ((x0, y0), (x1, y1)) = corners(r);
                let _ = writeln!(
                    o,
                    r#"<rect x="{x0:.1}" y="{y0:.1}" width="{:.1}" height="{:.1}" fill="{}" fill-opacity="0.9"/>"#,
                    x1 - x0,
                    y1 - y0,
                    th.background
                );
                let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0 + 0.35 * size);
                let _ = writeln!(
                    o,
                    r#"<text x="{cx:.1}" y="{cy:.1}" font-size="{size:.2}" font-family="{family}" font-style="italic" fill="{}" text-anchor="middle">{}</text>"#,
                    th.label,
                    xml(text)
                );
            }
            Prim::Key { entry, at, size } => {
                let (x, y) = tx(*at);
                o.push_str(&key_swatch(entry, th, x, y, *size));
                o.push('\n');
                let _ = writeln!(
                    o,
                    r#"<text x="{:.2}" y="{:.2}" font-size="{size}" font-family="{family}" fill="{}">{}</text>"#,
                    x + 1.5 * size,
                    y + 0.35 * size,
                    th.text,
                    xml(&entry.label)
                );
            }
        }
    }
    o.push_str("</svg>\n");
    o
}

const MAX_PIXELS: u64 = 256_000_000;

pub fn png(svg: &str, fonts: &Fonts, scale: f64) -> Result<Vec<u8>, String> {
    let mut opt = resvg::usvg::Options::default();
    for f in fonts.regular.iter().chain(&fonts.bold) {
        opt.fontdb_mut().load_font_data(f.data.clone());
    }
    let tree = resvg::usvg::Tree::from_str(svg, &opt).map_err(|e| e.to_string())?;
    let size = tree.size();
    let k = (scale * 0.75) as f32;
    let (pw, ph) = (
        (size.width() * k).ceil() as u32,
        (size.height() * k).ceil() as u32,
    );
    if pw as u64 * ph as u64 > MAX_PIXELS {
        return Err(format!(
            "image too large: {pw}x{ph}px (limit {} Mpx); lower --scale",
            MAX_PIXELS / 1_000_000
        ));
    }
    let mut pix =
        resvg::tiny_skia::Pixmap::new(pw, ph).ok_or(format!("image too large: {pw}x{ph}px"))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(k, k),
        &mut pix.as_mut(),
    );
    pix.encode_png().map_err(|e| e.to_string())
}

pub struct Sheet<'a> {
    pub size: Option<(f64, f64)>,
    pub cells: Vec<(&'a str, &'a Fonts, f64, f64)>,
}

pub fn pdf(sheets: &[Sheet]) -> Result<Vec<u8>, String> {
    use krilla_svg::SurfaceExt;
    let mut doc = krilla::Document::new();
    for sheet in sheets {
        let mut trees = Vec::new();
        for &(svg, fonts, x, y) in &sheet.cells {
            let mut opt = usvg_pdf::Options::default();
            for f in fonts.regular.iter().chain(&fonts.bold) {
                opt.fontdb_mut().load_font_data(f.data.clone());
            }
            let tree = usvg_pdf::Tree::from_str(svg, &opt).map_err(|e| e.to_string())?;
            let size = krilla::geom::Size::from_wh(
                tree.size().width() * 0.75,
                tree.size().height() * 0.75,
            )
            .ok_or("empty drawing")?;
            trees.push((tree, size, x as f32, y as f32));
        }
        let page_size = match sheet.size {
            Some((w, h)) => krilla::geom::Size::from_wh(w as f32, h as f32).ok_or("empty sheet")?,
            None => trees.first().map(|t| t.1).ok_or("empty sheet")?,
        };
        let mut page = doc.start_page_with(krilla::page::PageSettings::new(page_size));
        let mut surface = page.surface();
        for (tree, size, x, y) in &trees {
            surface.push_transform(&krilla::geom::Transform::from_translate(*x, *y));
            surface
                .draw_svg(tree, *size, krilla_svg::SvgSettings::default())
                .ok_or("could not draw the SVG")?;
            surface.pop();
        }
        surface.finish();
        page.finish();
    }
    doc.finish().map_err(|e| format!("{e:?}"))
}

fn tex(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '#' | '%' | '&' | '$' | '_' | '{' | '}' => {
                o.push('\\');
                o.push(c);
            }
            '~' => o.push_str("\\textasciitilde{}"),
            '^' => o.push_str("\\textasciicircum{}"),
            '\\' => o.push_str("\\textbackslash{}"),
            _ => o.push(c),
        }
    }
    o
}

fn cname(c: &str) -> String {
    format!("c{}", theme::rgb(c))
}

fn tikz_line(o: &mut String, l: &TextLine, x: f64, y: f64, color: &str) {
    let mut body = String::new();
    for r in &l.runs {
        let t = match r.bold == l.bold {
            true => tex(&r.text),
            false if r.bold => format!("{{\\bfseries {}}}", tex(&r.text)),
            false => format!("{{\\mdseries {}}}", tex(&r.text)),
        };
        if r.script == Script::Normal {
            body += &t;
        } else {
            let s = l.size * r.script.scale();
            let _ = write!(
                body,
                "\\raisebox{{{:.2}bp}}{{\\fontsize{{{s:.2}bp}}{{{s:.2}bp}}\\selectfont {t}}}",
                r.script.shift() * l.size
            );
        }
    }
    let bf = if l.bold { "\\bfseries" } else { "" };
    let _ = writeln!(
        o,
        "\\node[anchor=base west, inner sep=0bp, text={color}] at ({x:.2},{y:.2}) {{\\fontsize{{{s:.2}bp}}{{{s:.2}bp}}\\selectfont{bf} {body}}};",
        s = l.size
    );
}

#[allow(clippy::too_many_arguments)]
pub fn tikz(
    g: &Graph,
    t: &Tree,
    fonts: &Fonts,
    lay: &Layout,
    w: f64,
    h: f64,
    th: &Theme,
    key: Option<&KeyPlan>,
) -> String {
    let page = Rect {
        x0: -w / 2.0,
        y0: -h / 2.0,
        x1: w / 2.0,
        y1: h / 2.0,
    };
    let prims = draw(g, t, lay, th, key, page);
    let mut fallback: BTreeMap<char, String> = BTreeMap::new();
    for bx in &lay.boxes {
        for l in &bx.lines {
            for r in l.runs.iter().filter(|r| r.font > 0) {
                for c in r.text.chars() {
                    fallback
                        .entry(c)
                        .or_insert_with(|| fonts.font(r.bold, r.font).family.clone());
                }
            }
        }
    }
    let families: Vec<String> = fallback
        .values()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut o = String::new();
    o += "% Generated by fitd; edit the .fitd source instead.\n\\documentclass[border=0bp]{standalone}\n\\usepackage{fontspec}\n";
    let _ = writeln!(
        o,
        "\\setmainfont{{{}}}\n\\usepackage{{newunicodechar}}",
        fonts.regular[0].family
    );
    for (i, f) in families.iter().enumerate() {
        let _ = writeln!(
            o,
            "\\newfontfamily\\cmfb{}{{{f}}}",
            (b'A' + i as u8) as char
        );
    }
    for (c, f) in &fallback {
        let i = families.iter().position(|x| x == f).unwrap();
        let _ = writeln!(
            o,
            "\\newunicodechar{{{c}}}{{{{\\cmfb{} {c}}}}}",
            (b'A' + i as u8) as char
        );
    }
    o += "\\usepackage{tikz}\n\\usetikzlibrary{arrows.meta}\n";
    let mut defined = std::collections::BTreeSet::new();
    let used = [&th.background, &th.text, &th.edge, &th.link, &th.label]
        .into_iter()
        .map(String::as_str)
        .chain(th.gradient.as_deref())
        .chain(prims.iter().flat_map(Prim::colors));
    for c in used {
        if defined.insert(theme::rgb(c)) {
            let _ = writeln!(
                o,
                "\\definecolor{{{}}}{{HTML}}{{{}}}",
                cname(c),
                theme::rgb(c)
            );
        }
    }
    o += "\\begin{document}\n\\begin{tikzpicture}[x=1bp,y=1bp]\n";
    let bg = cname(&th.background);
    let text = |size: f64, style: &str, body: &str| {
        format!("{{\\fontsize{{{size:.2}bp}}{{{size:.2}bp}}\\selectfont{style} {body}}}")
    };
    for p in &prims {
        match p {
            Prim::Page(r) => {
                let (cmd, fill) = match &th.gradient {
                    Some(end) => (
                        "shade",
                        format!("top color={bg}, bottom color={}", cname(end)),
                    ),
                    None => ("fill", bg.clone()),
                };
                let _ = writeln!(
                    o,
                    "\\{cmd}[{fill}, opacity={:.3}] ({:.1},{:.1}) rectangle ({:.1},{:.1});",
                    theme::alpha(&th.background),
                    r.x0,
                    r.y0,
                    r.x1,
                    r.y1
                );
            }
            Prim::Frame {
                r,
                color,
                opacity,
                width,
                radius,
                dash,
            } => {
                let c = cname(color);
                let dash = dash.map_or(String::new(), |(on, off)| {
                    format!(", dash pattern=on {on:.1}bp off {off:.1}bp")
                });
                let _ = writeln!(
                    o,
                    "\\filldraw[fill={c}, fill opacity={opacity:.2}, draw={c}, line width={width:.2}bp, rounded corners={radius:.1}bp{dash}] ({:.1},{:.1}) rectangle ({:.1},{:.1});",
                    r.x0, r.y0, r.x1, r.y1
                );
            }
            Prim::Title {
                at,
                text: t,
                size,
                color,
                center,
            } => {
                let anchor = if *center { "" } else { "anchor=base west, " };
                let _ = writeln!(
                    o,
                    "\\node[{anchor}inner sep=0bp, text={}] at ({:.1},{:.1}) {};",
                    cname(color),
                    at.0,
                    at.1,
                    text(*size, "\\bfseries", &tex(t))
                );
            }
            Prim::Disc {
                c,
                r,
                color,
                opacity,
                width,
            } => {
                let k = cname(color);
                let _ = writeln!(
                    o,
                    "\\filldraw[fill={k}, fill opacity={opacity:.2}, draw={k}, line width={width:.2}bp] ({:.1},{:.1}) circle ({r:.1}bp);",
                    c.0, c.1
                );
            }
            Prim::Edge {
                pts,
                color,
                width,
                start,
                end,
                dash,
                faint,
            } => {
                let pts: Vec<String> = pts
                    .iter()
                    .map(|p| format!("({:.1},{:.1})", p.0, p.1))
                    .collect();
                let fade = if *faint { ", opacity=0.7" } else { "" };
                let _ = writeln!(
                    o,
                    "\\draw[{}{fade}{}, line width={width:.2}bp] {};",
                    cname(color),
                    tikz_decor(*start, *end, *dash),
                    pts.join(" -- ")
                );
            }
            Prim::Body {
                shape,
                r,
                fill,
                under,
                stroke,
                radius,
            } => {
                let (bw, bh) = (r.x1 - r.x0, r.y1 - r.y0);
                let (cx, cy) = (r.x0 + bw / 2.0, r.y1 - bh / 2.0);
                let path = match shape {
                    Shape::Plain => continue,
                    Shape::Circle => format!("({cx:.1},{cy:.1}) circle ({:.1})", bw / 2.0),
                    Shape::Diamond => tikz_points(&rhombus(cx, cy, bw, bh)),
                    Shape::Rect => format!(
                        "({:.1},{:.1}) rectangle ({:.1},{:.1})",
                        r.x0, r.y1, r.x1, r.y0
                    ),
                };
                let corner = match shape {
                    Shape::Rect => format!(", rounded corners={radius:.1}bp"),
                    Shape::Diamond => ", line join=round".into(),
                    _ => String::new(),
                };
                let f = cname(fill);
                let base = if under.is_some() { &bg } else { &f };
                match stroke {
                    Some((c, sw)) => {
                        let _ = writeln!(
                            o,
                            "\\filldraw[fill={base}, draw={}, line width={sw:.2}bp{corner}] {path};",
                            cname(c)
                        );
                    }
                    None => {
                        let _ = writeln!(o, "\\fill[{base}] {path};");
                    }
                }
                if let Some(op) = under {
                    let corner = if *shape == Shape::Rect { &corner } else { "" };
                    let _ = writeln!(o, "\\fill[{f}, opacity={op}{corner}] {path};");
                }
            }
            Prim::Image { r, im } => {
                let _ = writeln!(
                    o,
                    "\\node[anchor=north west, inner sep=0pt] at ({:.1},{:.1}) {{\\includegraphics[width={:.1}bp, height={:.1}bp]{{{}}}}};",
                    r.x0,
                    r.y1,
                    r.x1 - r.x0,
                    r.y1 - r.y0,
                    im.path
                );
            }
            Prim::Cell {
                r,
                color,
                shade,
                width,
                radius,
            } => {
                let c = cname(color);
                let rect = format!(
                    "rounded corners={radius:.1}bp] ({:.1},{:.1}) rectangle ({:.1},{:.1});",
                    r.x0, r.y1, r.x1, r.y0
                );
                if *shade > 0.0 {
                    let _ = writeln!(o, "\\fill[{c}, opacity={shade:.2}, {rect}");
                }
                let _ = writeln!(o, "\\draw[{c}, line width={width:.2}bp, {rect}");
            }
            Prim::Text { line, at, color } => tikz_line(&mut o, line, at.0, at.1, &cname(color)),
            Prim::Label { r, text: t, size } => {
                let _ = writeln!(
                    o,
                    "\\node[fill={bg}, fill opacity=0.9, text opacity=1, inner sep=1bp, text={}] at ({:.1},{:.1}) {};",
                    cname(&th.label),
                    (r.x0 + r.x1) / 2.0,
                    (r.y0 + r.y1) / 2.0,
                    text(*size, "\\itshape", &tex(t))
                );
            }
            Prim::Key { entry: e, at, size } => {
                let ((x, cy), s) = (*at, *size);
                let c = cname(e.color.as_deref().unwrap_or(&th.edge));
                let (fill, op) = match (&e.fill, e.shape) {
                    _ if e.frame => (c.clone(), th.tint / 2.0),
                    (Some(f), _) => (cname(f), 1.0),
                    (None, Some(Shape::Circle)) => (c.clone(), 1.0),
                    (None, _) => (c.clone(), th.tint),
                };
                if let Some(d) = e.line {
                    let dash = tikz_decor(Head::None, e.head.unwrap_or(Head::None), d);
                    let _ = writeln!(
                        o,
                        "\\draw[{c}{dash}, line width={:.2}bp] ({x:.2},{cy:.2}) -- ({:.2},{cy:.2});",
                        s / 5.0,
                        x + s
                    );
                } else if e.shape == Some(Shape::Diamond) && !e.frame {
                    let _ = writeln!(
                        o,
                        "\\filldraw[fill={fill}, fill opacity={op:.2}, draw={c}, line width=1bp, line join=round] {};",
                        tikz_points(&rhombus(x + s / 2.0, cy, s, s))
                    );
                } else if e.shape == Some(Shape::Circle) && !e.frame {
                    let _ = writeln!(
                        o,
                        "\\filldraw[fill={fill}, fill opacity={op:.2}, draw={c}, line width=1bp] ({:.2},{cy:.2}) circle ({:.2}bp);",
                        x + s / 2.0,
                        s / 2.0
                    );
                } else {
                    let _ = writeln!(
                        o,
                        "\\filldraw[fill={fill}, fill opacity={op:.2}, draw={c}, line width=1bp, rounded corners={:.2}bp] ({x:.2},{:.2}) rectangle ({:.2},{:.2});",
                        s * if e.frame { 0.35 } else { 0.15 },
                        cy - s / 2.0,
                        x + s,
                        cy + s / 2.0
                    );
                }
                let _ = writeln!(
                    o,
                    "\\node[anchor=base west, inner sep=0bp, text={}] at ({:.2},{:.2}) {};",
                    cname(&th.text),
                    x + 1.5 * s,
                    cy - 0.35 * s,
                    text(s, "", &tex(&e.label))
                );
            }
        }
    }
    o += "\\end{tikzpicture}\n\\end{document}\n";
    o
}

/// base64 encodes bytes for an SVG data URI.
fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for ch in data.chunks(3) {
        let n = ch
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            out.push(match i <= ch.len() {
                true => A[(n >> (18 - 6 * i) & 63) as usize] as char,
                false => '=',
            });
        }
    }
    out
}

#[cfg(test)]
mod base64_tests {
    #[test]
    fn base64_pads_like_rfc_4648() {
        let enc = |s: &str| super::base64(s.as_bytes());
        assert_eq!(
            [enc(""), enc("f"), enc("fo"), enc("foo"), enc("foobar")],
            ["", "Zg==", "Zm8=", "Zm9v", "Zm9vYmFy"]
        );
    }
}
