//! Borrow-checker-style verifier, independent of the solver.
//!
//! Every node owns its region of the page; tree edges and links borrow a corridor through the
//! page. The checker recomputes everything from the finished layout, so a solver bug surfaces
//! as a diagnostic instead of a broken picture.

use crate::diag::Diag;
use crate::geom::{P, segment_hits_rect, segments_cross};
use crate::graph::{Graph, LayoutKind, Relation};
use crate::geom::Rect;
use crate::layout::{Config, Drawn, EdgeStyle, Layout, Shape, frame, frame_clear, line_height, paths_meet, relation_holds};
use crate::metrics::Fonts;

pub const EXPLAIN: &[(&str, &str)] = &[
    ("E0001", "An edge, constraint, or `root` names a node id that is never declared with `node`."),
    ("E0002", "A line could not be parsed. Statements: `title`, `root ID`, `layout radial|layered`, `page`, `font`, `theme [PRESET] k=v`, `style NAME k=v`, `node ID \"Title\" [.style] [k=v]`, `| body`, `A -> B, C [\"label\"] [.style] [color=#hex]`, `A ..> B`, `group [\"Title\"] A, B`, `require REL`, `prefer REL [weight=N]`, `include \"FILE\"`."),
    ("E0003", "Two `node` (or `style`) statements declare the same name."),
    ("E0004", "A node uses `.name` but no `style name` is declared."),
    ("E0005", "An attribute is not valid on this statement, such as `weight=` on `require`."),
    ("E0006", "An attribute value has the wrong type or unit. Lengths need a unit: pt, mm, cm, in, or em."),
    ("E0007", "A constraint names the same node twice, or an alignment lists fewer than two nodes."),
    ("E0008", "Hard constraints contradict each other on one axis: orderings (`left-of`, `above`) form a cycle, possibly through alignments. Found before search; no layout can satisfy them."),
    ("E0009", "An unknown keyword value: layout, relation, alignment side, place, shape, or page flag."),
    ("E0010", "A node is listed in two `group` statements. Groups cannot share or nest nodes, since each frame must hold its own members and nothing else."),
    ("E0011", "An `include` names a file that is already being included (a cycle), or a file that cannot be read. The include is skipped. Paths are relative to the including file."),
    ("E0101", "Two nodes claim overlapping regions. Each node owns its box exclusively, like a mutable borrow."),
    ("E0102", "An edge passes through a node that is neither of its endpoints, so a reader cannot tell where it goes."),
    ("E0103", "Two tree edges cross. Radial trees must stay planar so each branch reads as one direction."),
    ("E0104", "A node extends past the page margin."),
    ("E0105", "A text line is wider than the box (or circle) that owns it."),
    ("E0106", "The body font is below --min-font, so the map is not readable at the target size."),
    ("E0108", "The finished layout breaks a `require` constraint or a `group` (its frame leaves the page, holds a node that is not a member, or meets another frame). The solver should never produce this; it indicates a solver bug."),
    ("E0109", "An edge label is covered by a node, overlaps another label, or is clipped by the page. The solver should never produce this."),
    ("E0110", "Even at --min-font, the listed `require` constraints cannot all hold together on this page. The set is minimal: drop any one and the rest fit (QuickXplain). Positions come from the strategy's candidate grid. A note says whether the core is proved or the search ran out of checks; in the latter case a larger `--budget` may shrink or dissolve it."),
    ("E0107", "A child sits closer to the center than its parent, breaking the general-to-specific direction."),
    ("W0201", "A cross link passes through an unrelated node. Allowed, but harder to follow."),
    ("W0202", "A character has no glyph in any fallback font and will render as a box."),
    ("W0203", "A style is declared but no node uses it."),
    ("W0205", "A `prefer` constraint is not met in the final layout; its weight was traded for others."),
    ("W0206", "Two edges cross in a layered layout. Crossings are soft (cost 1 each) there, since many graphs cannot be drawn without them."),
    ("W0207", "Two dashed links cross or run close together. Avoided when possible (cost 1 per pair), but not always avoidable."),
    ("W0204", "A setting (title, root, layout, page, font) appears twice; the last one wins."),
];

