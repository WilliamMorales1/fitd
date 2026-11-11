use crate::ast::{Attr, Name, Rel, Stmt, StmtKind, Value};
use crate::diag::{Diag, Span};
use crate::graph::{
    Cell, Constraint, Dir, Edge, EdgeKind, Graph, KeyEntry, LayoutKind, Length, Node, Place,
    Relation, Settings, Shape, Side,
};
use crate::theme::{self, Overrides, Theme};
use std::collections::HashMap;

const MM: f64 = 72.0 / 25.4;

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

pub fn length(s: &str, bare: Option<&str>) -> Result<Length, String> {
    let cut = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());
    let x: f64 = s[..cut]
        .parse()
        .map_err(|_| format!("`{s}` is not a number"))?;
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

/// ratio reads a `W:H` page shape such as `16:9`.
pub fn ratio(s: &str) -> Option<Result<(f64, f64), String>> {
    let (w, h) = s.split_once(':')?;
    let num = |v: &str| v.parse::<f64>().ok().filter(|x| x.is_finite() && *x > 0.0);
    Some(match (num(w), num(h)) {
        (Some(w), Some(h)) if (w / h).max(h / w) <= 20.0 => Ok((w, h)),
        (Some(_), Some(_)) => Err(format!("page ratio `{s}` is more than 20:1")),
        _ => Err(format!("`{s}` is not a page ratio like `16:9`")),
    })
}

pub fn page_size(s: &str, bare: Option<&str>) -> Result<(f64, f64), String> {
    if let Some(p) = paper(s) {
        return Ok(p);
    }
    let (w, h) = s
        .split_once('x')
        .ok_or(format!("`{s}` is not a page size (a0..a4 or WxH)"))?;
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
    options
        .map(|o| (edit_distance(word, o), o))
        .filter(|&(d, _)| d <= limit)
        .min()
        .map(|(_, o)| format!("did you mean `{o}`?"))
}

fn unknown(code: &'static str, what: &str, n: &Name, options: &[&str]) -> Diag {
    let d = Diag::error(code, format!("unknown {what} `{}`", n.text))
        .primary(n.span, format!("not a known {what}"));
    match suggest(&n.text, options.iter().copied()) {
        Some(s) => d.help(s),
        None => d.note(format!("expected one of: {}", options.join(", "))),
    }
}

struct Sema {
    diags: Vec<Diag>,
    ids: HashMap<String, usize>,
    nodes: Vec<Node>,
    decl: Vec<Span>,
}

impl Sema {
    fn node(&mut self, n: &Name) -> Option<usize> {
        if let Some(&i) = self.ids.get(&n.text) {
            return Some(i);
        }
        let d = Diag::error("E0001", format!("unknown node `{}`", n.text))
            .primary(n.span, "not declared with `node`");
        let d = match suggest(&n.text, self.ids.keys().map(String::as_str)) {
            Some(s) => d.help(s),
            None => d,
        };
        self.diags.push(d);
        None
    }

    fn type_err(&mut self, v: &Value, msg: String) {
        self.diags
            .push(Diag::error("E0006", msg).primary(v.span(), "wrong type or unit"));
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
                self.type_err(
                    v,
                    "expected an absolute length (pt, mm, cm, in), not em".into(),
                );
                None
            }
        }
    }
}

fn color(v: &Value, diags: &mut Vec<Diag>) -> Option<String> {
    match v {
        Value::Color(c, _) | Value::Str(c, _) if theme::is_color(c) => Some(c.clone()),
        _ => {
            diags.push(
                Diag::error(
                    "E0006",
                    "expected a color like `#1565C0` or `#15C` (written right after `=`)",
                )
                .primary(v.span(), "wrong type or unit"),
            );
            None
        }
    }
}

#[derive(Default)]
struct Look {
    color: Option<String>,
    fill: Option<String>,
    text: Option<String>,
    shape: Option<Shape>,
    stack: Option<u8>,
    wrap_em: Option<f64>,
    aside: Option<bool>,
}

fn look(attrs: &[Attr], l: &mut Look, diags: &mut Vec<Diag>) {
    for a in attrs {
        match (a.key.text.as_str(), &a.value) {
            ("color", v) => l.color = color(v, diags).or(l.color.take()),
            ("fill", v) => l.fill = color(v, diags).or(l.fill.take()),
            ("text", v) => l.text = color(v, diags).or(l.text.take()),
            ("shape", Value::Name(n)) if n.text == "circle" => l.shape = Some(Shape::Circle),
            ("shape", Value::Name(n)) if n.text == "box" => l.shape = Some(Shape::Rect),
            ("shape", Value::Name(n)) if n.text == "none" => l.shape = Some(Shape::Plain),
            ("shape", Value::Name(n)) => {
                diags.push(unknown("E0009", "shape", n, &["circle", "box", "none"]))
            }
            ("shape", v) => diags.push(
                Diag::error("E0006", "expected `circle` or `box`")
                    .primary(v.span(), "wrong type or unit"),
            ),
            ("stack", Value::Num(t, _)) if t.parse::<u8>().is_ok_and(|k| (1..=8).contains(&k)) => {
                l.stack = t.parse().ok()
            }
            ("stack", v) => diags.push(
                Diag::error("E0006", "expected a whole number of copies, 1 to 8")
                    .primary(v.span(), "wrong type or unit"),
            ),
            ("wrap", Value::Num(t, _)) => match length(t, None) {
                Ok(Length::Em(x)) if x > 0.0 => l.wrap_em = Some(x),
                _ => diags.push(
                    Diag::error("E0006", "wrap width is in em, such as `24em`")
                        .primary(a.value.span(), "wrong type or unit"),
                ),
            },
            ("place", Value::Name(n)) if n.text == "aside" => l.aside = Some(true),
            ("place", Value::Name(n)) if n.text == "flow" => l.aside = Some(false),
            ("place", Value::Name(n)) => {
                diags.push(unknown("E0009", "place", n, &["flow", "aside"]))
            }
            ("place", v) => diags.push(
                Diag::error("E0006", "expected `flow` or `aside`")
                    .primary(v.span(), "wrong type or unit"),
            ),
            ("wrap", v) => diags.push(
                Diag::error("E0006", "wrap width is in em, such as `24em`")
                    .primary(v.span(), "wrong type or unit"),
            ),
            _ => diags.push(bad_attr(
                a,
                &["color", "fill", "text", "shape", "stack", "wrap", "place"],
            )),
        }
    }
}

