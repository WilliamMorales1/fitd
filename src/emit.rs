//! SVG, PNG and TikZ backends. All draw from the same solved geometry; none re-flows text.

use crate::graph::{Graph, LayoutKind, Relation};
use crate::geom::{P, Rect};
use crate::layout::{Drawn, EdgeStyle, FRAME_PAD, Layout, Role, Shape, TITLE_BAND, TextLine, frame};
use crate::metrics::{Fonts, Script};
use crate::theme::{self, Theme};
use crate::tree::Tree;
use std::collections::BTreeMap;
use std::fmt::Write;

/// Paint is how one node is drawn: accent stroke, fill, and text color.
struct Paint {
    accent: String,
    fill: String,
    /// Opacity of `fill` over the background.
    opacity: f64,
    text: String,
}

/// accents gives each node its own color, else its radial branch's, else the theme text color.
fn accents(g: &Graph, t: &Tree, th: &Theme, radial: bool) -> Vec<String> {
    let mut hub_color = BTreeMap::new();
    for (i, &h) in t.children[t.root].iter().enumerate() {
        hub_color.insert(h, g.nodes[h].color.clone().unwrap_or_else(|| th.palette[i % th.palette.len()].clone()));
    }
    (0..g.nodes.len())
        .map(|v| match t.branch(v).filter(|_| radial) {
            Some(h) => g.nodes[v].color.clone().unwrap_or_else(|| hub_color[&h].clone()),
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
                (Shape::Circle, Role::Root, None) => (th.root.clone(), 1.0, theme::on(&th.root).to_string()),
                (Shape::Circle, _, None) => (acc[v].clone(), 1.0, theme::on(&acc[v]).to_string()),
                (Shape::Rect, _, None) => (acc[v].clone(), th.tint, th.text.clone()),
            };
            let accent = if lay.boxes[v].role == Role::Root && n.color.is_none() { th.text.clone() } else { acc[v].clone() };
            Paint { accent, fill, opacity, text: n.text.clone().unwrap_or(auto) }
        })
        .collect()
}

/// clipped shortens a straight edge to the borders of its two nodes, so arrowheads show.
fn clipped(lay: &Layout, e: &Drawn) -> (P, P) {
    let (a, b) = (e.a, e.b);
    let (pa, pb) = lay.ends(e);
    let lerp = |t: f64| (pa.0 + t * (pb.0 - pa.0), pa.1 + t * (pb.1 - pa.1));
    let exit = |v: usize, mut inside: f64, mut outside: f64| {
        for _ in 0..40 {
            let mid = (inside + outside) / 2.0;
            if lay.inside(v, lerp(mid)) { inside = mid } else { outside = mid }
        }
        outside
    };
    (lerp(exit(a, 0.0, 1.0)), lerp(exit(b, 1.0, 0.0)))
}

/// link_points returns the drawn polyline of an edge, in layout coordinates.
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

/// stroke is the color the source gave an edge, if any. Edges are matched by endpoints, the
/// same direction first, since the radial tree may orient an edge either way.
fn stroke<'a>(g: &'a Graph, e: &Drawn) -> Option<&'a str> {
    let find = |a: usize, b: usize| g.edges.iter().find(|x| (x.a, x.b) == (a, b));
    find(e.a, e.b).or_else(|| find(e.b, e.a)).and_then(|x| x.color.as_deref())
}

/// Group is a frame to draw: its border, title, and color.
struct Group<'a> {
    rect: Rect,
    title: Option<&'a str>,
    color: Option<&'a str>,
}

fn groups<'a>(g: &'a Graph, lay: &Layout) -> Vec<Group<'a>> {
    g.constraints
        .iter()
        .filter_map(|c| match &c.rel {
            Relation::Group { title, color, .. } => Some(Group { rect: frame(&c.rel, &|v| lay.rect(v), lay.body), title: title.as_deref(), color: color.as_deref() }),
            _ => None,
        })
        .collect()
}

