use crate::geom::{P, Rect};
use crate::graph::{Dash, Dir, Graph, Head, KeyEntry, LayoutKind, Relation};
use crate::key::KeyPlan;
use crate::layout::{
    Drawn, EdgeStyle, FRAME_PAD, Layout, Role, Shape, TITLE_BAND, TITLE_SIZE, TextLine, frame,
};
use crate::metrics::{Fonts, Script};
use crate::theme::{self, Theme};
use crate::tree::Tree;
use std::collections::BTreeMap;
use std::fmt::Write;

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

fn clipped(lay: &Layout, e: &Drawn) -> (P, P) {
    let (a, b) = (e.a, e.b);
    let (pa, pb) = lay.ends(e);
    let lerp = |t: f64| (pa.0 + t * (pb.0 - pa.0), pa.1 + t * (pb.1 - pa.1));
    let exit = |v: usize, mut inside: f64, mut outside: f64| {
        for _ in 0..40 {
            let mid = (inside + outside) / 2.0;
            if lay.inside(v, lerp(mid)) {
                inside = mid
            } else {
                outside = mid
            }
        }
        outside
    };
    (lerp(exit(a, 0.0, 1.0)), lerp(exit(b, 1.0, 0.0)))
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
            let (s, t) = clipped(lay, e);
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
        let f = fonts.font(l.bold, r.font);
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

fn svg_key(o: &mut String, g: &Graph, fonts: &Fonts, th: &Theme, k: &KeyPlan, w: f64, h: f64) {
    let bottom = h - k.pad;
    let n = k.rows.len() as f64;
    for (r, row) in k.rows.iter().enumerate() {
        let cy = bottom - (n - r as f64 - 0.5) * k.row_h;
        let x0 = (w - row.width) / 2.0;
        for &(dx, i) in &row.items {
            let e = &g.key[i];
            let x = x0 + dx;
            o.push_str(&key_swatch(e, th, x, cy, k.size));
            o.push('\n');
            let _ = writeln!(
                o,
                r#"<text x="{:.2}" y="{:.2}" font-size="{}" font-family="{}" fill="{}">{}</text>"#,
                x + 1.5 * k.size,
                cy + 0.35 * k.size,
                k.size,
                xml(&fonts.regular[0].family),
                th.text,
                xml(&e.label)
            );
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
    let paint = paints(g, t, lay, th);
    let b = lay.body;
    let (w, h) = (view.x1 - view.x0, view.y1 - view.y0);
    let tx = |p: (f64, f64)| (p.0 - view.x0, view.y1 - p.1);
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
    let edge_color = |e: &Drawn| {
        stroke(g, e).unwrap_or(match e.style {
            EdgeStyle::Link => &th.link,
            EdgeStyle::Arrow => &th.edge,
            EdgeStyle::Line => &paint[e.b].accent,
        })
    };
    let mut markers: Vec<String> = vec![];
    for e in &lay.edges {
        let (s, t, _) = decor(g, e);
        let c = edge_color(e);
        for h in [s, t] {
            let id = marker_id(h, c);
            if h == Head::None || markers.contains(&id) {
                continue;
            }
            // UML and ER marks carry meaning in their outline, so they draw larger than arrows.
            let m = if h == Head::Arrow { 6 } else { 9 };
            let _ = write!(
                o,
                r#"<marker id="{id}" viewBox="0 0 10 10" refX="{}" refY="5" markerWidth="{m}" markerHeight="{m}" orient="auto-start-reverse">{}</marker>"#,
                head_path(h).1,
                head_svg(h, c, &th.background)
            );
            markers.push(id);
        }
    }
    o.push_str("</defs>\n");
    if let Some(end) = &th.gradient {
        let _ = writeln!(
            o,
            r#"<defs><linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{}"/><stop offset="1" stop-color="{end}"/></linearGradient></defs>"#,
            th.background
        );
        let _ = writeln!(o, r#"<rect width="{w}" height="{h}" fill="url(#bg)"/>"#);
    } else {
        let _ = writeln!(
            o,
            r#"<rect width="{w}" height="{h}" fill="{}"/>"#,
            th.background
        );
    }
    for f in groups(g, lay) {
        let ((x0, y0), (x1, y1)) = (tx((f.rect.x0, f.rect.y1)), tx((f.rect.x1, f.rect.y0)));
        let c = f.color.unwrap_or(&th.edge);
        let dash = if f.dashed {
            format!(r#" stroke-dasharray="{:.1} {:.1}""#, 0.7 * b, 0.5 * b)
        } else {
            String::new()
        };
        let _ = writeln!(
            o,
            r#"<rect x="{x0:.1}" y="{y0:.1}" width="{:.1}" height="{:.1}" rx="{:.1}" fill="{c}" fill-opacity="{:.2}" stroke="{c}" stroke-width="{:.2}"{dash}/>"#,
            x1 - x0,
            y1 - y0,
            0.5 * b,
            th.tint / 2.0,
            0.08 * b
        );
        if let Some(t) = f.title {
            let (x, y) = (x0 + FRAME_PAD * b, y0 + (TITLE_BAND / 2.0 + 0.3) * b);
            let _ = writeln!(
                o,
                r#"<text x="{x:.1}" y="{y:.1}" font-size="{:.2}" font-family="{}" font-weight="bold" fill="{}">{}</text>"#,
                TITLE_SIZE * b,
                xml(&fonts.regular[0].family),
                f.color.unwrap_or(&th.label),
                xml(t)
            );
        }
    }
    if let Some(v) = &lay.venn {
        let colors = venn_colors(g, lay, th);
        for (c, col) in v.circles.iter().zip(&colors) {
            let (x, y) = tx(c.c);
            let _ = writeln!(
                o,
                r#"<circle cx="{x:.1}" cy="{y:.1}" r="{:.1}" fill="{col}" fill-opacity="{:.2}" stroke="{col}" stroke-width="{:.2}"/>"#,
                c.r,
                th.tint,
                0.12 * b
            );
        }
        for (t, col) in v.titles.iter().zip(&colors) {
            let Some(t) = t else { continue };
            let (x, y) = tx((t.at.0, t.at.1 - 0.35 * t.size));
            let _ = writeln!(
                o,
                r#"<text x="{x:.1}" y="{y:.1}" font-size="{:.2}" font-family="{}" font-weight="bold" text-anchor="middle" fill="{col}">{}</text>"#,
                t.size,
                xml(&fonts.regular[0].family),
                xml(&t.text)
            );
        }
    }
    for e in &lay.edges {
        let pts = edge_points(lay, e);
        let d: Vec<String> = pts
            .iter()
            .map(|&p| tx(p))
            .map(|(x, y)| format!("{x:.1},{y:.1}"))
            .collect();
        let sw = e.width * b;
        let (start, end, dash) = decor(g, e);
        let c = edge_color(e);
        let mut marks = String::new();
        for (h, at) in [(start, "start"), (end, "end")] {
            if h != Head::None {
                let _ = write!(marks, r#" marker-{at}="url(#{})""#, marker_id(h, c));
            }
        }
        let opacity = if e.style == EdgeStyle::Line {
            r#" stroke-opacity="0.7""#
        } else {
            ""
        };
        let _ = writeln!(
            o,
            r#"<polyline points="{}" fill="none" stroke="{c}"{opacity} stroke-width="{sw:.2}"{}{marks}/>"#,
            d.join(" "),
            svg_dash(dash, sw, b)
        );
    }
    for (v, p) in paint.iter().enumerate() {
        let bx = &lay.boxes[v];
        let (cx, cy) = tx(lay.pos[v]);
        let (left, top) = (cx - bx.w / 2.0, cy - bx.h / 2.0);
        // A stacked node grows up and to the right: the front copy holds the content.
        let (bw, bh) = (bx.w - bx.shift, bx.h - bx.shift);
        let btop = top + bx.shift;
        let (fcx, fcy) = (left + bw / 2.0, btop + bh / 2.0);
        for k in (1..bx.stack).rev() {
            let d = bx.shift / (bx.stack - 1) as f64 * k as f64;
            let _ = match bx.shape {
                Shape::Circle => writeln!(
                    o,
                    r#"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="{}" stroke="{}" stroke-width="{:.2}"/>"#,
                    fcx + d,
                    fcy - d,
                    bw / 2.0,
                    th.background,
                    p.accent,
                    0.08 * b
                ),
                Shape::Plain => Ok(()),
                Shape::Diamond => writeln!(
                    o,
                    r#"<polygon points="{}" fill="{}" stroke="{}" stroke-width="{:.2}" stroke-linejoin="round"/>"#,
                    svg_points(&rhombus(fcx + d, fcy - d, bw, bh)),
                    th.background,
                    p.accent,
                    0.08 * b
                ),
                Shape::Rect => writeln!(
                    o,
                    r#"<rect x="{:.1}" y="{:.1}" width="{bw:.1}" height="{bh:.1}" rx="{:.1}" fill="{}" stroke="{}" stroke-width="{:.2}"/>"#,
                    left + d,
                    btop - d,
                    0.35 * b,
                    th.background,
                    p.accent,
                    0.08 * b
                ),
            };
        }
        match (bx.shape, bx.role) {
            (Shape::Plain, _) => {}
            (Shape::Circle, Role::Root) => {
                let _ = writeln!(
                    o,
                    r#"<circle cx="{fcx:.1}" cy="{fcy:.1}" r="{:.1}" fill="{}" stroke="{}" stroke-width="{:.2}"/>"#,
                    bw / 2.0,
                    p.fill,
                    p.accent,
                    0.12 * b
                );
            }
            (Shape::Circle, _) => {
                let _ = writeln!(
                    o,
                    r#"<circle cx="{fcx:.1}" cy="{fcy:.1}" r="{:.1}" fill="{}"/>"#,
                    bw / 2.0,
                    p.fill
                );
            }
            (Shape::Diamond, _) => {
                let pts = svg_points(&rhombus(fcx, fcy, bw, bh));
                let _ = writeln!(o, r#"<polygon points="{pts}" fill="{}"/>"#, th.background);
                let _ = writeln!(
                    o,
                    r#"<polygon points="{pts}" fill="{}" fill-opacity="{}" stroke="{}" stroke-width="{:.2}" stroke-linejoin="round"/>"#,
                    p.fill,
                    p.opacity,
                    p.accent,
                    0.1 * b
                );
            }
            (Shape::Rect, _) => {
                let _ = writeln!(
                    o,
                    r#"<rect x="{left:.1}" y="{btop:.1}" width="{bw:.1}" height="{bh:.1}" rx="{:.1}" fill="{}"/>"#,
                    0.35 * b,
                    th.background
                );
                let _ = writeln!(
                    o,
                    r#"<rect x="{left:.1}" y="{btop:.1}" width="{bw:.1}" height="{bh:.1}" rx="{:.1}" fill="{}" fill-opacity="{}" stroke="{}" stroke-width="{:.2}"/>"#,
                    0.35 * b,
                    p.fill,
                    p.opacity,
                    p.accent,
                    0.1 * b
                );
            }
        }
        for c in &bx.cells {
            if c.shade > 0.0 {
                let _ = writeln!(
                    o,
                    r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="{:.1}" fill="{}" fill-opacity="{:.2}"/>"#,
                    left + c.x,
                    top + c.y,
                    c.w,
                    c.h,
                    0.12 * b,
                    p.accent,
                    c.shade
                );
            }
            let _ = writeln!(
                o,
                r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="{:.1}" fill="none" stroke="{}" stroke-width="{:.2}"/>"#,
                left + c.x,
                top + c.y,
                c.w,
                c.h,
                0.12 * b,
                p.accent,
                0.07 * b
            );
        }
        for l in &bx.lines {
            svg_line(&mut o, fonts, l, left + l.x, top + l.baseline, &p.text);
        }
        for c in &bx.cells {
            let fg = if c.shade >= 0.55 {
                &th.background
            } else {
                &p.text
            };
            if let Some(l) = &c.label {
                svg_line(&mut o, fonts, l, left + l.x, top + l.baseline, fg);
            }
        }
    }
    for e in &lay.edges {
        let (Some(label), Some(r)) = (&e.label, lay.label_rect(e)) else {
            continue;
        };
        let size = 0.8 * b;
        let ((x0, y0), (x1, y1)) = (tx((r.x0, r.y1)), tx((r.x1, r.y0)));
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
            r#"<text x="{cx:.1}" y="{cy:.1}" font-size="{size:.2}" font-family="{}" font-style="italic" fill="{}" text-anchor="middle">{}</text>"#,
            xml(&fonts.regular[0].family),
            th.label,
            xml(label)
        );
    }
    if let Some(k) = key {
        svg_key(&mut o, g, fonts, th, k, w, h);
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
        let t = tex(&r.text);
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
    let paint = paints(g, t, lay, th);
    let b = lay.body;
    let mut fallback: BTreeMap<char, String> = BTreeMap::new();
    for bx in &lay.boxes {
        for l in &bx.lines {
            for r in l.runs.iter().filter(|r| r.font > 0) {
                for c in r.text.chars() {
                    fallback
                        .entry(c)
                        .or_insert_with(|| fonts.font(l.bold, r.font).family.clone());
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
    let frames = groups(g, lay);
    let extra: Vec<&str> = lay
        .edges
        .iter()
        .filter_map(|e| stroke(g, e))
        .chain(frames.iter().filter_map(|f| f.color))
        .collect();
    let venn = venn_colors(g, lay, th);
    let used = [&th.background, &th.text, &th.edge, &th.link, &th.label]
        .into_iter()
        .map(String::as_str)
        .chain(
            paint
                .iter()
                .flat_map(|p| [p.accent.as_str(), p.fill.as_str(), p.text.as_str()]),
        )
        .chain(extra)
        .chain(venn.iter().map(String::as_str))
        .chain(th.gradient.as_deref())
        .chain(
            g.key
                .iter()
                .flat_map(|e| [e.color.as_deref(), e.fill.as_deref()])
                .flatten(),
        );
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
    let page_fill = match &th.gradient {
        Some(end) => format!(
            "top color={}, bottom color={}",
            cname(&th.background),
            cname(end)
        ),
        None => cname(&th.background),
    };
    let page_cmd = if th.gradient.is_some() {
        "shade"
    } else {
        "fill"
    };
    let page_fill = format!("{page_fill}, opacity={:.3}", theme::alpha(&th.background));
    let _ = writeln!(
        o,
        "\\{page_cmd}[{page_fill}] ({:.1},{:.1}) rectangle ({:.1},{:.1});",
        -w / 2.0,
        -h / 2.0,
        w / 2.0,
        h / 2.0
    );
    for f in &frames {
        let (r, c) = (f.rect, cname(f.color.unwrap_or(&th.edge)));
        let dash = if f.dashed {
            format!(", dash pattern=on {:.1}bp off {:.1}bp", 0.7 * b, 0.5 * b)
        } else {
            String::new()
        };
        let _ = writeln!(
            o,
            "\\filldraw[fill={c}, fill opacity={:.2}, draw={c}, line width={:.2}bp, rounded corners={:.1}bp{dash}] ({:.1},{:.1}) rectangle ({:.1},{:.1});",
            th.tint / 2.0,
            0.08 * b,
            0.5 * b,
            r.x0,
            r.y0,
            r.x1,
            r.y1
        );
        if let Some(t) = f.title {
            let _ = writeln!(
                o,
                "\\node[anchor=base west, inner sep=0bp, text={}] at ({:.1},{:.1}) {{\\fontsize{{{s:.2}bp}}{{{s:.2}bp}}\\selectfont\\bfseries {}}};",
                cname(f.color.unwrap_or(&th.label)),
                r.x0 + FRAME_PAD * b,
                r.y1 - (TITLE_BAND / 2.0 + 0.3) * b,
                tex(t),
                s = TITLE_SIZE * b
            );
        }
    }
    if let Some(v) = &lay.venn {
        for (c, col) in v.circles.iter().zip(&venn) {
            let _ = writeln!(
                o,
                "\\filldraw[fill={k}, fill opacity={:.2}, draw={k}, line width={:.2}bp] ({:.1},{:.1}) circle ({:.1}bp);",
                th.tint,
                0.12 * b,
                c.c.0,
                c.c.1,
                c.r,
                k = cname(col)
            );
        }
        for (t, col) in v.titles.iter().zip(&venn) {
            let Some(t) = t else { continue };
            let _ = writeln!(
                o,
                "\\node[inner sep=0bp, text={}] at ({:.1},{:.1}) {{\\fontsize{{{s:.2}bp}}{{{s:.2}bp}}\\selectfont\\bfseries {}}};",
                cname(col),
                t.at.0,
                t.at.1,
                tex(&t.text),
                s = t.size
            );
        }
    }
    for e in &lay.edges {
        let raw = edge_points(lay, e);
        let pts: Vec<String> = raw
            .iter()
            .map(|p| format!("({:.1},{:.1})", p.0, p.1))
            .collect();
        let lw = e.width * b;
        let col = |default: &str| cname(stroke(g, e).unwrap_or(default));
        let (start, end, dash) = decor(g, e);
        let decor = tikz_decor(start, end, dash);
        let style = match e.style {
            EdgeStyle::Line => format!("{}, opacity=0.7{decor}", col(&paint[e.b].accent)),
            EdgeStyle::Arrow => format!("{}{decor}", col(&th.edge)),
            EdgeStyle::Link => format!("{}{decor}", col(&th.link)),
        };
        let _ = writeln!(
            o,
            "\\draw[{style}, line width={lw:.2}bp] {};",
            pts.join(" -- ")
        );
    }
    for (v, p) in paint.iter().enumerate() {
        let bx = &lay.boxes[v];
        let (cx, cy) = lay.pos[v];
        let (c, f) = (cname(&p.accent), cname(&p.fill));
        let (bw, bh) = (bx.w - bx.shift, bx.h - bx.shift);
        let (x0, ytop) = (cx - bx.w / 2.0, cy + bx.h / 2.0 - bx.shift);
        let (fcx, fcy) = (x0 + bw / 2.0, ytop - bh / 2.0);
        for k in (1..bx.stack).rev() {
            let d = bx.shift / (bx.stack - 1) as f64 * k as f64;
            let bg = cname(&th.background);
            let _ = match bx.shape {
                Shape::Circle => writeln!(
                    o,
                    "\\filldraw[fill={bg}, draw={c}, line width={:.2}bp] ({:.1},{:.1}) circle ({:.1});",
                    0.08 * b,
                    fcx + d,
                    fcy - d,
                    bw / 2.0
                ),
                Shape::Plain => Ok(()),
                Shape::Diamond => writeln!(
                    o,
                    "\\filldraw[fill={bg}, draw={c}, line width={:.2}bp, line join=round] {};",
                    0.08 * b,
                    tikz_points(&rhombus(fcx + d, fcy - d, bw, bh))
                ),
                Shape::Rect => writeln!(
                    o,
                    "\\filldraw[fill={bg}, draw={c}, line width={:.2}bp, rounded corners={:.1}bp] ({:.1},{:.1}) rectangle ({:.1},{:.1});",
                    0.08 * b,
                    0.35 * b,
                    x0 + d,
                    ytop - d,
                    x0 + d + bw,
                    ytop - d - bh
                ),
            };
        }
        match (bx.shape, bx.role) {
            (Shape::Plain, _) => {}
            (Shape::Circle, Role::Root) => {
                let _ = writeln!(
                    o,
                    "\\filldraw[fill={f}, draw={c}, line width={:.2}bp] ({fcx:.1},{fcy:.1}) circle ({:.1});",
                    0.12 * b,
                    bw / 2.0
                );
            }
            (Shape::Circle, _) => {
                let _ = writeln!(
                    o,
                    "\\fill[{f}] ({fcx:.1},{fcy:.1}) circle ({:.1});",
                    bw / 2.0
                );
            }
            (Shape::Diamond, _) => {
                let pts = tikz_points(&rhombus(fcx, fcy, bw, bh));
                let _ = writeln!(
                    o,
                    "\\filldraw[fill={}, draw={c}, line width={:.2}bp, line join=round] {pts};",
                    cname(&th.background),
                    0.1 * b
                );
                let _ = writeln!(o, "\\fill[{f}, opacity={}] {pts};", p.opacity);
            }
            (Shape::Rect, _) => {
                let _ = writeln!(
                    o,
                    "\\filldraw[fill={}, draw={c}, line width={:.2}bp, rounded corners={:.1}bp] ({x0:.1},{ytop:.1}) rectangle ({:.1},{:.1});",
                    cname(&th.background),
                    0.1 * b,
                    0.35 * b,
                    x0 + bw,
                    ytop - bh
                );
                let _ = writeln!(
                    o,
                    "\\fill[{f}, opacity={}, rounded corners={:.1}bp] ({x0:.1},{ytop:.1}) rectangle ({:.1},{:.1});",
                    p.opacity,
                    0.35 * b,
                    x0 + bw,
                    ytop - bh
                );
            }
        }
        let text = cname(&p.text);
        for c in &bx.cells {
            let (x, y) = (cx - bx.w / 2.0 + c.x, cy + bx.h / 2.0 - c.y);
            if c.shade > 0.0 {
                let _ = writeln!(
                    o,
                    "\\fill[{}, opacity={:.2}, rounded corners={:.1}bp] ({x:.1},{y:.1}) rectangle ({:.1},{:.1});",
                    cname(&p.accent),
                    c.shade,
                    0.12 * b,
                    x + c.w,
                    y - c.h
                );
            }
            let _ = writeln!(
                o,
                "\\draw[{}, line width={:.2}bp, rounded corners={:.1}bp] ({x:.1},{y:.1}) rectangle ({:.1},{:.1});",
                cname(&p.accent),
                0.07 * b,
                0.12 * b,
                x + c.w,
                y - c.h
            );
            if let Some(l) = &c.label {
                let fg = cname(if c.shade >= 0.55 {
                    &th.background
                } else {
                    &p.text
                });
                tikz_line(
                    &mut o,
                    l,
                    cx - bx.w / 2.0 + l.x,
                    cy + bx.h / 2.0 - l.baseline,
                    &fg,
                );
            }
        }
        for l in &bx.lines {
            tikz_line(
                &mut o,
                l,
                cx - bx.w / 2.0 + l.x,
                cy + bx.h / 2.0 - l.baseline,
                &text,
            );
        }
    }
    for e in &lay.edges {
        let (Some(label), Some(r)) = (&e.label, lay.label_rect(e)) else {
            continue;
        };
        let m = ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
        let _ = writeln!(
            o,
            "\\node[fill={}, fill opacity=0.9, text opacity=1, inner sep=1bp, text={}] at ({:.1},{:.1}) {{\\fontsize{{{s:.2}bp}}{{{s:.2}bp}}\\selectfont\\itshape {}}};",
            cname(&th.background),
            cname(&th.label),
            m.0,
            m.1,
            tex(label),
            s = 0.8 * b
        );
    }
    if let Some(k) = key {
        let n = k.rows.len() as f64;
        for (r, row) in k.rows.iter().enumerate() {
            let cy = -h / 2.0 + k.pad + (n - r as f64 - 0.5) * k.row_h;
            let x0 = -row.width / 2.0;
            for &(dx, i) in &row.items {
                let e = &g.key[i];
                let x = x0 + dx;
                let c = cname(e.color.as_deref().unwrap_or(&th.edge));
                let (fill, op) = match (&e.fill, e.shape) {
                    _ if e.frame => (c.clone(), th.tint / 2.0),
                    (Some(f), _) => (cname(f), 1.0),
                    (None, Some(Shape::Circle)) => (c.clone(), 1.0),
                    (None, _) => (c.clone(), th.tint),
                };
                let s = k.size;
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
                    "\\node[anchor=base west, inner sep=0bp, text={}] at ({:.2},{:.2}) {{\\fontsize{{{s:.2}bp}}{{{s:.2}bp}}\\selectfont {}}};",
                    cname(&th.text),
                    x + 1.5 * s,
                    cy - 0.35 * s,
                    tex(&e.label)
                );
            }
        }
    }
    o += "\\end{tikzpicture}\n\\end{document}\n";
    o
}