type Styles = HashMap<String, (Span, Vec<Attr>, bool)>;

fn apply_styles(s: &mut Sema, styles: &mut Styles, classes: &[Name], l: &mut Look) {
    for c in classes {
        match styles.get_mut(&c.text) {
            Some(st) => {
                st.2 = true;
                look(&st.1, l, &mut Vec::new());
            }
            None => {
                let names: Vec<&str> = styles.keys().map(String::as_str).collect();
                s.diags.push(unknown("E0004", "style", c, &names));
            }
        }
    }
}

fn bad_attr(a: &Attr, allowed: &[&str]) -> Diag {
    let d = Diag::error("E0005", format!("unknown attribute `{}`", a.key.text))
        .primary(a.key.span, "not valid on this statement");
    match suggest(&a.key.text, allowed.iter().copied()) {
        Some(s) => d.help(s),
        None if allowed.is_empty() => d.note("this statement takes no attributes"),
        None => d.note(format!("allowed here: {}", allowed.join(", "))),
    }
}

const RELATIONS: &[&str] = &["left-of", "right-of", "above", "below", "near", "at"];
const SIDES: &[&str] = &["top", "bottom", "left", "right", "row", "column"];
const PLACES: &[&str] = &[
    "top-left",
    "top",
    "top-right",
    "left",
    "center",
    "right",
    "bottom-left",
    "bottom",
    "bottom-right",
];

fn place(s: &str) -> Option<Place> {
    PLACES
        .iter()
        .position(|&p| p == s)
        .map(|i| Place((i % 3) as u8, (i / 3) as u8))
}

/// span reads a `*N` column-span suffix, leaving the iterator untouched when there is none.
fn span(it: &mut std::iter::Peekable<std::str::Chars>) -> usize {
    if it.peek() != Some(&'*') {
        return 1;
    }
    let mut look = it.clone();
    look.next();
    let mut n = String::new();
    while look.peek().is_some_and(char::is_ascii_digit) {
        n.push(look.next().unwrap());
    }
    match n.parse() {
        Ok(k) if k >= 1 => {
            *it = look;
            k
        }
        _ => 1,
    }
}

/// cells splits one `~` figure row into cells: `#` solid, `%NN` shaded, `.` outlined, `_` gap.
fn cells(line: &str) -> Vec<Cell> {
    let mut out = Vec::new();
    let mut it = line.chars().peekable();
    while let Some(&c) = it.peek() {
        if c.is_whitespace() {
            it.next();
            continue;
        }
        let mut shade = 0.0;
        if c == '#' {
            it.next();
            shade = 1.0;
        } else if c == '%' {
            let mut look = it.clone();
            look.next();
            let mut n = String::new();
            while look.peek().is_some_and(char::is_ascii_digit) {
                n.push(look.next().unwrap());
            }
            if let Ok(v) = n.parse::<f64>() {
                shade = (v / 100.0).clamp(0.0, 1.0);
                it = look;
            }
        }
        let mut gap = false;
        let mut cols = 1;
        let label = match it.peek().copied() {
            None => None,
            Some(c) if c.is_whitespace() => None,
            Some('"') => {
                it.next();
                let mut s = String::new();
                for c in it.by_ref() {
                    if c == '"' {
                        break;
                    }
                    s.push(c);
                }
                cols = span(&mut it);
                Some(s)
            }
            Some(_) => {
                let mut s = String::new();
                while let Some(&c) = it.peek() {
                    if c.is_whitespace() {
                        break;
                    }
                    s.push(c);
                    it.next();
                }
                // `*N` is a span only when digits end the word, so a label like `a*t` survives.
                if let Some(i) = s.rfind('*') {
                    let tail = &s[i + 1..];
                    if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) {
                        cols = tail.parse().unwrap_or(1);
                        s.truncate(i);
                    }
                }
                match s.as_str() {
                    "." | "" => None,
                    "_" if shade == 0.0 => {
                        gap = true;
                        None
                    }
                    _ => Some(s),
                }
            }
        };
        out.push(Cell {
            shade,
            gap,
            span: cols.clamp(1, 64),
            label,
        });
    }
    out
}

