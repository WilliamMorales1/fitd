//! Semantic analysis: resolves names, applies styles, types attribute values, and rejects
//! constraint sets that contradict each other before any search runs.

use crate::ast::{Attr, Name, Rel, Stmt, StmtKind, Value};
use crate::diag::{Diag, Span};
use crate::graph::{Constraint, Dir, Edge, EdgeKind, Graph, LayoutKind, Length, Node, Place, Relation, Settings, Shape, Side};
use std::collections::HashMap;

const MM: f64 = 72.0 / 25.4;

/// paper returns a named ISO size in points, landscape.
pub fn paper(name: &str) -> Option<(f64, f64)> {
    let mm = match name.to_ascii_lowercase().as_str() {
        "a0" => (1189.0, 841.0),
        "a1" => (841.0, 594.0),
        "a2" => (594.0, 420.0),
        "a3" => (420.0, 297.0),
        "a4" => (297.0, 210.0),
        _ => return None,
    };
    Some((mm.0 * MM, mm.1 * MM))
}

fn unit(u: &str) -> Option<Length> {
    Some(match u {
        "pt" => Length::Pt(1.0),
        "mm" => Length::Pt(MM),
        "cm" => Length::Pt(10.0 * MM),
        "in" => Length::Pt(72.0),
        "em" => Length::Em(1.0),
        _ => return None,
    })
}

/// length parses `12pt`, `5mm`, or `2em`; `bare` is the unit assumed when none is written.
pub fn length(s: &str, bare: Option<&str>) -> Result<Length, String> {
    let cut = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());
    let x: f64 = s[..cut].parse().map_err(|_| format!("`{s}` is not a number"))?;
    let u = match (&s[cut..], bare) {
        ("", Some(b)) => b,
        ("", None) => return Err(format!("`{s}` needs a unit (pt, mm, cm, in, em)")),
        (u, _) => u,
    };
    match unit(u).ok_or(format!("unknown unit `{u}` (pt, mm, cm, in, em)"))? {
        Length::Pt(k) => Ok(Length::Pt(k * x)),
        Length::Em(k) => Ok(Length::Em(k * x)),
    }
}