pub fn check(g: &Graph, fonts: &Fonts, cfg: &Config, lay: &Layout) -> Vec<Diag> {
    let mut d = Vec::new();
    let n = g.nodes.len();
    let id = |v: usize| format!("`{}`", g.nodes[v].id);
    let at = |v: Option<usize>| v.map(|v| g.nodes[v].span);
    let mut push = |err: bool, code, node: Option<usize>, msg: String, notes: Vec<String>| {
        let mut x = if err { Diag::error(code, msg) } else { Diag::warning(code, msg) };
        if let Some(span) = at(node) {
            x = x.primary(span, "");
        }
        x.notes = notes;
        d.push(x);
    };

    if lay.body + 1e-9 < cfg.min_font {
        push(true, "E0106", None, format!("body font {:.1}pt below minimum {:.1}pt", lay.body, cfg.min_font), vec![]);
    }
    let page = crate::geom::Rect::centered((0.0, 0.0), cfg.page_w - 2.0 * cfg.margin, cfg.page_h - 2.0 * cfg.margin);
    for v in 0..n {
        if !lay.rect(v).within(&page) {
            push(true, "E0104", Some(v), format!("{} extends past the page margin", id(v)), vec![]);
        }
    }
    for a in 0..n {
        for b in a + 1..n {
            if lay.rect(a).inflate(-0.01).intersects(&lay.rect(b)) {
                push(true, "E0101", Some(b), format!("{} overlaps the region owned by {}", id(b), id(a)),
                    vec![format!("{} owns {:?}", id(a), lay.rect(a))]);
            }
        }
    }
    let radial = lay.kind == LayoutKind::Radial;
    let straight: Vec<(usize, usize, (P, P))> = lay.edges.iter().filter(|e| e.style != EdgeStyle::Link).map(|e| (e.a, e.b, lay.ends(e))).collect();
    for &(p, v, (pp, pv)) in &straight {
        for u in (0..n).filter(|&u| u != p && u != v) {
            if segment_hits_rect(pp, pv, &lay.rect(u).inflate(-0.01)) {
                push(true, "E0102", Some(v), format!("edge {} -> {} passes through {}", id(p), id(v), id(u)), vec![]);
            }
        }
        if radial && lay.radius(v) + 1e-6 < lay.radius(p) {
            push(true, "E0107", Some(v), format!("{} is nearer the center than its parent {}", id(v), id(p)), vec![]);
        }
    }
    for (i, &(p, v, (pp, pv))) in straight.iter().enumerate() {
        for &(q, w, (pq, pw)) in &straight[i + 1..] {
            if segments_cross(pp, pv, pq, pw) {
                // Radial trees can always be drawn planar; layered graphs in general cannot.
                let (err, code) = if radial { (true, "E0103") } else { (false, "W0206") };
                push(err, code, Some(w), format!("edge {} -> {} crosses edge {} -> {}", id(q), id(w), id(p), id(v)), vec![]);
            }
        }
    }
    for v in 0..n {
        let b = &lay.boxes[v];
        for l in &b.lines {
            let width = fonts.runs_width(&l.runs, l.size, l.bold);
            let fits = match b.shape {
                Shape::Rect => l.x + width <= b.w - b.pad + 0.01 && l.baseline - 0.96 * l.size >= 0.0 && l.baseline - 0.96 * l.size + line_height(l.size) <= b.h + 0.01,
                Shape::Circle => {
                    let r = b.w / 2.0;
                    let top = l.baseline - 0.96 * l.size - r;
                    let far = top.abs().max((top + line_height(l.size)).abs());
                    (width / 2.0).hypot(far) <= r + 0.01
                }
            };
            if !fits {
                let text: String = l.runs.iter().map(|r| r.text.as_str()).collect();
                push(true, "E0105", Some(v), format!("text \"{text}\" overflows {}", id(v)), vec![]);
            }
        }
    }
    for e in lay.edges.iter().filter(|e| e.style == EdgeStyle::Link) {
        let path = lay.path(e);
        if let Some(u) = (0..n).find(|&u| u != e.a && u != e.b && path.iter().any(|&p| lay.inside(u, p))) {
            push(false, "W0201", Some(e.b), format!("link {} -> {} passes through {}", id(e.a), id(e.b), id(u)), vec![]);
        }
    }
    for &c in fonts.missing.lock().unwrap().iter() {
        let msg = format!("no glyph for {c:?} (U+{:04X})", c as u32);
        let mut x = Diag::warning("W0202", msg);
        if let Some(span) = glyph_owner(g, c) {
            x = x.primary(span, "needed here");
        }
        d.push(x);
    }
    let labels: Vec<(usize, Rect)> = lay.edges.iter().enumerate().filter_map(|(i, e)| lay.label_rect(e).map(|r| (i, r))).collect();
    for (k, &(i, r)) in labels.iter().enumerate() {
        let e = &lay.edges[i];
        let what = format!("label \"{}\" on {} -> {}", e.label.as_deref().unwrap_or(""), id(e.a), id(e.b));
        let problem = if !r.within(&page) {
            Some("is clipped by the page margin".to_string())
        } else if let Some(u) = (0..n).find(|&u| r.intersects(&lay.rect(u))) {
            Some(format!("is covered by {}", id(u)))
        } else {
            labels[k + 1..].iter().find(|(_, q)| r.intersects(q)).map(|&(j, _)| format!("overlaps the label on {} -> {}", id(lay.edges[j].a), id(lay.edges[j].b)))
        };
        if let Some(p) = problem {
            d.push(Diag::error("E0109", format!("{what} {p}")).primary(g.nodes[e.b].span, ""));
        }
    }
    let links: Vec<&Drawn> = lay.edges.iter().filter(|e| e.style == EdgeStyle::Link).collect();
    for (k, e) in links.iter().enumerate() {
        for f in &links[k + 1..] {
            let skip: Vec<Rect> = [e.a, e.b].into_iter().filter(|u| *u == f.a || *u == f.b).map(|u| lay.rect(u).inflate(1.5 * lay.body)).collect();
            if paths_meet(&lay.path(e), &lay.path(f), 0.5 * lay.body, &skip) {
                d.push(Diag::warning("W0207", format!("links {} -> {} and {} -> {} cross or run together", id(e.a), id(e.b), id(f.a), id(f.b))).primary(g.nodes[f.b].span, ""));
            }
        }
    }
    let rect = |v: usize| lay.rect(v);
    let groups: Vec<(usize, Rect)> = g.constraints.iter().enumerate().filter(|(_, c)| matches!(c.rel, Relation::Group { .. })).map(|(i, c)| (i, frame(&c.rel, &rect, lay.body))).collect();
    for (k, &(ci, f)) in groups.iter().enumerate() {
        let c = &g.constraints[ci];
        let items = c.rel.nodes();
        if let Some(u) = (0..n).find(|&u| !items.contains(&u) && !frame_clear(&f, &lay.rect(u), lay.body)) {
            d.push(Diag::error("E0108", format!("group frame holds {}, which is not a member", id(u))).primary(c.span, "violated by the layout"));
        }
        if let Some(&(cj, _)) = groups[k + 1..].iter().find(|(_, h)| !frame_clear(&f, h, lay.body)) {
            d.push(Diag::error("E0108", "group frames meet").primary(c.span, "this frame").secondary(g.constraints[cj].span, "meets this one"));
        }
    }
    for c in &g.constraints {
        if !relation_holds(&c.rel, &rect, lay.body, &page, lay.tol) {
            d.push(if c.hard {
                Diag::error("E0108", "required constraint does not hold").primary(c.span, "violated by the layout")
            } else {
                Diag::warning("W0205", "preference not met").primary(c.span, format!("costs {}", c.weight))
            });
        }
    }
    d
}