pub fn analyze(stmts: &[Stmt]) -> (Graph, Vec<Diag>) {
    let mut s = Sema {
        diags: Vec::new(),
        ids: HashMap::new(),
        nodes: Vec::new(),
        decl: Vec::new(),
    };
    let mut styles: Styles = HashMap::new();

    let mut last_node = None;
    for st in stmts {
        match &st.kind {
            StmtKind::Node { id, title, .. } => {
                if let Some(&prev) = s.ids.get(&id.text) {
                    let d = Diag::error("E0003", format!("node `{}` declared twice", id.text))
                        .primary(id.span, "declared again here");
                    s.diags
                        .push(d.secondary(s.decl[prev], "first declared here"));
                    last_node = None;
                    continue;
                }
                s.ids.insert(id.text.clone(), s.nodes.len());
                let title = title
                    .as_ref()
                    .map(|t| t.0.clone())
                    .unwrap_or(id.text.clone());
                last_node = Some(s.nodes.len());
                s.nodes.push(Node {
                    id: id.text.clone(),
                    title,
                    body: Vec::new(),
                    figure: Vec::new(),
                    color: None,
                    fill: None,
                    text: None,
                    shape: None,
                    stack: None,
                    wrap_em: None,
                    aside: false,
                    span: id.span,
                });
                s.decl.push(st.span);
            }
            StmtKind::Cells(text) => match last_node {
                Some(v) => {
                    let row = cells(text);
                    if row.is_empty() {
                        s.diags.push(
                            Diag::error("E0002", "figure row has no cells")
                                .primary(st.span, "expected cells like `# . _ GLU`"),
                        );
                    } else {
                        s.nodes[v].figure.push(row);
                    }
                }
                None => s.diags.push(
                    Diag::error("E0002", "figure row outside a node")
                        .primary(st.span, "no `node` above this line"),
                ),
            },
            StmtKind::Body(text) => match last_node {
                Some(v) => s.nodes[v].body.push(text.clone()),
                None => s.diags.push(
                    Diag::error("E0002", "body line outside a node")
                        .primary(st.span, "no `node` above this line"),
                ),
            },
            StmtKind::Style { name, attrs } => {
                if let Some((prev, ..)) = styles.get(&name.text) {
                    let d = Diag::error("E0003", format!("style `{}` declared twice", name.text))
                        .primary(name.span, "declared again here");
                    s.diags.push(d.secondary(*prev, "first declared here"));
                } else {
                    styles.insert(name.text.clone(), (name.span, attrs.clone(), false));
                }
                last_node = None;
            }
            _ => last_node = None,
        }
    }

    let mut set = Settings::default();
    let (mut title, mut root, mut edges, mut constraints) = (None, None, Vec::new(), Vec::new());
    let mut seen_setting: HashMap<&str, Span> = HashMap::new();
    let mut grouped: Vec<(usize, Vec<Span>)> = Vec::new();
    let mut key_entries = Vec::new();
    // Style names used by groups and by anything else, to tell frame keys apart.
    let (mut on_group, mut on_other, mut key_classes) = (Vec::new(), Vec::new(), Vec::new());
    for st in stmts {
        let key = match &st.kind {
            StmtKind::Title(_) => "title",
            StmtKind::Root(_) => "root",
            StmtKind::Layout { .. } => "layout",
            StmtKind::Page { .. } => "page",
            StmtKind::Font { .. } => "font",
            StmtKind::Theme { .. } => "theme",
            _ => "",
        };
        if !key.is_empty()
            && let Some(prev) = seen_setting.insert(key, st.span)
        {
            let d = Diag::warning(
                "W0204",
                format!("`{key}` set more than once; the last one wins"),
            )
            .primary(st.span, "this one wins");
            s.diags.push(d.secondary(prev, "earlier setting ignored"));
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
                        &["dir", "bands", "order"]
                    }
                    _ => {
                        s.diags
                            .push(unknown("E0009", "layout", kind, &["radial", "layered"]));
                        &[]
                    }
                };
                for a in attrs {
                    match (a.key.text.as_str(), &a.value) {
                        (k, _) if !allowed.contains(&k) => s.diags.push(bad_attr(a, allowed)),
                        ("root", Value::Name(n)) => root = s.node(n).or(root),
                        ("root", v) => s.type_err(v, "expected a node name".into()),
                        ("dir", Value::Name(n)) if n.text == "down" => {
                            set.layout = Some(LayoutKind::Layered(Dir::Down))
                        }
                        ("dir", Value::Name(n)) if n.text == "right" => {
                            set.layout = Some(LayoutKind::Layered(Dir::Right))
                        }
                        ("dir", Value::Name(n)) => {
                            s.diags
                                .push(unknown("E0009", "direction", n, &["down", "right"]))
                        }
                        ("bands", Value::Num(t, _)) if t.parse::<usize>().is_ok_and(|k| k > 0) => {
                            set.bands = t.parse().ok()
                        }
                        ("bands", v) => {
                            s.type_err(v, "expected a whole number of bands, 1 or more".into())
                        }
                        ("order", Value::Name(n)) if n.text == "source" => set.source_order = true,
                        ("order", Value::Name(n)) if n.text == "auto" => set.source_order = false,
                        ("order", Value::Name(n)) => {
                            s.diags
                                .push(unknown("E0009", "order", n, &["auto", "source"]))
                        }
                        ("order", v) => s.type_err(v, "expected `auto` or `source`".into()),
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
                match text.map(|t| (t, ratio(t))) {
                    Some((_, Some(Ok(r)))) => {
                        set.page = None;
                        set.page_ratio = Some(r);
                        set.page_fixed = true;
                    }
                    Some((_, Some(Err(e)))) => s.type_err(size, e),
                    _ => {}
                }
                match text
                    .filter(|t| !t.contains(':'))
                    .map(|t| page_size(t, None))
                {
                    Some(Ok(p)) => {
                        set.page_ratio = None;
                        set.page = Some(p);
                        set.page_fixed =
                            text.is_some_and(|t| paper(t).is_none()) || !flags.is_empty();
                    }
                    Some(Err(e)) => s.type_err(size, e),
                    None if text.is_some_and(|t| t.contains(':')) => {}
                    None => s.type_err(
                        size,
                        "expected a page size like `a1`, `594x420mm`, or `16:9`".into(),
                    ),
                }
                for f in flags {
                    if let Some((w, h)) = set.page_ratio {
                        match f.text.as_str() {
                            "portrait" => set.page_ratio = Some((w.min(h), w.max(h))),
                            "landscape" => set.page_ratio = Some((w.max(h), w.min(h))),
                            _ => {}
                        }
                    }
                    match (f.text.as_str(), set.page) {
                        ("portrait", Some((w, h))) => set.page = Some((w.min(h), w.max(h))),
                        ("landscape", Some((w, h))) => set.page = Some((w.max(h), w.min(h))),
                        ("portrait" | "landscape", None) => {}
                        _ => s.diags.push(unknown(
                            "E0009",
                            "page flag",
                            f,
                            &["portrait", "landscape"],
                        )),
                    }
                }
                for a in attrs {
                    match a.key.text.as_str() {
                        "margin" => set.margin = s.abs_len(&a.value).or(set.margin),
                        "keysize" => set.key_size = s.abs_len(&a.value).or(set.key_size),
                        _ => s.diags.push(bad_attr(a, &["margin", "keysize"])),
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
                        set.font_span = Some(st.span);
                    }
                }
                for a in attrs {
                    match a.key.text.as_str() {
                        "wrap" => match s.len_value(&a.value, None) {
                            Some(Length::Em(x)) => set.wrap_em = Some(x),
                            Some(Length::Pt(_)) => {
                                s.type_err(&a.value, "wrap width is in em, such as `24em`".into())
                            }
                            None => {}
                        },
                        "family" => match &a.value {
                            Value::Str(f, _) if !f.trim().is_empty() => {
                                set.family = Some(f.trim().to_string())
                            }
                            v => s.type_err(
                                v,
                                "expected a quoted font family like `\"Inter\"`".into(),
                            ),
                        },
                        _ => s.diags.push(bad_attr(a, &["wrap", "family"])),
                    }
                }
            }
            StmtKind::Node {
                id, classes, attrs, ..
            } => {
                let Some(&v) = s.ids.get(&id.text) else {
                    continue;
                };
                if s.nodes[v].span != id.span {
                    continue;
                }
                let mut l = Look::default();
                apply_styles(&mut s, &mut styles, classes, &mut l);
                on_other.extend(classes.iter().map(|c| c.text.clone()));
                look(attrs, &mut l, &mut s.diags);
                let n = &mut s.nodes[v];
                (n.color, n.fill, n.text, n.shape) = (l.color, l.fill, l.text, l.shape);
                n.stack = l.stack;
                n.wrap_em = l.wrap_em;
                n.aside = l.aside.unwrap_or(false);
            }
            StmtKind::Style { attrs, .. } => {
                look(attrs, &mut Look::default(), &mut s.diags);
            }
            StmtKind::Theme { preset, attrs } => {
                if let Some(p) = preset {
                    match Theme::preset(&p.text) {
                        Some(_) => set.theme = Some(p.text.clone()),
                        None => s.diags.push(unknown("E0009", "theme", p, theme::PRESETS)),
                    }
                }
                set.theme_over = theme_overrides(attrs, &mut s.diags);
            }
            StmtKind::Edge {
                from,
                link,
                directed,
                to,
                label,
                classes,
                attrs,
            } => {
                let kind = if *link {
                    EdgeKind::Link
                } else {
                    EdgeKind::Tree
                };
                let mut l = Look::default();
                apply_styles(&mut s, &mut styles, classes, &mut l);
                on_other.extend(classes.iter().map(|c| c.text.clone()));
                for a in attrs {
                    match a.key.text.as_str() {
                        "color" => l.color = color(&a.value, &mut s.diags).or(l.color.take()),
                        _ => s.diags.push(bad_attr(a, &["color"])),
                    }
                }
                let a = s.node(from);
                for t in to {
                    if let (Some(a), Some(b)) = (a, s.node(t)) {
                        edges.push(Edge {
                            a,
                            b,
                            kind,
                            directed: *directed,
                            label: label.clone(),
                            color: l.color.clone(),
                            span: from.span.to(t.span),
                        });
                    }
                }
            }
            StmtKind::Key {
                label,
                classes,
                attrs,
            } => {
                let mut l = Look::default();
                apply_styles(&mut s, &mut styles, classes, &mut l);
                let mut line = None;
                let rest: Vec<Attr> = attrs
                    .iter()
                    .filter(|a| {
                        if a.key.text != "line" {
                            return true;
                        }
                        match &a.value {
                            Value::Name(n) if n.text == "solid" => line = Some(false),
                            Value::Name(n) if n.text == "dashed" => line = Some(true),
                            Value::Name(n) => {
                                s.diags
                                    .push(unknown("E0009", "line", n, &["solid", "dashed"]))
                            }
                            v => s.diags.push(
                                Diag::error("E0006", "expected `solid` or `dashed`")
                                    .primary(v.span(), "wrong type or unit"),
                            ),
                        }
                        false
                    })
                    .cloned()
                    .collect();
                look(&rest, &mut l, &mut s.diags);
                if line.is_none() && !classes.is_empty() {
                    key_classes.push((key_entries.len(), classes.clone()));
                }
                key_entries.push(KeyEntry {
                    label: label.clone(),
                    color: l.color,
                    fill: l.fill,
                    shape: l.shape,
                    line,
                    frame: false,
                });
            }
            StmtKind::Group {
                title,
                items,
                classes,
                attrs,
            } => {
                // Only a class's color applies; a group has no fill, shape, or stack.
                let mut l = Look::default();
                apply_styles(&mut s, &mut styles, classes, &mut l);
                on_group.extend(classes.iter().map(|c| c.text.clone()));
                let (mut col, mut times) = (l.color, None);
                for a in attrs {
                    match (a.key.text.as_str(), &a.value) {
                        ("color", _) => col = color(&a.value, &mut s.diags).or(col),
                        ("times", Value::Num(t, _))
                            if t.parse::<u16>().is_ok_and(|k| (2..=99).contains(&k)) =>
                        {
                            times = t.parse::<u16>().ok()
                        }
                        ("times", v) => s.diags.push(
                            Diag::error("E0006", "expected a whole number of repeats, 2 to 99")
                                .primary(v.span(), "wrong type or unit"),
                        ),
                        _ => s.diags.push(bad_attr(a, &["color", "times"])),
                    }
                }
                // `Nx` in front of the title is how a repeated stack is marked in the literature.
                let title = match (times, title) {
                    (Some(k), Some(t)) => Some(format!("{k}x {t}")),
                    (Some(k), None) => Some(format!("{k}x")),
                    (None, t) => t.clone(),
                };
                let ids: Vec<Option<usize>> = items.iter().map(|n| s.node(n)).collect();
                let Some(ids) = ids.into_iter().collect::<Option<Vec<_>>>() else {
                    continue;
                };
                if let Some(dup) = ids
                    .iter()
                    .enumerate()
                    .find(|(i, v)| ids[..*i].contains(v))
                    .map(|(_, &v)| v)
                {
                    s.diags.push(
                        Diag::error("E0007", format!("group names `{}` twice", s.nodes[dup].id))
                            .primary(st.span, "listed twice"),
                    );
                    continue;
                }
                grouped.push((constraints.len(), items.iter().map(|n| n.span).collect()));
                constraints.push(Constraint {
                    rel: Relation::Group {
                        items: ids,
                        title,
                        color: col,
                        title_em: 0.0,
                        dashed: times.is_some(),
                        inner: Vec::new(),
                    },
                    hard: true,
                    weight: 1,
                    span: st.span,
                });
            }
            StmtKind::Constraint { hard, rel, attrs } => {
                constraints.extend(constraint(&mut s, *hard, rel, attrs, st.span));
            }
            StmtKind::Body(_) | StmtKind::Cells(_) | StmtKind::Include(_) => {}
        }
    }
    for (i, classes) in key_classes {
        key_entries[i].frame = classes
            .iter()
            .all(|c| on_group.contains(&c.text) && !on_other.contains(&c.text));
    }
    let mut unused: Vec<_> = styles.iter().filter(|(_, v)| !v.2).collect();
    unused.sort_by_key(|(_, v)| v.0.lo);
    for (name, (span, ..)) in unused {
        s.diags.push(
            Diag::warning("W0203", format!("style `{name}` is never used"))
                .primary(*span, "declared here"),
        );
    }
    nest_groups(&mut constraints, &grouped, &s.nodes, &mut s.diags);
    s.diags.extend(contradictions(&constraints));
    for (v, n) in s.nodes.iter().enumerate().filter(|(_, n)| n.aside) {
        let flows = edges.iter().any(|e| {
            let other = if e.a == v { e.b } else { e.a };
            e.kind == EdgeKind::Tree && (e.a == v || e.b == v) && !s.nodes[other].aside
        });
        let grouped = constraints
            .iter()
            .any(|c| matches!(&c.rel, Relation::Group { items, .. } if items.contains(&v)));
        if flows || grouped {
            let why = if flows {
                "it is on a `->` edge"
            } else {
                "it is in a group"
            };
            s.diags.push(
                Diag::warning("W0208", format!("`place=aside` on `{}` is ignored", n.id))
                    .primary(n.span, why)
                    .help("aside nodes join the flow only by `..>` links; `->` may join two aside nodes"),
            );
        }
    }
    if s.nodes.is_empty() {
        s.diags.push(
            Diag::error("E0002", "diagram has no nodes")
                .primary(Span::new(0, 0), "expected at least one `node`"),
        );
    }
    let g = Graph {
        key: key_entries,
        title,
        root,
        nodes: s.nodes,
        edges,
        constraints,
        settings: set,
    };
    (g, s.diags)
}