/// page_size parses a paper name or `WxH` with one trailing unit, such as `594x420mm`.
pub fn page_size(s: &str, bare: Option<&str>) -> Result<(f64, f64), String> {
    if let Some(p) = paper(s) {
        return Ok(p);
    }
    let (w, h) = s.split_once('x').ok_or(format!("`{s}` is not a page size (a0..a4 or WxH)"))?;
    let cut = h.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(h.len());
    let u = &h[cut..];
    let abs = |v: &str| match length(&format!("{v}{u}"), bare)? {
        Length::Pt(x) => Ok(x),
        Length::Em(_) => Err("page size cannot be in em".to_string()),
    };
    Ok((abs(w)?, abs(&h[..cut])?))
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for j in 0..b.len() {
            let cur = row[j + 1];
            row[j + 1] = (prev + (ca != b[j]) as usize).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

fn suggest<'a>(word: &str, options: impl Iterator<Item = &'a str>) -> Option<String> {
    let limit = (word.chars().count() / 3).max(1);
    options.map(|o| (edit_distance(word, o), o)).filter(|&(d, _)| d <= limit).min().map(|(_, o)| format!("did you mean `{o}`?"))
}

fn unknown(code: &'static str, what: &str, n: &Name, options: &[&str]) -> Diag {
    let d = Diag::error(code, format!("unknown {what} `{}`", n.text)).primary(n.span, format!("not a known {what}"));
    match suggest(&n.text, options.iter().copied()) {
        Some(s) => d.help(s),
        None => d.note(format!("expected one of: {}", options.join(", "))),
    }
}

struct Sema {
    diags: Vec<Diag>,
    ids: HashMap<String, usize>,
    nodes: Vec<Node>,
    /// Statement span of each node's declaration, for "first defined here" labels.
    decl: Vec<Span>,
}

impl Sema {
    fn node(&mut self, n: &Name) -> Option<usize> {
        if let Some(&i) = self.ids.get(&n.text) {
            return Some(i);
        }
        let d = Diag::error("E0001", format!("unknown node `{}`", n.text)).primary(n.span, "not declared with `node`");
        let d = match suggest(&n.text, self.ids.keys().map(String::as_str)) {
            Some(s) => d.help(s),
            None => d,
        };
        self.diags.push(d);
        None
    }

    fn type_err(&mut self, v: &Value, msg: String) {
        self.diags.push(Diag::error("E0006", msg).primary(v.span(), "wrong type or unit"));
    }

    fn len_value(&mut self, v: &Value, bare: Option<&str>) -> Option<Length> {
        match v {
            Value::Num(s, _) => match length(s, bare) {
                Ok(l) => Some(l),
                Err(e) => {
                    self.type_err(v, e);
                    None
                }
            },
            _ => {
                self.type_err(v, "expected a length like `12pt` or `2em`".into());
                None
            }
        }
    }

    fn abs_len(&mut self, v: &Value) -> Option<f64> {
        match self.len_value(v, None)? {
            Length::Pt(x) => Some(x),
            Length::Em(_) => {
                self.type_err(v, "expected an absolute length (pt, mm, cm, in), not em".into());
                None
            }
        }
    }
}

/// look applies `color` and `shape` attributes to a node or style.
fn look(attrs: &[Attr], color: &mut Option<String>, shape: &mut Option<Shape>, diags: &mut Vec<Diag>) {
    let type_err = |diags: &mut Vec<Diag>, v: &Value, msg: &str| diags.push(Diag::error("E0006", msg).primary(v.span(), "wrong type or unit"));
    for a in attrs {
        match (a.key.text.as_str(), &a.value) {
            ("color", Value::Color(c, _)) => *color = Some(c.clone()),
            ("color", Value::Str(c, _)) if c.starts_with('#') && c.len() > 1 && c[1..].chars().all(|h| h.is_ascii_hexdigit()) => *color = Some(c.clone()),
            ("color", v) => type_err(diags, v, "expected a color like `#1565C0` (written right after `=`)"),
            ("shape", Value::Name(n)) if n.text == "circle" => *shape = Some(Shape::Circle),
            ("shape", Value::Name(n)) if n.text == "box" => *shape = Some(Shape::Rect),
            ("shape", Value::Name(n)) => diags.push(unknown("E0009", "shape", n, &["circle", "box"])),
            ("shape", v) => type_err(diags, v, "expected `circle` or `box`"),
            _ => diags.push(bad_attr(a, &["color", "shape"])),
        }
    }
}

fn bad_attr(a: &Attr, allowed: &[&str]) -> Diag {
    let d = Diag::error("E0005", format!("unknown attribute `{}`", a.key.text)).primary(a.key.span, "not valid on this statement");
    match suggest(&a.key.text, allowed.iter().copied()) {
        Some(s) => d.help(s),
        None if allowed.is_empty() => d.note("this statement takes no attributes"),
        None => d.note(format!("allowed here: {}", allowed.join(", "))),
    }
}

const RELATIONS: &[&str] = &["left-of", "right-of", "above", "below", "near", "at"];
const SIDES: &[&str] = &["top", "bottom", "left", "right", "row", "column"];
const PLACES: &[&str] = &["top-left", "top", "top-right", "left", "center", "right", "bottom-left", "bottom", "bottom-right"];

fn place(s: &str) -> Option<Place> {
    PLACES.iter().position(|&p| p == s).map(|i| Place((i % 3) as u8, (i / 3) as u8))
}

pub fn analyze(stmts: &[Stmt]) -> (Graph, Vec<Diag>) {
    let mut s = Sema { diags: Vec::new(), ids: HashMap::new(), nodes: Vec::new(), decl: Vec::new() };
    let mut styles: HashMap<String, (Span, Vec<Attr>, bool)> = HashMap::new();

    // Pass 1: declarations, so edges and constraints may name nodes declared further down.
    let mut last_node = None;
    for st in stmts {
        match &st.kind {
            StmtKind::Node { id, title, .. } => {
                if let Some(&prev) = s.ids.get(&id.text) {
                    let d = Diag::error("E0003", format!("node `{}` declared twice", id.text)).primary(id.span, "declared again here");
                    s.diags.push(d.secondary(s.decl[prev], "first declared here"));
                    last_node = None;
                    continue;
                }
                s.ids.insert(id.text.clone(), s.nodes.len());
                let title = title.as_ref().map(|t| t.0.clone()).unwrap_or(id.text.clone());
                last_node = Some(s.nodes.len());
                s.nodes.push(Node { id: id.text.clone(), title, body: Vec::new(), color: None, shape: None, span: id.span });
                s.decl.push(st.span);
            }
            StmtKind::Body(text) => match last_node {
                Some(v) => s.nodes[v].body.push(text.clone()),
                None => s.diags.push(Diag::error("E0002", "body line outside a node").primary(st.span, "no `node` above this line")),
            },
            StmtKind::Style { name, attrs } => {
                if let Some((prev, ..)) = styles.get(&name.text) {
                    let d = Diag::error("E0003", format!("style `{}` declared twice", name.text)).primary(name.span, "declared again here");
                    s.diags.push(d.secondary(*prev, "first declared here"));
                } else {
                    styles.insert(name.text.clone(), (name.span, attrs.clone(), false));
                }
                last_node = None;
            }
            _ => last_node = None,
        }
    }

    // Pass 2: everything that refers to a declaration.
    let mut set = Settings::default();
    let (mut title, mut root, mut edges, mut constraints) = (None, None, Vec::new(), Vec::new());
    let mut seen_setting: HashMap<&str, Span> = HashMap::new();
    for st in stmts {
        let key = match &st.kind {
            StmtKind::Title(_) => "title",
            StmtKind::Root(_) => "root",
            StmtKind::Layout { .. } => "layout",
            StmtKind::Page { .. } => "page",
            StmtKind::Font { .. } => "font",
            _ => "",
        };
        if !key.is_empty() {
            if let Some(prev) = seen_setting.insert(key, st.span) {
                let d = Diag::warning("W0204", format!("`{key}` set more than once; the last one wins")).primary(st.span, "this one wins");
                s.diags.push(d.secondary(prev, "earlier setting ignored"));
            }
        }
        match &st.kind {
            StmtKind::Title(t) => title = Some(t.clone()),
            StmtKind::Root(n) => root = s.node(n).or(root),
            StmtKind::Layout { kind, attrs } => {
                let allowed: &[&str] = match kind.text.as_str() {
                    "radial" => {
                        set.layout = Some(LayoutKind::Radial);
                        &["root"]
                    }
                    "layered" => {
                        set.layout = Some(LayoutKind::Layered(Dir::Down));
                        &["dir"]
                    }
                    _ => {
                        s.diags.push(unknown("E0009", "layout", kind, &["radial", "layered"]));
                        &[]
                    }
                };
                for a in attrs {
                    match (a.key.text.as_str(), &a.value) {
                        (k, _) if !allowed.contains(&k) => s.diags.push(bad_attr(a, allowed)),
                        ("root", Value::Name(n)) => root = s.node(n).or(root),
                        ("root", v) => s.type_err(v, "expected a node name".into()),
                        ("dir", Value::Name(n)) if n.text == "down" => set.layout = Some(LayoutKind::Layered(Dir::Down)),
                        ("dir", Value::Name(n)) if n.text == "right" => set.layout = Some(LayoutKind::Layered(Dir::Right)),
                        ("dir", Value::Name(n)) => s.diags.push(unknown("E0009", "direction", n, &["down", "right"])),
                        (_, v) => s.type_err(v, "expected `down` or `right`".into()),
                    }
                }
            }
            StmtKind::Page { size, flags, attrs } => {
                let text = match size {
                    Value::Name(n) => Some(&n.text),
                    Value::Num(t, _) => Some(t),
                    _ => None,
                };
                match text.map(|t| page_size(t, None)) {
                    Some(Ok(p)) => set.page = Some(p),
                    Some(Err(e)) => s.type_err(size, e),
                    None => s.type_err(size, "expected a page size like `a1` or `594x420mm`".into()),
                }
                for f in flags {
                    match (f.text.as_str(), set.page) {
                        ("portrait", Some((w, h))) => set.page = Some((w.min(h), w.max(h))),
                        ("landscape", Some((w, h))) => set.page = Some((w.max(h), w.min(h))),
                        ("portrait" | "landscape", None) => {}
                        _ => s.diags.push(unknown("E0009", "page flag", f, &["portrait", "landscape"])),
                    }
                }
                for a in attrs {
                    match a.key.text.as_str() {
                        "margin" => set.margin = s.abs_len(&a.value).or(set.margin),
                        _ => s.diags.push(bad_attr(a, &["margin"])),
                    }
                }
            }
            StmtKind::Font { size, attrs } => {
                let (lo, hi) = match size {
                    Value::Range(lo, hi) => (s.abs_len(lo), s.abs_len(hi)),
                    v => {
                        let x = s.abs_len(v);
                        (x, x)
                    }
                };
                if let (Some(lo), Some(hi)) = (lo, hi) {
                    if lo > hi || lo <= 0.0 {
                        s.type_err(size, format!("font range {lo}pt..{hi}pt is empty"));
                    } else {
                        set.font = Some((lo, hi));
                    }
                }
                for a in attrs {
                    match a.key.text.as_str() {
                        "wrap" => match s.len_value(&a.value, None) {
                            Some(Length::Em(x)) => set.wrap_em = Some(x),
                            Some(Length::Pt(_)) => s.type_err(&a.value, "wrap width is in em, such as `24em`".into()),
                            None => {}
                        },
                        _ => s.diags.push(bad_attr(a, &["wrap"])),
                    }
                }
            }
            StmtKind::Node { id, classes, attrs, .. } => {
                let Some(&v) = s.ids.get(&id.text) else { continue };
                if s.nodes[v].span != id.span {
                    continue;
                }
                let (mut color, mut shape) = (None, None);
                for c in classes {
                    match styles.get_mut(&c.text) {
                        Some(st) => {
                            st.2 = true;
                            look(&st.1, &mut color, &mut shape, &mut Vec::new());
                        }
                        None => {
                            let names: Vec<&str> = styles.keys().map(String::as_str).collect();
                            s.diags.push(unknown("E0004", "style", c, &names));
                        }
                    }
                }
                look(attrs, &mut color, &mut shape, &mut s.diags);
                (s.nodes[v].color, s.nodes[v].shape) = (color, shape);
            }
            StmtKind::Style { attrs, .. } => {
                // Checked once here; each use re-applies them silently.
                look(attrs, &mut None, &mut None, &mut s.diags);
            }
            StmtKind::Edge { from, link, to, label } => {
                let kind = if *link { EdgeKind::Link } else { EdgeKind::Tree };
                let a = s.node(from);
                for t in to {
                    if let (Some(a), Some(b)) = (a, s.node(t)) {
                        edges.push(Edge { a, b, kind, label: label.clone(), span: from.span.to(t.span) });
                    }
                }
            }
            StmtKind::Constraint { hard, rel, attrs } => {
                if let Some(c) = constraint(&mut s, *hard, rel, attrs, st.span) {
                    constraints.push(c);
                }
            }
            StmtKind::Body(_) => {}
        }
    }
    let mut unused: Vec<_> = styles.iter().filter(|(_, v)| !v.2).collect();
    unused.sort_by_key(|(_, v)| v.0.lo);
    for (name, (span, ..)) in unused {
        s.diags.push(Diag::warning("W0203", format!("style `{name}` is never used")).primary(*span, "declared here"));
    }
    s.diags.extend(contradictions(&constraints));
    if s.nodes.is_empty() {
        s.diags.push(Diag::error("E0002", "diagram has no nodes").primary(Span::new(0, 0), "expected at least one `node`"));
    }
    let g = Graph { title, root, nodes: s.nodes, edges, constraints, settings: set };
    (g, s.diags)
}

fn constraint(s: &mut Sema, hard: bool, rel: &Rel, attrs: &[Attr], span: Span) -> Option<Constraint> {
    let mut weight = 1;
    let mut gap = Length::Em(1.0);
    let mut within = Length::Em(4.0);
    let mut allowed = vec![];
    match rel {
        Rel::Pair { op, .. } if op.text == "near" => allowed.push("within"),
        Rel::Pair { .. } => allowed.push("gap"),
        _ => {}
    }
    if !hard {
        allowed.push("weight");
    }
    for a in attrs {
        match a.key.text.as_str() {
            k if !allowed.contains(&k) && k == "weight" => {
                let d = Diag::error("E0005", "`weight` only applies to `prefer`").primary(a.key.span, "`require` is never traded off");
                s.diags.push(d.help("use `prefer` for a soft constraint"));
            }
            k if !allowed.contains(&k) => s.diags.push(bad_attr(a, &allowed)),
            "weight" => match &a.value {
                Value::Num(t, _) if t.parse::<u32>().is_ok_and(|w| w > 0) => weight = t.parse().unwrap(),
                v => s.type_err(v, "weight must be a positive integer".into()),
            },
            "gap" => gap = s.len_value(&a.value, None)?,
            _ => within = s.len_value(&a.value, None)?,
        }
    }
    let rel = match rel {
        Rel::Pair { a, op, b } => {
            let (a, b) = (s.node(a), s.node(b));
            let (a, b) = (a?, b?);
            match op.text.as_str() {
                "left-of" => Relation::LeftOf { a, b, gap },
                "right-of" => Relation::LeftOf { a: b, b: a, gap },
                "above" => Relation::Above { a, b, gap },
                "below" => Relation::Above { a: b, b: a, gap },
                "near" => Relation::Near { a, b, within },
                _ => {
                    s.diags.push(unknown("E0009", "relation", op, RELATIONS));
                    return None;
                }
            }
        }
        Rel::Align { side, items } => {
            let ids: Vec<Option<usize>> = items.iter().map(|n| s.node(n)).collect();
            let side = match side.text.as_str() {
                "top" => Side::Top,
                "bottom" => Side::Bottom,
                "left" => Side::Left,
                "right" => Side::Right,
                "row" => Side::Row,
                "column" => Side::Column,
                _ => {
                    s.diags.push(unknown("E0009", "alignment", side, SIDES));
                    return None;
                }
            };
            if items.len() < 2 {
                s.diags.push(Diag::error("E0007", "alignment needs at least two nodes").primary(span, "only one node"));
                return None;
            }
            Relation::Align { side, items: ids.into_iter().collect::<Option<Vec<_>>>()? }
        }
        Rel::At { a, place: p } => {
            let a = s.node(a)?;
            match place(&p.text) {
                Some(place) => Relation::At { a, place },
                None => {
                    s.diags.push(unknown("E0009", "place", p, PLACES));
                    return None;
                }
            }
        }
    };
    let nodes = rel.nodes();
    if let Some(dup) = nodes.iter().enumerate().find(|(i, v)| nodes[..*i].contains(v)).map(|(_, &v)| v) {
        let d = Diag::error("E0007", format!("constraint names `{}` twice", s.nodes[dup].id)).primary(span, "a node cannot be related to itself");
        s.diags.push(d);
        return None;
    }
    Some(Constraint { rel, hard, weight, span })
}

/// contradictions finds hard ordering constraints that no layout can meet, without search.
///
/// Alignments merge nodes into classes per axis (union-find). An ordering between two nodes
/// of one class, or a cycle of orderings between classes, is a contradiction: a strict order
/// with a positive gap cannot loop back, and aligned boxes cannot sit strictly apart.
fn contradictions(cs: &[Constraint]) -> Vec<Diag> {
    let mut out = Vec::new();
    for vertical in [false, true] {
        let n = cs.iter().flat_map(|c| c.rel.nodes()).max().map_or(0, |m| m + 1);
        // Alignment graph: node pairs joined by an alignment, labeled by constraint index.
        let mut same: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
        let mut order: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
        for (ci, c) in cs.iter().enumerate().filter(|(_, c)| c.hard) {
            match &c.rel {
                Relation::Align { side, items } if side.vertical() == vertical => {
                    for w in items.windows(2) {
                        same[w[0]].push((w[1], ci));
                        same[w[1]].push((w[0], ci));
                    }
                }
                Relation::LeftOf { a, b, .. } if !vertical => order[*a].push((*b, ci)),
                Relation::Above { a, b, .. } if vertical => order[*a].push((*b, ci)),
                _ => {}
            }
        }
        if let Some(cycle) = find_cycle(&same, &order) {
            let axis = if vertical { "vertical" } else { "horizontal" };
            let mut d = Diag::error("E0008", format!("contradictory {axis} constraints"));
            for (k, &ci) in cycle.iter().enumerate() {
                let msg = if k == 0 { "these constraints cannot all hold" } else { "" };
                d = if k == 0 { d.primary(cs[ci].span, msg) } else { d.secondary(cs[ci].span, msg) };
            }
            out.push(d.note("following the orderings and alignments leads back to where they start"));
        }
    }
    out
}

/// find_cycle searches the graph whose vertices are alignment classes and whose arcs are
/// orderings. It returns the constraint indices along one cycle, including the alignments
/// that connect consecutive arcs.
fn find_cycle(same: &[Vec<(usize, usize)>], order: &[Vec<(usize, usize)>]) -> Option<Vec<usize>> {
    let n = same.len();
    // A state is a node plus whether an ordering arc has been used yet; a cycle must use one.
    // BFS from each start keeps the reported cycle short.
    for start in 0..n {
        let mut prev: HashMap<(usize, bool), ((usize, bool), usize)> = HashMap::new();
        let mut q = std::collections::VecDeque::from([(start, false)]);
        while let Some((u, used)) = q.pop_front() {
            let arcs = same[u].iter().map(|&a| (a, used)).chain(order[u].iter().map(|&a| (a, true)));
            for ((w, ci), now) in arcs {
                let next = (w, now);
                if next == (start, true) {
                    let mut path = vec![ci];
                    let mut at = (u, used);
                    while at != (start, false) {
                        let (p, c) = prev[&at];
                        path.push(c);
                        at = p;
                    }
                    path.reverse();
                    path.dedup();
                    return Some(path);
                }
                if next != (start, false) && !prev.contains_key(&next) {
                    prev.insert(next, ((u, used), ci));
                    q.push_back(next);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    fn run(src: &str) -> (Graph, Vec<Diag>) {
        let (stmts, errs) = parse(src);
        assert!(errs.is_empty(), "{:?}", errs.iter().map(|d| &d.msg).collect::<Vec<_>>());
        analyze(&stmts)
    }

    fn codes(src: &str) -> Vec<&'static str> {
        run(src).1.iter().map(|d| d.code).collect()
    }

    #[test]
    fn resolves_forward_references_and_styles() {
        let (g, d) = run("a -> b\nstyle hub shape=circle color=#123456\nnode a .hub\n| line\nnode b color=#abcdef\n");
        assert!(d.is_empty());
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.nodes[0].shape, Some(Shape::Circle));
        assert_eq!(g.nodes[0].body, vec!["line"]);
        assert_eq!(g.nodes[1].color.as_deref(), Some("#abcdef"));
    }

    #[test]
    fn suggests_close_names() {
        let (_, d) = run("node cyclic\nnode b\ncyclc -> b\n");
        assert_eq!(d[0].code, "E0001");
        assert_eq!(d[0].help.as_deref(), Some("did you mean `cyclic`?"));
    }

    #[test]
    fn types_values_and_units() {
        assert_eq!(codes("node a\nnode b\nrequire a left-of b gap=3\n"), vec!["E0006"]);
        assert_eq!(codes("node a\nnode b\nrequire a left-of b weight=3\n"), vec!["E0005"]);
        assert_eq!(codes("node a\nfont 7pt..48pt wrap=2pt\n"), vec!["E0006"]);
        assert_eq!(codes("node a\npage 594x420mm portrait margin=1cm\n"), Vec::<&str>::new());
        assert_eq!(codes("node a .nope\nstyle x\n"), vec!["E0004", "W0203"]);
    }

    #[test]
    fn rejects_ordering_cycles() {
        assert_eq!(codes("node a\nnode b\nnode c\nrequire a left-of b\nrequire b left-of c\nrequire c left-of a\n"), vec!["E0008"]);
        assert_eq!(codes("node a\nnode b\nrequire align row(a, b)\nrequire a above b\n"), vec!["E0008"]);
        assert_eq!(codes("node a\nnode b\nrequire align column(a, b)\nrequire a above b\n"), Vec::<&str>::new());
        assert_eq!(codes("node a\nnode b\nprefer a left-of b\nprefer b left-of a\n"), Vec::<&str>::new());
    }

    #[test]
    fn cycle_reports_every_constraint() {
        let (_, d) = run("node a\nnode b\nnode c\nrequire align top(a, b)\nrequire b above c\nrequire c above a\n");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].labels.len(), 3);
    }
}
