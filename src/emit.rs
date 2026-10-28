//! SVG and TikZ backends. Both draw from the same solved geometry; neither re-flows text.

use crate::graph::Graph;
use crate::geom::P;
use crate::layout::{Drawn, EdgeStyle, Layout, Role, Shape, TextLine};
use crate::metrics::{Fonts, Script};
use crate::tree::Tree;
use std::collections::BTreeMap;
use std::fmt::Write;

const PALETTE: &[&str] = &["#8C5A2B", "#2E7D32", "#1565C0", "#6A1B9A", "#C62828", "#00838F", "#EF6C00", "#455A64"];

pub fn colors(g: &Graph, t: &Tree) -> Vec<String> {
    let mut hub_color = BTreeMap::new();
    for (i, &h) in t.children[t.root].iter().enumerate() {
        hub_color.insert(h, g.nodes[h].color.clone().unwrap_or_else(|| PALETTE[i % PALETTE.len()].to_string()));
    }
    (0..g.nodes.len())
        .map(|v| match t.branch(v) {
            Some(h) => g.nodes[v].color.clone().unwrap_or_else(|| hub_color[&h].clone()),
            None => "#424242".to_string(),
        })
        .collect()
}

/// clipped shortens a straight edge to the borders of its two nodes, so arrowheads show.
fn clipped(lay: &Layout, a: usize, b: usize) -> (P, P) {
    let (pa, pb) = (lay.pos[a], lay.pos[b]);
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
        EdgeStyle::Link => lay.link_path(e.a, e.b),
        EdgeStyle::Arrow => {
            let (s, t) = clipped(lay, e.a, e.b);
            vec![s, t]
        }
        EdgeStyle::Line => vec![lay.pos[e.a], lay.pos[e.b]],
    }
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