fn nest_groups(
    constraints: &mut Vec<Constraint>,
    grouped: &[(usize, Vec<Span>)],
    nodes: &[Node],
    diags: &mut Vec<Diag>,
) {
    let items = |c: &Constraint| match &c.rel {
        Relation::Group { items, .. } => items.clone(),
        _ => unreachable!("grouped holds only groups"),
    };
    let within = |a: &[usize], b: &[usize]| a.len() < b.len() && a.iter().all(|v| b.contains(v));
    let mut kept: Vec<usize> = Vec::new();
    for (ci, spans) in grouped {
        let mine = items(&constraints[*ci]);
        let clash = kept.iter().find_map(|&cj| {
            let theirs = items(&constraints[cj]);
            let k = mine.iter().position(|v| theirs.contains(v))?;
            (!within(&mine, &theirs) && !within(&theirs, &mine)).then_some((cj, k))
        });
        match clash {
            Some((cj, k)) => {
                let d = Diag::error(
                    "E0010",
                    format!("`{}` is already in a group", nodes[mine[k]].id),
                )
                .primary(spans[k], "second group");
                diags.push(d.secondary(constraints[cj].span, "first group").help("a group may sit wholly inside another, but two groups cannot share only some nodes"));
            }
            None => kept.push(*ci),
        }
    }
    let dropped: Vec<usize> = grouped
        .iter()
        .map(|g| g.0)
        .filter(|ci| !kept.contains(ci))
        .collect();
    kept.sort_by_key(|&ci| items(&constraints[ci]).len());
    for (k, &ci) in kept.iter().enumerate() {
        let mine = items(&constraints[ci]);
        let parent = kept[k + 1..]
            .iter()
            .copied()
            .find(|&cj| within(&mine, &items(&constraints[cj])));
        if let Some(cj) = parent {
            let copy = constraints[ci].rel.clone();
            if let Relation::Group { inner, .. } = &mut constraints[cj].rel {
                inner.push(copy);
            }
        }
    }
    let mut i = 0;
    constraints.retain(|_| {
        i += 1;
        !dropped.contains(&(i - 1))
    });
}