/// bounds is the smallest rectangle, in layout coordinates, holding every node, edge, label,
/// and group frame.
pub fn bounds(g: &Graph, lay: &Layout) -> Rect {
    let mut r = Rect { x0: f64::MAX, y0: f64::MAX, x1: f64::MIN, y1: f64::MIN };
    let mut add = |b: Rect| r = Rect { x0: r.x0.min(b.x0), y0: r.y0.min(b.y0), x1: r.x1.max(b.x1), y1: r.y1.max(b.y1) };
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
    r
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn svg_line(out: &mut String, fonts: &Fonts, l: &TextLine, x: f64, y: f64, fill: &str) {
    let _ = write!(out, r#"<text x="{x:.2}" y="{y:.2}" font-size="{:.2}" fill="{fill}" xml:space="preserve">"#, l.size);
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

/// svg draws the part of the layout inside `view`, which becomes the whole image.
pub fn svg(g: &Graph, t: &Tree, fonts: &Fonts, lay: &Layout, view: Rect, th: &Theme) -> String {
    let paint = paints(g, t, lay, th);
    let b = lay.body;
    let (w, h) = (view.x1 - view.x0, view.y1 - view.y0);
    let tx = |p: (f64, f64)| (p.0 - view.x0, view.y1 - p.1);
    let mut o = String::new();
    let _ = writeln!(o, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}pt" height="{h}pt" viewBox="0 0 {w} {h}">"#);
    if let Some(title) = &g.title {
        let _ = writeln!(o, "<title>{}</title>", xml(title));
    }
    o.push_str("<defs>");
    // Arrowheads take their edge's color, so each edge color gets its own marker.
    let mut markers = vec![("arr".to_string(), th.edge.as_str()), ("lnk".to_string(), th.link.as_str())];
    for e in lay.edges.iter().filter(|e| e.style != EdgeStyle::Line) {
        if let Some(c) = stroke(g, e).filter(|c| !markers.iter().any(|m| m.1 == *c)) {
            markers.push((format!("m{}", theme::rgb(c)), c));
        }
    }
    for (id, c) in &markers {
        let _ = write!(o, r#"<marker id="{id}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="{c}"/></marker>"#);
    }
    o.push_str("</defs>\n");
    let _ = writeln!(o, r#"<rect width="{w}" height="{h}" fill="{}"/>"#, th.background);
    for f in groups(g, lay) {
        let ((x0, y0), (x1, y1)) = (tx((f.rect.x0, f.rect.y1)), tx((f.rect.x1, f.rect.y0)));
        let c = f.color.unwrap_or(&th.edge);
        let _ = writeln!(o, r#"<rect x="{x0:.1}" y="{y0:.1}" width="{:.1}" height="{:.1}" rx="{:.1}" fill="{c}" fill-opacity="{:.2}" stroke="{c}" stroke-width="{:.2}"/>"#, x1 - x0, y1 - y0, 0.5 * b, th.tint / 2.0, 0.08 * b);
        if let Some(t) = f.title {
            let (x, y) = (x0 + FRAME_PAD * b, y0 + (TITLE_BAND / 2.0 + 0.3) * b);
            let _ = writeln!(o, r#"<text x="{x:.1}" y="{y:.1}" font-size="{:.2}" font-family="{}" font-weight="bold" fill="{}">{}</text>"#, 0.8 * b, xml(&fonts.regular[0].family), f.color.unwrap_or(&th.label), xml(t));
        }
    }
    for e in &lay.edges {
        let pts = edge_points(lay, e);
        let d: Vec<String> = pts.iter().map(|&p| tx(p)).map(|(x, y)| format!("{x:.1},{y:.1}")).collect();
        let sw = e.width * b;
        let marker = |c: &str| markers.iter().find(|m| m.1 == c).map_or("arr", |m| m.0.as_str()).to_string();
        match e.style {
            EdgeStyle::Line => {
                let _ = writeln!(o, r#"<polyline points="{}" fill="none" stroke="{}" stroke-opacity="0.7" stroke-width="{sw:.2}"/>"#, d.join(" "), stroke(g, e).unwrap_or(&paint[e.b].accent));
            }
            EdgeStyle::Arrow => {
                let c = stroke(g, e).unwrap_or(&th.edge);
                let _ = writeln!(o, r#"<polyline points="{}" fill="none" stroke="{c}" stroke-width="{sw:.2}" marker-end="url(#{})"/>"#, d.join(" "), marker(c));
            }
            EdgeStyle::Link => {
                let c = stroke(g, e).unwrap_or(&th.link);
                let _ = writeln!(o, r#"<polyline points="{}" fill="none" stroke="{c}" stroke-width="{sw:.2}" stroke-dasharray="{:.1} {:.1}" marker-end="url(#{})"/>"#, d.join(" "), 0.5 * b, 0.3 * b, marker(c));
            }
        }
    }
    for (v, p) in paint.iter().enumerate() {
        let bx = &lay.boxes[v];
        let (cx, cy) = tx(lay.pos[v]);
        let (left, top) = (cx - bx.w / 2.0, cy - bx.h / 2.0);
        match (bx.shape, bx.role) {
            (Shape::Circle, Role::Root) => {
                let _ = writeln!(o, r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{:.1}" fill="{}" stroke="{}" stroke-width="{:.2}"/>"#, bx.w / 2.0, p.fill, p.accent, 0.12 * b);
            }
            (Shape::Circle, _) => {
                let _ = writeln!(o, r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{:.1}" fill="{}"/>"#, bx.w / 2.0, p.fill);
            }
            (Shape::Rect, _) => {
                let _ = writeln!(o, r#"<rect x="{left:.1}" y="{top:.1}" width="{:.1}" height="{:.1}" rx="{:.1}" fill="{}"/>"#, bx.w, bx.h, 0.35 * b, th.background);
                let _ = writeln!(o, r#"<rect x="{left:.1}" y="{top:.1}" width="{:.1}" height="{:.1}" rx="{:.1}" fill="{}" fill-opacity="{}" stroke="{}" stroke-width="{:.2}"/>"#, bx.w, bx.h, 0.35 * b, p.fill, p.opacity, p.accent, 0.1 * b);
            }
        }
        for l in &bx.lines {
            svg_line(&mut o, fonts, l, left + l.x, top + l.baseline, &p.text);
        }
    }
    // Labels last, so nothing is ever drawn over them.
    for e in &lay.edges {
        let (Some(label), Some(r)) = (&e.label, lay.label_rect(e)) else { continue };
        let size = 0.8 * b;
        let ((x0, y0), (x1, y1)) = (tx((r.x0, r.y1)), tx((r.x1, r.y0)));
        let _ = writeln!(o, r#"<rect x="{x0:.1}" y="{y0:.1}" width="{:.1}" height="{:.1}" fill="{}" fill-opacity="0.9"/>"#, x1 - x0, y1 - y0, th.background);
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0 + 0.35 * size);
        let _ = writeln!(o, r#"<text x="{cx:.1}" y="{cy:.1}" font-size="{size:.2}" font-family="{}" font-style="italic" fill="{}" text-anchor="middle">{}</text>"#, xml(&fonts.regular[0].family), th.label, xml(label));
    }
    o.push_str("</svg>\n");
    o
}

/// png rasterizes an SVG from `svg` at `scale` pixels per point, using the measuring fonts.
const MAX_PIXELS: u64 = 256_000_000;

pub fn png(svg: &str, fonts: &Fonts, scale: f64) -> Result<Vec<u8>, String> {
    let mut opt = resvg::usvg::Options::default();
    for f in fonts.regular.iter().chain(&fonts.bold) {
        opt.fontdb_mut().load_font_data(f.data.clone());
    }
    let tree = resvg::usvg::Tree::from_str(svg, &opt).map_err(|e| e.to_string())?;
    let size = tree.size();
    // usvg reads `pt` as 4/3 px, so scale from its pixel size back to points.
    let k = (scale * 0.75) as f32;
    let (pw, ph) = ((size.width() * k).ceil() as u32, (size.height() * k).ceil() as u32);
    // tiny-skia accepts far more than memory holds; 256 Mpx is already a 1 GB buffer.
    if pw as u64 * ph as u64 > MAX_PIXELS {
        return Err(format!("image too large: {pw}x{ph}px (limit {} Mpx); lower --scale", MAX_PIXELS / 1_000_000));
    }
    let mut pix = resvg::tiny_skia::Pixmap::new(pw, ph).ok_or(format!("image too large: {pw}x{ph}px"))?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(k, k), &mut pix.as_mut());
    pix.encode_png().map_err(|e| e.to_string())
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

/// cname is the TikZ color name defined for `c` in the preamble.
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
            let _ = write!(body, "\\raisebox{{{:.2}bp}}{{\\fontsize{{{s:.2}bp}}{{{s:.2}bp}}\\selectfont {t}}}", r.script.shift() * l.size);
        }
    }
    let bf = if l.bold { "\\bfseries" } else { "" };
    let _ = writeln!(o, "\\node[anchor=base west, inner sep=0bp, text={color}] at ({x:.2},{y:.2}) {{\\fontsize{{{s:.2}bp}}{{{s:.2}bp}}\\selectfont{bf} {body}}};", s = l.size);
}

pub fn tikz(g: &Graph, t: &Tree, fonts: &Fonts, lay: &Layout, w: f64, h: f64, th: &Theme) -> String {
    let paint = paints(g, t, lay, th);
    let b = lay.body;
    let mut fallback: BTreeMap<char, String> = BTreeMap::new();
    for bx in &lay.boxes {
        for l in &bx.lines {
            for r in l.runs.iter().filter(|r| r.font > 0) {
                for c in r.text.chars() {
                    fallback.entry(c).or_insert_with(|| fonts.font(l.bold, r.font).family.clone());
                }
            }
        }
    }
    let families: Vec<String> = fallback.values().cloned().collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    let mut o = String::new();
    o += "% Generated by cmapc; edit the .cmap source instead.\n\\documentclass[border=0bp]{standalone}\n\\usepackage{fontspec}\n";
    let _ = writeln!(o, "\\setmainfont{{{}}}\n\\usepackage{{newunicodechar}}", fonts.regular[0].family);
    for (i, f) in families.iter().enumerate() {
        let _ = writeln!(o, "\\newfontfamily\\cmfb{}{{{f}}}", (b'A' + i as u8) as char);
    }
    for (c, f) in &fallback {
        let i = families.iter().position(|x| x == f).unwrap();
        let _ = writeln!(o, "\\newunicodechar{{{c}}}{{{{\\cmfb{} {c}}}}}", (b'A' + i as u8) as char);
    }
    o += "\\usepackage{tikz}\n\\usetikzlibrary{arrows.meta}\n";
    let mut defined = std::collections::BTreeSet::new();
    let frames = groups(g, lay);
    let extra: Vec<&str> = lay.edges.iter().filter_map(|e| stroke(g, e)).chain(frames.iter().filter_map(|f| f.color)).collect();
    let used = [&th.background, &th.text, &th.edge, &th.link, &th.label].into_iter().map(String::as_str).chain(paint.iter().flat_map(|p| [p.accent.as_str(), p.fill.as_str(), p.text.as_str()])).chain(extra);
    for c in used {
        if defined.insert(theme::rgb(c)) {
            let _ = writeln!(o, "\\definecolor{{{}}}{{HTML}}{{{}}}", cname(c), theme::rgb(c));
        }
    }
    o += "\\begin{document}\n\\begin{tikzpicture}[x=1bp,y=1bp]\n";
    let _ = writeln!(o, "\\fill[{}] ({:.1},{:.1}) rectangle ({:.1},{:.1});", cname(&th.background), -w / 2.0, -h / 2.0, w / 2.0, h / 2.0);
    for f in &frames {
        let (r, c) = (f.rect, cname(f.color.unwrap_or(&th.edge)));
        let _ = writeln!(o, "\\filldraw[fill={c}, fill opacity={:.2}, draw={c}, line width={:.2}bp, rounded corners={:.1}bp] ({:.1},{:.1}) rectangle ({:.1},{:.1});", th.tint / 2.0, 0.08 * b, 0.5 * b, r.x0, r.y0, r.x1, r.y1);
        if let Some(t) = f.title {
            let _ = writeln!(o, "\\node[anchor=base west, inner sep=0bp, text={}] at ({:.1},{:.1}) {{\\fontsize{{{s:.2}bp}}{{{s:.2}bp}}\\selectfont\\bfseries {}}};", cname(f.color.unwrap_or(&th.label)), r.x0 + FRAME_PAD * b, r.y1 - (TITLE_BAND / 2.0 + 0.3) * b, tex(t), s = 0.8 * b);
        }
    }
    for e in &lay.edges {
        let raw = edge_points(lay, e);
        let pts: Vec<String> = raw.iter().map(|p| format!("({:.1},{:.1})", p.0, p.1)).collect();
        let lw = e.width * b;
        let col = |default: &str| cname(stroke(g, e).unwrap_or(default));
        let style = match e.style {
            EdgeStyle::Line => format!("{}, opacity=0.7", col(&paint[e.b].accent)),
            EdgeStyle::Arrow => format!("{}, -{{Stealth}}", col(&th.edge)),
            EdgeStyle::Link => format!("{}, dashed, -{{Stealth}}", col(&th.link)),
        };
        let _ = writeln!(o, "\\draw[{style}, line width={lw:.2}bp] {};", pts.join(" -- "));
    }
    for (v, p) in paint.iter().enumerate() {
        let bx = &lay.boxes[v];
        let (cx, cy) = lay.pos[v];
        let (c, f) = (cname(&p.accent), cname(&p.fill));
        match (bx.shape, bx.role) {
            (Shape::Circle, Role::Root) => {
                let _ = writeln!(o, "\\filldraw[fill={f}, draw={c}, line width={:.2}bp] ({cx:.1},{cy:.1}) circle ({:.1});", 0.12 * b, bx.w / 2.0);
            }
            (Shape::Circle, _) => {
                let _ = writeln!(o, "\\fill[{f}] ({cx:.1},{cy:.1}) circle ({:.1});", bx.w / 2.0);
            }
            (Shape::Rect, _) => {
                let _ = writeln!(o, "\\filldraw[fill={}, draw={c}, line width={:.2}bp, rounded corners={:.1}bp] ({:.1},{:.1}) rectangle ({:.1},{:.1});", cname(&th.background), 0.1 * b, 0.35 * b, cx - bx.w / 2.0, cy - bx.h / 2.0, cx + bx.w / 2.0, cy + bx.h / 2.0);
                let _ = writeln!(o, "\\fill[{f}, opacity={}, rounded corners={:.1}bp] ({:.1},{:.1}) rectangle ({:.1},{:.1});", p.opacity, 0.35 * b, cx - bx.w / 2.0, cy - bx.h / 2.0, cx + bx.w / 2.0, cy + bx.h / 2.0);
            }
        }
        let text = cname(&p.text);
        for l in &bx.lines {
            tikz_line(&mut o, l, cx - bx.w / 2.0 + l.x, cy + bx.h / 2.0 - l.baseline, &text);
        }
    }
    for e in &lay.edges {
        let (Some(label), Some(r)) = (&e.label, lay.label_rect(e)) else { continue };
        let m = ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
        let _ = writeln!(o, "\\node[fill={}, fill opacity=0.9, text opacity=1, inner sep=1bp, text={}] at ({:.1},{:.1}) {{\\fontsize{{{s:.2}bp}}{{{s:.2}bp}}\\selectfont\\itshape {}}};", cname(&th.background), cname(&th.label), m.0, m.1, tex(label), s = 0.8 * b);
    }
    o += "\\end{tikzpicture}\n\\end{document}\n";
    o
}