pub fn svg(g: &Graph, t: &Tree, fonts: &Fonts, lay: &Layout, w: f64, h: f64) -> String {
    let col = colors(g, t);
    let b = lay.body;
    let tx = |p: (f64, f64)| (w / 2.0 + p.0, h / 2.0 - p.1);
    let mut o = String::new();
    let _ = writeln!(o, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}pt" height="{h}pt" viewBox="0 0 {w} {h}">"#);
    if let Some(title) = &g.title {
        let _ = writeln!(o, "<title>{}</title>", xml(title));
    }
    let _ = writeln!(o, r##"<defs><marker id="arr" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="#777"/></marker></defs>"##);
    let _ = writeln!(o, r#"<rect width="{w}" height="{h}" fill="white"/>"#);
    for e in &lay.edges {
        let pts = edge_points(lay, e);
        let d: Vec<String> = pts.iter().map(|&p| tx(p)).map(|(x, y)| format!("{x:.1},{y:.1}")).collect();
        let sw = e.width * b;
        match e.style {
            EdgeStyle::Line => {
                let _ = writeln!(o, r#"<polyline points="{}" fill="none" stroke="{}" stroke-opacity="0.7" stroke-width="{sw:.2}"/>"#, d.join(" "), col[e.b]);
            }
            EdgeStyle::Arrow => {
                let _ = writeln!(o, r##"<polyline points="{}" fill="none" stroke="#555" stroke-width="{sw:.2}" marker-end="url(#arr)"/>"##, d.join(" "));
            }
            EdgeStyle::Link => {
                let _ = writeln!(o, r##"<polyline points="{}" fill="none" stroke="#777" stroke-width="{sw:.2}" stroke-dasharray="{:.1} {:.1}" marker-end="url(#arr)"/>"##, d.join(" "), 0.5 * b, 0.3 * b);
            }
        }
        if let Some(label) = &e.label {
            let size = 0.8 * b;
            let lw = fonts.width(label, size, false);
            let (x, y) = tx(label_spot(lay, &pts, lw + 4.0, size * 1.3));
            let _ = writeln!(o, r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="white"/>"#, x - lw / 2.0 - 2.0, y - size, lw + 4.0, size * 1.3);
            let _ = writeln!(o, r##"<text x="{x:.1}" y="{y:.1}" font-size="{size:.2}" font-family="{}" font-style="italic" fill="#555" text-anchor="middle">{}</text>"##, xml(&fonts.regular[0].family), xml(label));
        }
    }
    for v in 0..lay.pos.len() {
        let bx = &lay.boxes[v];
        let (cx, cy) = tx(lay.pos[v]);
        let (left, top) = (cx - bx.w / 2.0, cy - bx.h / 2.0);
        let fill = match (bx.shape, bx.role) {
            (Shape::Circle, Role::Root) => {
                let _ = writeln!(o, r##"<circle cx="{cx:.1}" cy="{cy:.1}" r="{:.1}" fill="#eeeeee" stroke="#222" stroke-width="{:.2}"/>"##, bx.w / 2.0, 0.12 * b);
                "#111"
            }
            (Shape::Circle, _) => {
                let _ = writeln!(o, r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{:.1}" fill="{}"/>"#, bx.w / 2.0, col[v]);
                "white"
            }
            (Shape::Rect, _) => {
                let _ = writeln!(o, r#"<rect x="{left:.1}" y="{top:.1}" width="{:.1}" height="{:.1}" rx="{:.1}" fill="white"/>"#, bx.w, bx.h, 0.35 * b);
                let _ = writeln!(o, r#"<rect x="{left:.1}" y="{top:.1}" width="{:.1}" height="{:.1}" rx="{:.1}" fill="{c}" fill-opacity="0.08" stroke="{c}" stroke-width="{:.2}"/>"#, bx.w, bx.h, 0.35 * b, 0.1 * b, c = col[v]);
                "#111"
            }
        };
        for l in &bx.lines {
            svg_line(&mut o, fonts, l, left + l.x, top + l.baseline, fill);
        }
    }
    o.push_str("</svg>\n");
    o
}

/// label_spot slides a label along its edge, from the middle outward, to the first place
/// where its box clears every node; the middle if there is none.
fn label_spot(lay: &Layout, pts: &[P], w: f64, h: f64) -> P {
    let at = |t: f64| {
        let f = t * (pts.len() - 1) as f64;
        let (i, u) = ((f as usize).min(pts.len() - 2), f.fract());
        let (a, b) = (pts[i], pts[i + 1]);
        (a.0 + u * (b.0 - a.0), a.1 + u * (b.1 - a.1))
    };
    for k in 0..=20 {
        let t = 0.5 + if k % 2 == 0 { 1.0 } else { -1.0 } * (k as f64 / 2.0).ceil() * 0.02;
        let p = at(t);
        let r = crate::geom::Rect::centered(p, w, h);
        if (0..lay.pos.len()).all(|v| !lay.rect(v).intersects(&r)) {
            return p;
        }
    }
    at(0.5)
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

fn hex(c: &str) -> String {
    c.trim_start_matches('#').to_uppercase()
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

pub fn tikz(g: &Graph, t: &Tree, fonts: &Fonts, lay: &Layout, w: f64, h: f64) -> String {
    let col = colors(g, t);
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
    for c in &col {
        if defined.insert(hex(c)) {
            let _ = writeln!(o, "\\definecolor{{c{}}}{{HTML}}{{{}}}", hex(c), hex(c));
        }
    }
    o += "\\begin{document}\n\\begin{tikzpicture}[x=1bp,y=1bp]\n";
    let _ = writeln!(o, "\\fill[white] ({:.1},{:.1}) rectangle ({:.1},{:.1});", -w / 2.0, -h / 2.0, w / 2.0, h / 2.0);
    for e in &lay.edges {
        let raw = edge_points(lay, e);
        let pts: Vec<String> = raw.iter().map(|p| format!("({:.1},{:.1})", p.0, p.1)).collect();
        let lw = e.width * b;
        let style = match e.style {
            EdgeStyle::Line => format!("c{}, opacity=0.7", hex(&col[e.b])),
            EdgeStyle::Arrow => "black!65, -{Stealth}".to_string(),
            EdgeStyle::Link => "black!50, dashed, -{Stealth}".to_string(),
        };
        let _ = writeln!(o, "\\draw[{style}, line width={lw:.2}bp] {};", pts.join(" -- "));
        if let Some(label) = &e.label {
            let m = label_spot(lay, &raw, fonts.width(label, 0.8 * b, false) + 4.0, 1.04 * b);
            let _ = writeln!(o, "\\node[fill=white, inner sep=1bp, text=black!65] at ({:.1},{:.1}) {{\\fontsize{{{s:.2}bp}}{{{s:.2}bp}}\\selectfont\\itshape {}}};", m.0, m.1, tex(label), s = 0.8 * b);
        }
    }
    for v in 0..lay.pos.len() {
        let bx = &lay.boxes[v];
        let (cx, cy) = lay.pos[v];
        let c = format!("c{}", hex(&col[v]));
        let text = match (bx.shape, bx.role) {
            (Shape::Circle, Role::Root) => {
                let _ = writeln!(o, "\\filldraw[fill=black!7, draw=black!85, line width={:.2}bp] ({cx:.1},{cy:.1}) circle ({:.1});", 0.12 * b, bx.w / 2.0);
                "black"
            }
            (Shape::Circle, _) => {
                let _ = writeln!(o, "\\fill[{c}] ({cx:.1},{cy:.1}) circle ({:.1});", bx.w / 2.0);
                "white"
            }
            (Shape::Rect, _) => {
                let _ = writeln!(o, "\\filldraw[fill=white, draw={c}, line width={:.2}bp, rounded corners={:.1}bp] ({:.1},{:.1}) rectangle ({:.1},{:.1});", 0.1 * b, 0.35 * b, cx - bx.w / 2.0, cy - bx.h / 2.0, cx + bx.w / 2.0, cy + bx.h / 2.0);
                let _ = writeln!(o, "\\fill[{c}, opacity=0.08, rounded corners={:.1}bp] ({:.1},{:.1}) rectangle ({:.1},{:.1});", 0.35 * b, cx - bx.w / 2.0, cy - bx.h / 2.0, cx + bx.w / 2.0, cy + bx.h / 2.0);
                "black"
            }
        };
        for l in &bx.lines {
            tikz_line(&mut o, l, cx - bx.w / 2.0 + l.x, cy + bx.h / 2.0 - l.baseline, text);
        }
    }
    o += "\\end{tikzpicture}\n\\end{document}\n";
    o
}