fn theme_overrides(attrs: &[Attr], diags: &mut Vec<Diag>) -> Overrides {
    let mut o = Overrides::default();
    for a in attrs {
        let slot = match a.key.text.as_str() {
            "background" => &mut o.background,
            "gradient" => &mut o.gradient,
            "text" => &mut o.text,
            "edge" => &mut o.edge,
            "link" => &mut o.link,
            "label" => &mut o.label,
            "root" => &mut o.root,
            "palette" => {
                let colors: Vec<&str> = match &a.value {
                    Value::Str(p, _) => p
                        .split(|c: char| c == ',' || c.is_whitespace())
                        .filter(|c| !c.is_empty())
                        .collect(),
                    _ => vec![],
                };
                if !colors.is_empty() && colors.iter().all(|c| theme::is_color(c)) {
                    o.palette = Some(colors.iter().map(|c| c.to_string()).collect());
                } else {
                    diags.push(
                        Diag::error(
                            "E0006",
                            "expected a quoted list of colors like `\"#1565C0 #C62828\"`",
                        )
                        .primary(a.value.span(), "wrong type or unit"),
                    );
                }
                continue;
            }
            _ => {
                diags.push(bad_attr(a, theme::KEYS));
                continue;
            }
        };
        if let Some(c) = color(&a.value, diags) {
            *slot = Some(c);
        }
    }
    o
}