/// glyph_owner finds the first statement, in source order, whose drawn text contains `c`.
fn glyph_owner(g: &Graph, c: char) -> Option<crate::diag::Span> {
    let nodes = g.nodes.iter().filter(|v| v.title.contains(c) || v.body.iter().any(|l| l.contains(c))).map(|v| v.span);
    let edges = g.edges.iter().filter(|e| e.label.as_deref().is_some_and(|l| l.contains(c))).map(|e| e.span);
    let groups = g.constraints.iter().filter(|k| matches!(&k.rel, Relation::Group { title: Some(t), .. } if t.contains(c))).map(|k| k.span);
    nodes.chain(edges).chain(groups).min_by_key(|s| s.lo)
}

#[cfg(test)]
mod tests {
    use super::EXPLAIN;

    #[test]
    fn every_code_has_an_explain_page() {
        let sources = [include_str!("diag.rs"), include_str!("lex.rs"), include_str!("parse.rs"), include_str!("sema.rs"), include_str!("check.rs"), include_str!("main.rs"), include_str!("include.rs")];
        let mut seen = 0;
        for src in sources {
            for (i, _) in src.match_indices('"') {
                let code = src.get(i + 1..i + 6).unwrap_or("");
                if src.get(i + 6..i + 7) == Some("\"") && (code.starts_with('E') || code.starts_with('W')) && code[1..].chars().all(|c| c.is_ascii_digit()) {
                    seen += 1;
                    assert!(EXPLAIN.iter().any(|(c, _)| *c == code), "{code} has no `fitd explain` page");
                }
            }
        }
        assert!(seen > 20, "found only {seen} codes");
        let lang = include_str!("../docs/LANGUAGE.md");
        for (code, _) in EXPLAIN {
            let n: u32 = code[1..].parse().unwrap();
            // The table lists some codes as ranges, like `E0101-E0107`.
            let listed = lang.contains(code) || lang.match_indices(&code[..3]).any(|(i, _)| {
                let range = &lang[i..(i + 11).min(lang.len())];
                range.len() == 11 && &range[5..6] == "-" && range[1..5].parse::<u32>().is_ok_and(|lo| lo <= n) && range[7..11].parse::<u32>().is_ok_and(|hi| n <= hi)
            });
            assert!(listed, "{code} missing from docs/LANGUAGE.md");
        }
    }
}