fn constraint(s: &mut Sema, hard: bool, rel: &Rel, attrs: &[Attr], span: Span) -> Vec<Constraint> {
    let mut weight = 1;
    let mut gap = Length::Em(1.0);
    let mut within = Length::Em(4.0);
    let mut allowed = vec![];
    if let Rel::Chain { ops, .. } = rel {
        if ops.iter().any(|o| o.text == "near") {
            allowed.push("within");
        }
        if ops.iter().any(|o| o.text != "near") {
            allowed.push("gap");
        }
    }
    if !hard {
        allowed.push("weight");
    }
    for a in attrs {
        match a.key.text.as_str() {
            k if !allowed.contains(&k) && k == "weight" => {
                let d = Diag::error("E0005", "`weight` only applies to `prefer`")
                    .primary(a.key.span, "`require` is never traded off");
                s.diags.push(d.help("use `prefer` for a soft constraint"));
            }
            k if !allowed.contains(&k) => s.diags.push(bad_attr(a, &allowed)),
            "weight" => match &a.value {
                Value::Num(t, _) if t.parse::<u32>().is_ok_and(|w| w > 0) => {
                    weight = t.parse().unwrap()
                }
                v => s.type_err(v, "weight must be a positive integer".into()),
            },
            "gap" => match s.len_value(&a.value, None) {
                Some(l) => gap = l,
                None => return vec![],
            },
            _ => match s.len_value(&a.value, None) {
                Some(l) => within = l,
                None => return vec![],
            },
        }
    }
    let rels = match rel {
        Rel::Chain { nodes, ops } => {
            let ids: Vec<Option<usize>> = nodes.iter().map(|n| s.node(n)).collect();
            let mut rels = vec![];
            for (i, op) in ops.iter().enumerate() {
                let (Some(a), Some(b)) = (ids[i], ids[i + 1]) else {
                    continue;
                };
                let r = match op.text.as_str() {
                    "left-of" => Relation::LeftOf { a, b, gap },
                    "right-of" => Relation::LeftOf { a: b, b: a, gap },
                    "above" => Relation::Above { a, b, gap },
                    "below" => Relation::Above { a: b, b: a, gap },
                    "near" => Relation::Near { a, b, within },
                    _ => {
                        s.diags.push(unknown("E0009", "relation", op, RELATIONS));
                        continue;
                    }
                };
                let link = if ops.len() == 1 { span } else { op.span };
                rels.push((r, link));
            }
            rels
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
                    return vec![];
                }
            };
            if items.len() < 2 {
                s.diags.push(
                    Diag::error("E0007", "alignment needs at least two nodes")
                        .primary(span, "only one node"),
                );
                return vec![];
            }
            let Some(items) = ids.into_iter().collect::<Option<Vec<_>>>() else {
                return vec![];
            };
            vec![(Relation::Align { side, items }, span)]
        }
        Rel::At { a, place: p } => {
            let Some(a) = s.node(a) else { return vec![] };
            match place(&p.text) {
                Some(place) => vec![(Relation::At { a, place }, span)],
                None => {
                    s.diags.push(unknown("E0009", "place", p, PLACES));
                    return vec![];
                }
            }
        }
    };
    let mut out = vec![];
    for (rel, span) in rels {
        let nodes: Vec<_> = rel.nodes().collect();
        if let Some(dup) = nodes
            .iter()
            .enumerate()
            .find(|(i, v)| nodes[..*i].contains(v))
            .map(|(_, &v)| v)
        {
            let d = Diag::error(
                "E0007",
                format!("constraint names `{}` twice", s.nodes[dup].id),
            )
            .primary(span, "a node cannot be related to itself");
            s.diags.push(d);
            continue;
        }
        out.push(Constraint {
            rel,
            hard,
            weight,
            span,
        });
    }
    out
}

fn contradictions(cs: &[Constraint]) -> Vec<Diag> {
    let mut out = Vec::new();
    for vertical in [false, true] {
        let n = cs
            .iter()
            .flat_map(|c| c.rel.nodes())
            .max()
            .map_or(0, |m| m + 1);
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
                let msg = if k == 0 {
                    "these constraints cannot all hold"
                } else {
                    ""
                };
                d = if k == 0 {
                    d.primary(cs[ci].span, msg)
                } else {
                    d.secondary(cs[ci].span, msg)
                };
            }
            out.push(
                d.note("following the orderings and alignments leads back to where they start"),
            );
        }
    }
    out
}

fn find_cycle(same: &[Vec<(usize, usize)>], order: &[Vec<(usize, usize)>]) -> Option<Vec<usize>> {
    let n = same.len();
    for start in 0..n {
        let mut prev: HashMap<(usize, bool), ((usize, bool), usize)> = HashMap::new();
        let mut q = std::collections::VecDeque::from([(start, false)]);
        while let Some((u, used)) = q.pop_front() {
            let arcs = same[u]
                .iter()
                .map(|&a| (a, used))
                .chain(order[u].iter().map(|&a| (a, true)));
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
        assert!(
            errs.is_empty(),
            "{:?}",
            errs.iter().map(|d| &d.msg).collect::<Vec<_>>()
        );
        analyze(&stmts)
    }

    fn codes(src: &str) -> Vec<&'static str> {
        run(src).1.iter().map(|d| d.code).collect()
    }

    #[test]
    fn page_ratios() {
        let (g, d) = run("page 16:9 portrait margin=10pt\nnode a\n");
        assert!(
            d.is_empty(),
            "{:?}",
            d.iter().map(|d| &d.msg).collect::<Vec<_>>()
        );
        assert_eq!(g.settings.page_ratio, Some((9.0, 16.0)));
        assert!(g.settings.page.is_none() && g.settings.page_fixed);
        let (g, _) = run("page 1.414:1\npage a3\nnode a\n");
        assert!(g.settings.page_ratio.is_none() && g.settings.page.is_some());
        assert_eq!(codes("page 3:0\nnode a\n"), ["E0006"]);
        assert_eq!(codes("page 30:1\nnode a\n"), ["E0006"]);
    }

    #[test]
    fn resolves_forward_references_and_styles() {
        let (g, d) = run(
            "a -> b\nstyle hub shape=circle color=#123456\nnode a .hub\n| line\nnode b color=#abcdef\n",
        );
        assert!(d.is_empty());
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.nodes[0].shape, Some(Shape::Circle));
        assert_eq!(g.nodes[0].body, vec!["line"]);
        assert_eq!(g.nodes[1].color.as_deref(), Some("#abcdef"));
    }

    #[test]
    fn themes_and_node_colors() {
        let (g, d) = run(
            "theme dark background=#000 palette=\"#f00, #0f0\"\nfont 7pt family=\"DejaVu Sans\"\nstyle s fill=#fff\nnode a .s text=#123\n",
        );
        assert!(
            d.is_empty(),
            "{:?}",
            d.iter().map(|d| &d.msg).collect::<Vec<_>>()
        );
        assert_eq!(g.settings.theme.as_deref(), Some("dark"));
        assert_eq!(
            g.settings.theme_over.palette.as_ref().map(Vec::len),
            Some(2)
        );
        assert_eq!(g.settings.family.as_deref(), Some("DejaVu Sans"));
        assert_eq!(
            (g.nodes[0].fill.as_deref(), g.nodes[0].text.as_deref()),
            (Some("#fff"), Some("#123"))
        );
        assert_eq!(codes("node a\ntheme drak\n"), vec!["E0009"]);
        assert_eq!(codes("node a\ntheme edge=blue\n"), vec!["E0006"]);
        assert_eq!(
            codes("node a\ntheme palette=\"#f00 nope\"\n"),
            vec!["E0006"]
        );
        assert_eq!(codes("node a\ntheme bg=#000\n"), vec!["E0005"]);
        assert_eq!(codes("node a fill=#12345\n"), vec!["E0006"]);
    }

    #[test]
    fn edge_colors_and_groups() {
        let (g, d) = run(
            "style hot color=#f00 shape=circle\nnode a\nnode b\nnode c\na -> b .hot\na -> c color=#00f\ngroup \"G\" b, c color=#0f0\n",
        );
        assert!(
            d.is_empty(),
            "{:?}",
            d.iter().map(|d| &d.msg).collect::<Vec<_>>()
        );
        assert_eq!(
            (g.edges[0].color.as_deref(), g.edges[1].color.as_deref()),
            (Some("#f00"), Some("#00f"))
        );
        assert!(
            matches!(&g.constraints[0], Constraint { rel: Relation::Group { items, title: Some(_), color: Some(_), .. }, hard: true, .. } if items == &[1, 2])
        );
        assert_eq!(codes("node a\nnode b\na -> b width=2pt\n"), vec!["E0005"]);
        assert_eq!(
            codes("node a\nnode b\nnode c\ngroup a, b\ngroup b, c\n"),
            vec!["E0010"]
        );
        assert_eq!(
            codes("node a\nnode b\ngroup a, b\ngroup b, a\n"),
            vec!["E0010"]
        );
        let (g, d) =
            run("node a\nnode b\nnode c\ngroup \"I\" b\ngroup \"O\" a, b, c\ngroup \"M\" b, c\n");
        assert!(d.is_empty());
        assert_eq!(g.constraints[1].rel.depth(), 3);
        assert!(
            matches!(&g.constraints[1].rel, Relation::Group { inner, .. } if inner.len() == 1 && inner[0].depth() == 2)
        );
        assert_eq!(codes("node a\ngroup a, a\n"), vec!["E0007"]);
        assert_eq!(codes("node a\ngroup a, ghost\n"), vec!["E0001"]);
    }

    #[test]
    fn keys_collect_entries() {
        let (g, d) =
            run("node a\nstyle s color=#f00\nkey \"X\" .s line=dashed\nkey \"Y\" shape=circle\n");
        assert!(d.is_empty());
        assert_eq!(g.key.len(), 2);
        assert_eq!(g.key[0].line, Some(true));
        assert_eq!(g.key[1].shape, Some(Shape::Circle));
        assert_eq!(codes("node a\nkey \"X\" line=wavy\n"), ["E0009"]);
    }

    #[test]
    fn suggests_close_names() {
        let (_, d) = run("node cyclic\nnode b\ncyclc -> b\n");
        assert_eq!(d[0].code, "E0001");
        assert_eq!(d[0].help.as_deref(), Some("did you mean `cyclic`?"));
    }

    #[test]
    fn types_values_and_units() {
        assert_eq!(
            codes("node a\nnode b\nrequire a left-of b gap=3\n"),
            vec!["E0006"]
        );
        assert_eq!(
            codes("node a\nnode b\nrequire a left-of b weight=3\n"),
            vec!["E0005"]
        );
        assert_eq!(codes("node a\nfont 7pt..48pt wrap=2pt\n"), vec!["E0006"]);
        assert_eq!(
            codes("node a\npage 594x420mm portrait margin=1cm\n"),
            Vec::<&str>::new()
        );
        assert_eq!(codes("node a .nope\nstyle x\n"), vec!["E0004", "W0203"]);
    }

    #[test]
    fn rejects_ordering_cycles() {
        assert_eq!(
            codes(
                "node a\nnode b\nnode c\nrequire a left-of b\nrequire b left-of c\nrequire c left-of a\n"
            ),
            vec!["E0008"]
        );
        assert_eq!(
            codes("node a\nnode b\nrequire align row(a, b)\nrequire a above b\n"),
            vec!["E0008"]
        );
        assert_eq!(
            codes("node a\nnode b\nrequire align column(a, b)\nrequire a above b\n"),
            Vec::<&str>::new()
        );
        assert_eq!(
            codes("node a\nnode b\nprefer a left-of b\nprefer b left-of a\n"),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn chains_expand_to_one_constraint_per_link() {
        let (g, d) = run("node a\nnode b\nnode c\nrequire a left-of b above c gap=2em\n");
        assert!(d.is_empty());
        assert!(matches!(
            g.constraints[..],
            [
                Constraint {
                    rel: Relation::LeftOf {
                        a: 0,
                        b: 1,
                        gap: Length::Em(2.0)
                    },
                    ..
                },
                Constraint {
                    rel: Relation::Above {
                        a: 1,
                        b: 2,
                        gap: Length::Em(2.0)
                    },
                    ..
                },
            ]
        ));
        assert_ne!(g.constraints[0].span, g.constraints[1].span);
        assert_eq!(
            codes("node a\nnode b\nnode c\nrequire a left-of b left-of c left-of a\n"),
            vec!["E0008"]
        );
        assert_eq!(
            codes("node a\nnode b\nnode c\nrequire a left-of b near c within=2em\n"),
            Vec::<&str>::new()
        );
        assert_eq!(
            codes("node a\nnode b\nnode c\nrequire a near b near c gap=2em\n"),
            vec!["E0005"]
        );
        assert_eq!(
            codes("node a\nnode b\nnode c\nrequire a left-of b sideways c\n"),
            vec!["E0009"]
        );
    }

    #[test]
    fn cycle_reports_every_constraint() {
        let (_, d) = run(
            "node a\nnode b\nnode c\nrequire align top(a, b)\nrequire b above c\nrequire c above a\n",
        );
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].labels.len(), 3);
    }
}
