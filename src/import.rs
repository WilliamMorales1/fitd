//! `fitd import`: translates Graphviz DOT, Mermaid flowcharts and mind maps, and D2 to fitd.
//! Nodes, labels, edges, and clusters, subgraphs, or containers (as nested `group`s) carry over;
//! everything else is dropped and listed, since fitd places nodes itself.

use std::collections::{BTreeSet, HashMap};

#[derive(Default)]
struct INode {
    id: String,
    label: Option<String>,
    color: Option<String>,
    fill: Option<String>,
    circle: bool,
    /// Innermost cluster the node was first mentioned in.
    group: Option<usize>,
}

struct IEdge {
    a: usize,
    b: usize,
    label: Option<String>,
    directed: bool,
    dashed: bool,
    color: Option<String>,
}

struct IGroup {
    title: Option<String>,
    name: String,
    /// The cluster this one is nested in.
    parent: Option<usize>,
}

/// Doc is a diagram read from another language, before it is written as fitd.
#[derive(Default)]
struct Doc {
    /// None for radial; Some(true) for a layered flow down, Some(false) for right.
    down: Option<bool>,
    /// The root of a radial map, when the source names one.
    root: Option<usize>,
    nodes: Vec<INode>,
    index: HashMap<String, usize>,
    edges: Vec<IEdge>,
    groups: Vec<IGroup>,
    dropped: BTreeSet<String>,
}

impl Doc {
    fn node(&mut self, id: &str, group: Option<usize>) -> usize {
        if let Some(&v) = self.index.get(id) {
            // A node mentioned again in a cluster nested inside its own moves into it.
            match (self.nodes[v].group, group) {
                (None, _) => self.nodes[v].group = group,
                (Some(old), Some(new)) if self.within(new, old) => self.nodes[v].group = group,
                (Some(old), Some(new)) if !self.within(old, new) => {
                    self.dropped.insert(format!("membership of `{id}` in a second cluster"));
                }
                _ => {}
            }
            return v;
        }
        self.index.insert(id.to_string(), self.nodes.len());
        self.nodes.push(INode { id: id.to_string(), group, ..INode::default() });
        self.nodes.len() - 1
    }

    /// within says whether cluster `g` is `k` or nested somewhere inside it.
    fn within(&self, g: usize, k: usize) -> bool {
        let mut g = Some(g);
        while let Some(h) = g {
            if h == k {
                return true;
            }
            g = self.groups[h].parent;
        }
        false
    }

    fn drop(&mut self, what: impl Into<String>) {
        self.dropped.insert(what.into());
    }
}

const KEYWORDS: &[&str] = &["title", "root", "layout", "page", "font", "theme", "style", "node", "group", "require", "prefer", "include"];

/// fitd_ids gives every node an id fitd accepts (letters, digits, `_`, inner `-`, not starting
/// with a digit, not a keyword), keeping the original where it already is one.
fn fitd_ids(names: &[&str]) -> Vec<String> {
    let mut used = BTreeSet::new();
    names
        .iter()
        .map(|name| {
            let mut id: String = name.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
            id = id.trim_matches('-').to_string();
            if id.is_empty() || id.starts_with(|c: char| c.is_ascii_digit()) || KEYWORDS.contains(&id.as_str()) {
                id = format!("n_{id}");
            }
            let base = id.clone();
            let mut k = 2;
            while !used.insert(id.clone()) {
                id = format!("{base}_{k}");
                k += 1;
            }
            id
        })
        .collect()
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
}

/// hex keeps a color fitd can use: `#RGB` or `#RRGGBB`.
fn hex(c: &str) -> Option<String> {
    let h = c.strip_prefix('#')?;
    (matches!(h.len(), 3 | 6) && h.chars().all(|c| c.is_ascii_hexdigit())).then(|| c.to_uppercase())
}

/// strip_html turns `<br>` into a newline and drops other tags.
fn strip_html(s: &str, dropped: &mut BTreeSet<String>) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        out += &rest[..i];
        let Some(j) = rest[i..].find('>') else { break };
        let tag = rest[i + 1..i + j].trim().trim_end_matches('/').trim().to_ascii_lowercase();
        if tag == "br" {
            out.push('\n');
        } else {
            dropped.insert("HTML markup in labels".into());
        }
        rest = &rest[i + j + 1..];
    }
    out + rest
}

impl Doc {
    fn write(mut self) -> (String, Vec<String>) {
        let names: Vec<&str> = self.nodes.iter().map(|n| n.id.as_str()).collect();
        let ids = fitd_ids(&names);
        let mut o = String::from("# Imported by `fitd import`; positions come from the solver.\n");
        match self.down {
            Some(true) => o += "layout layered\n",
            Some(false) => o += "layout layered dir=right\n",
            None => match self.root {
                Some(r) => o += &format!("layout radial root={}\n", ids[r]),
                None => o += "layout radial\n",
            },
        }
        o.push('\n');
        for (v, n) in self.nodes.iter().enumerate() {
            o += &format!("node {}", ids[v]);
            match &n.label {
                Some(l) => o += &format!(" {}", quote(l)),
                None if ids[v] != n.id => o += &format!(" {}", quote(&n.id)),
                None => {}
            }
            if let Some(c) = &n.color {
                o += &format!(" color={c}");
            }
            if let Some(c) = &n.fill {
                o += &format!(" fill={c}");
            }
            if n.circle {
                o += " shape=circle";
            }
            o.push('\n');
        }
        if !self.edges.is_empty() {
            o.push('\n');
        }
        for e in &self.edges {
            let op = match (e.dashed, e.directed) {
                (false, true) => "->",
                (false, false) => "--",
                (true, true) => "..>",
                (true, false) => "..",
            };
            o += &format!("{} {op} {}", ids[e.a], ids[e.b]);
            if let Some(l) = &e.label {
                o += &format!(" {}", quote(l));
            }
            if let Some(c) = &e.color {
                o += &format!(" color={c}");
            }
            o.push('\n');
        }
        let mut wrote_group = false;
        let mut written: Vec<Vec<&str>> = Vec::new();
        for (k, g) in self.groups.iter().enumerate() {
            let members: Vec<&str> = (0..self.nodes.len()).filter(|&v| self.nodes[v].group.is_some_and(|g| self.within(g, k))).map(|v| ids[v].as_str()).collect();
            if members.is_empty() {
                continue;
            }
            // fitd nests a group only inside a strictly larger one.
            if written.contains(&members) {
                self.dropped.insert(format!("cluster `{}`, which holds the same nodes as the one around it", g.name));
                continue;
            }
            written.push(members.clone());
            if !wrote_group {
                o.push('\n');
                wrote_group = true;
            }
            o += "group";
            if let Some(t) = &g.title {
                o += &format!(" {}", quote(t));
            }
            o += &format!(" {}\n", members.join(", "));
        }
        (o, self.dropped.into_iter().collect())
    }
}

// ---- DOT ----

#[derive(Clone, Debug, PartialEq)]
enum DTok {
    Id(String),
    /// A quoted string, kept apart from bare ids so `"node"` is a name, not a keyword.
    Quoted(String),
    Html(String),
    Punct(&'static str),
}

fn dot_lex(text: &str) -> Result<Vec<DTok>, String> {
    let cs: Vec<char> = text.chars().collect();
    let (mut out, mut i) = (Vec::new(), 0);
    let at_line_start = |i: usize| cs[..i].iter().rev().take_while(|&&c| c != '\n').all(|c| c.is_whitespace());
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '/' && cs.get(i + 1) == Some(&'/') || c == '#' && at_line_start(i) {
            while i < cs.len() && cs[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && cs.get(i + 1) == Some(&'*') {
            i += 2;
            while i < cs.len() && !(cs[i] == '*' && cs.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
        } else if c == '"' {
            let mut s = String::new();
            i += 1;
            while i < cs.len() && cs[i] != '"' {
                if cs[i] == '\\' && i + 1 < cs.len() {
                    match cs[i + 1] {
                        '"' => s.push('"'),
                        'n' | 'l' | 'r' => s.push('\n'),
                        '\n' => {}
                        o => {
                            s.push('\\');
                            s.push(o);
                        }
                    }
                    i += 2;
                } else {
                    s.push(cs[i]);
                    i += 1;
                }
            }
            i += 1;
            // `"a" + "b"` concatenates.
            match out.last_mut() {
                Some(DTok::Punct("+")) => {
                    out.pop();
                    if let Some(DTok::Quoted(prev)) = out.last_mut() {
                        prev.push_str(&s);
                    }
                }
                _ => out.push(DTok::Quoted(s)),
            }
        } else if c == '<' {
            let (mut depth, mut s) = (0, String::new());
            while i < cs.len() {
                match cs[i] {
                    '<' => depth += 1,
                    '>' => depth -= 1,
                    _ => {}
                }
                s.push(cs[i]);
                i += 1;
                if depth == 0 {
                    break;
                }
            }
            out.push(DTok::Html(s[1..s.len().saturating_sub(1)].to_string()));
        } else if c == '-' && matches!(cs.get(i + 1), Some('>' | '-')) {
            out.push(DTok::Punct(if cs[i + 1] == '>' { "->" } else { "--" }));
            i += 2;
        } else if c.is_alphanumeric() || c == '_' || c == '.' || c == '-' || !c.is_ascii() {
            let mut s = String::new();
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_' || cs[i] == '.' || !cs[i].is_ascii() || (cs[i] == '-' && !matches!(cs.get(i + 1), Some('>' | '-')))) {
                s.push(cs[i]);
                i += 1;
            }
            out.push(DTok::Id(s));
        } else {
            let p = match c {
                '{' => "{",
                '}' => "}",
                '[' => "[",
                ']' => "]",
                ';' => ";",
                ',' => ",",
                '=' => "=",
                ':' => ":",
                '+' => "+",
                _ => return Err(format!("unexpected `{c}`")),
            };
            out.push(DTok::Punct(p));
            i += 1;
        }
    }
    Ok(out)
}

struct DotParser {
    toks: Vec<DTok>,
    pos: usize,
    doc: Doc,
    directed: bool,
    /// Cluster stack; the innermost is last.
    clusters: Vec<usize>,
}

type Attrs = Vec<(String, String)>;

impl DotParser {
    fn peek(&self) -> Option<&DTok> {
        self.toks.get(self.pos)
    }

    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), Some(DTok::Punct(q)) if *q == p)
    }

    fn keyword(&self, k: &str) -> bool {
        matches!(self.peek(), Some(DTok::Id(s)) if s.eq_ignore_ascii_case(k))
    }

    fn expect(&mut self, p: &str) -> Result<(), String> {
        if self.is(p) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected `{p}`, found {:?}", self.peek()))
        }
    }

    fn id(&mut self) -> Result<String, String> {
        match self.toks.get(self.pos).cloned() {
            Some(DTok::Id(s) | DTok::Quoted(s)) => {
                self.pos += 1;
                Ok(s)
            }
            Some(DTok::Html(s)) => {
                self.pos += 1;
                Ok(strip_html(&s, &mut self.doc.dropped))
            }
            t => Err(format!("expected a name, found {t:?}")),
        }
    }

    fn attrs(&mut self) -> Result<Attrs, String> {
        let mut out = Vec::new();
        while self.is("[") {
            self.pos += 1;
            while !self.is("]") {
                let k = self.id()?;
                let v = if self.is("=") {
                    self.pos += 1;
                    self.id()?
                } else {
                    "true".into()
                };
                out.push((k.to_ascii_lowercase(), v));
                if self.is(",") || self.is(";") {
                    self.pos += 1;
                }
            }
            self.pos += 1;
        }
        Ok(out)
    }

    fn graph(&mut self) -> Result<(), String> {
        if self.keyword("strict") {
            self.pos += 1;
        }
        self.directed = if self.keyword("digraph") {
            true
        } else if self.keyword("graph") {
            false
        } else {
            return Err("expected `graph` or `digraph`".into());
        };
        self.pos += 1;
        self.doc.down = self.directed.then_some(true);
        if !self.is("{") {
            self.id()?;
        }
        self.expect("{")?;
        self.stmts(&[], &[])?;
        Ok(())
    }

    /// stmts reads up to the closing `}` and returns every node mentioned, for edges to a
    /// subgraph. Node and edge defaults are scoped to the block.
    fn stmts(&mut self, node_def: &[(String, String)], edge_def: &[(String, String)]) -> Result<Vec<usize>, String> {
        let (mut node_def, mut edge_def) = (node_def.to_vec(), edge_def.to_vec());
        let mut seen = Vec::new();
        while !self.is("}") {
            if self.peek().is_none() {
                return Err("missing `}`".into());
            }
            if self.is(";") {
                self.pos += 1;
                continue;
            }
            let kw = match self.peek() {
                Some(DTok::Id(s)) => s.to_ascii_lowercase(),
                _ => String::new(),
            };
            if matches!(kw.as_str(), "graph" | "node" | "edge") && matches!(self.toks.get(self.pos + 1), Some(DTok::Punct("["))) {
                self.pos += 1;
                let a = self.attrs()?;
                match kw.as_str() {
                    "node" => node_def.extend(a),
                    "edge" => edge_def.extend(a),
                    _ => self.graph_attrs(&a),
                }
                continue;
            }
            if matches!(self.toks.get(self.pos + 1), Some(DTok::Punct("="))) {
                let k = self.id()?.to_ascii_lowercase();
                self.pos += 1;
                let v = self.id()?;
                self.graph_attrs(&[(k, v)]);
                continue;
            }
            let mut ends = vec![self.endpoint(&node_def, &edge_def)?];
            let mut ops = Vec::new();
            while self.is("->") || self.is("--") {
                ops.push(self.is("->"));
                self.pos += 1;
                ends.push(self.endpoint(&node_def, &edge_def)?);
            }
            let a = self.attrs()?;
            if ops.is_empty() {
                if let [(Some(v), _)] = ends.as_slice() {
                    let a: Attrs = node_def.iter().cloned().chain(a).collect();
                    self.node_attrs(*v, &a);
                }
            } else {
                let a: Attrs = edge_def.iter().cloned().chain(a).collect();
                for (k, &directed) in ops.iter().enumerate() {
                    for &x in &ends[k].1 {
                        for &y in &ends[k + 1].1 {
                            self.edge(x, y, directed, &a);
                        }
                    }
                }
            }
            for (_, vs) in ends {
                seen.extend(vs);
            }
        }
        self.pos += 1;
        Ok(seen)
    }

    /// endpoint reads a node (with its port dropped) or a subgraph: (the node, every node).
    fn endpoint(&mut self, node_def: &[(String, String)], edge_def: &[(String, String)]) -> Result<(Option<usize>, Vec<usize>), String> {
        if self.keyword("subgraph") || self.is("{") {
            let mut name = String::new();
            if self.keyword("subgraph") {
                self.pos += 1;
                if !self.is("{") {
                    name = self.id()?;
                }
            }
            self.expect("{")?;
            let cluster = name.starts_with("cluster");
            if cluster {
                let parent = self.clusters.last().copied();
                self.doc.groups.push(IGroup { title: None, name: name.clone(), parent });
                self.clusters.push(self.doc.groups.len() - 1);
            }
            let vs = self.stmts(node_def, edge_def)?;
            if cluster {
                self.clusters.pop();
            }
            return Ok((None, vs));
        }
        let id = self.id()?;
        if self.is(":") {
            self.doc.drop("ports");
            self.pos += 1;
            self.id()?;
            if self.is(":") {
                self.pos += 1;
                self.id()?;
            }
        }
        let v = self.doc.node(&id, self.clusters.last().copied());
        if !node_def.is_empty() && self.doc.nodes[v].label.is_none() {
            let d = node_def.to_vec();
            self.node_attrs(v, &d);
        }
        Ok((Some(v), vec![v]))
    }

    fn graph_attrs(&mut self, a: &[(String, String)]) {
        for (k, v) in a {
            match (k.as_str(), self.clusters.last()) {
                ("label", Some(&g)) => self.doc.groups[g].title = Some(v.clone()),
                ("rankdir", None) if self.directed => match v.to_ascii_uppercase().as_str() {
                    "LR" => self.doc.down = Some(false),
                    "TB" => self.doc.down = Some(true),
                    "RL" => {
                        self.doc.down = Some(false);
                        self.doc.drop("rankdir=RL (drawn left to right)");
                    }
                    _ => self.doc.drop("rankdir=BT (drawn top to bottom)"),
                },
                ("color" | "fillcolor" | "style" | "bgcolor" | "fontname" | "fontsize", Some(_)) => self.doc.drop(format!("cluster attribute `{k}`")),
                _ => self.doc.drop(format!("graph attribute `{k}`")),
            }
        }
    }

    fn node_attrs(&mut self, v: usize, a: &[(String, String)]) {
        for (k, val) in a {
            match k.as_str() {
                "label" if val == "\\N" => {}
                "label" => self.doc.nodes[v].label = Some(val.clone()),
                "shape" if matches!(val.as_str(), "circle" | "doublecircle" | "point") => self.doc.nodes[v].circle = true,
                "shape" if matches!(val.as_str(), "box" | "rect" | "rectangle" | "ellipse" | "oval" | "plaintext" | "plain" | "none") => {}
                "color" if hex(val).is_some() => self.doc.nodes[v].color = hex(val),
                "fillcolor" if hex(val).is_some() => self.doc.nodes[v].fill = hex(val),
                "style" if val == "filled" => {}
                _ => self.doc.drop(format!("node attribute `{k}={val}`")),
            }
        }
    }

    fn edge(&mut self, a: usize, b: usize, directed: bool, attrs: &[(String, String)]) {
        let mut e = IEdge { a, b, label: None, directed, dashed: false, color: None };
        for (k, v) in attrs {
            match k.as_str() {
                "label" | "xlabel" => e.label = Some(v.clone()),
                "style" if matches!(v.as_str(), "dashed" | "dotted") => e.dashed = true,
                "color" if hex(v).is_some() => e.color = hex(v),
                "dir" if v == "none" => e.directed = false,
                "dir" if v == "back" => (e.a, e.b) = (b, a),
                _ => self.doc.drop(format!("edge attribute `{k}={v}`")),
            }
        }
        self.doc.edges.push(e);
    }
}

fn from_dot(text: &str) -> Result<Doc, String> {
    let mut p = DotParser { toks: dot_lex(text)?, pos: 0, doc: Doc::default(), directed: true, clusters: Vec::new() };
    p.graph()?;
    if p.pos < p.toks.len() {
        p.doc.drop("graphs after the first");
    }
    Ok(p.doc)
}

// ---- Mermaid ----

/// mermaid_node reads `id`, `id[text]`, `id(text)`, `id((text))`, `id{text}`, and the other
/// shapes, returning (id, text, circle) and the rest of the line.
fn mermaid_node(s: &str) -> Option<(String, Option<String>, bool, &str)> {
    let s = s.trim_start();
    let mut end = 0;
    for (i, c) in s.char_indices() {
        // A hyphen inside an id is fine; one that starts an arrow ends it.
        let arrow = c == '-' && (s[i..].starts_with("--") || s[i..].starts_with("-.") || s[i..].starts_with("->"));
        if !(c.is_alphanumeric() || c == '_' || c == '-' || !c.is_ascii()) || arrow {
            break;
        }
        end = i + c.len_utf8();
    }
    let (id, mut rest) = (s[..end].to_string(), &s[end..]);
    if id.is_empty() {
        return None;
    }
    rest = rest.strip_prefix(":::").map_or(rest, |r| r.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_' || c == '-'));
    const SHAPES: [(&str, &str, bool); 14] = [
        ("(((", ")))", true),
        ("((", "))", true),
        ("([", "])", false),
        ("[[", "]]", false),
        ("[(", ")]", false),
        ("[/", "/]", false),
        ("[\\", "\\]", false),
        ("[/", "\\]", false),
        ("[\\", "/]", false),
        ("{{", "}}", false),
        ("[", "]", false),
        ("(", ")", false),
        ("{", "}", false),
        (">", "]", false),
    ];
    for (open, close, circle) in SHAPES {
        if let Some(r) = rest.strip_prefix(open) {
            let (text, after) = match r.trim_start().strip_prefix('"') {
                Some(q) => {
                    let j = q.find('"')?;
                    (q[..j].to_string(), q[j + 1..].trim_start().strip_prefix(close)?)
                }
                None => {
                    let j = r.find(close)?;
                    (r[..j].trim().to_string(), &r[j + close.len()..])
                }
            };
            return Some((id, Some(text), circle, after));
        }
    }
    Some((id, None, false, rest))
}

/// mermaid_arrow reads a link: `-->`, `---`, `-.->`, `==>`, `--o`, `<-->`, with an optional
/// `|label|` or `-- label -->` text. Returns (directed, dotted, label, rest).
fn mermaid_arrow<'a>(s: &'a str, dropped: &mut BTreeSet<String>) -> Option<(bool, bool, Option<String>, &'a str)> {
    let s = s.trim_start();
    let run = |t: &str| t.find(|c: char| !matches!(c, '-' | '=' | '.' | '<' | '>')).unwrap_or(t.len());
    let n = run(s);
    if n < 2 {
        return None;
    }
    let (mut op, mut rest) = (s[..n].to_string(), &s[n..]);
    let mut label = None;
    if matches!(op.as_str(), "--" | "==" | "-.") && rest.starts_with(' ') {
        // `A -- text --> B`: the text runs to the closing half of the arrow.
        let close = ["-->", "---", "==>", "===", ".->", ".-"].iter().filter_map(|c| rest.find(c).map(|i| (i, *c))).min()?;
        label = Some(rest[..close.0].trim().to_string());
        let after = &rest[close.0..];
        let m = run(after);
        op += &after[..m];
        rest = &after[m..];
    }
    if let Some(r) = rest.strip_prefix(['o', 'x']).filter(|r| r.starts_with(' ')) {
        dropped.insert("circle and cross arrowheads".into());
        op.push('>');
        rest = r;
    }
    if op.starts_with('<') {
        dropped.insert("two-headed arrows (drawn undirected)".into());
    }
    if op.contains('=') {
        dropped.insert("thick links".into());
    }
    let directed = op.ends_with('>') && !op.starts_with('<');
    let dotted = op.contains('.');
    let r = rest.trim_start();
    if let Some(r) = r.strip_prefix('|') {
        let j = r.find('|')?;
        label = Some(r[..j].trim().trim_matches('"').to_string());
        rest = &r[j + 1..];
    }
    Some((directed, dotted, label, rest))
}

fn from_mermaid(text: &str) -> Result<Doc, String> {
    let mut doc = Doc::default();
    let mut header = false;
    let mut clusters: Vec<usize> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.split("%%").next().unwrap_or("").trim().trim_end_matches(';');
        if line.is_empty() {
            continue;
        }
        let word = line.split_whitespace().next().unwrap_or("");
        if !header {
            if word != "flowchart" && word != "graph" {
                return Err(format!("line {}: only Mermaid flowcharts (`flowchart TD`) can be imported", i + 1));
            }
            header = true;
            doc.down = Some(match line.split_whitespace().nth(1).unwrap_or("TD").to_ascii_uppercase().as_str() {
                "LR" => false,
                "RL" => {
                    doc.drop("direction RL (drawn left to right)");
                    false
                }
                "BT" => {
                    doc.drop("direction BT (drawn top to bottom)");
                    true
                }
                _ => true,
            });
            continue;
        }
        match word {
            "subgraph" => {
                let rest = line["subgraph".len()..].trim();
                // `subgraph id [Title]`, or `subgraph Title` alone.
                let (name, title) = match (rest.find('['), rest.strip_suffix(']')) {
                    (Some(j), Some(inner)) => (rest[..j].trim().to_string(), inner[j + 1..].trim().trim_matches('"').to_string()),
                    _ => (rest.to_string(), rest.trim_matches('"').to_string()),
                };
                doc.groups.push(IGroup { title: Some(title).filter(|t| !t.is_empty()), name, parent: clusters.last().copied() });
                clusters.push(doc.groups.len() - 1);
                continue;
            }
            "end" => {
                clusters.pop();
                continue;
            }
            "direction" => {
                doc.drop("subgraph direction");
                continue;
            }
            "classDef" | "class" | "style" | "linkStyle" | "click" | "accTitle" | "accDescr" => {
                doc.drop(format!("`{word}` statements"));
                continue;
            }
            _ => {}
        }
        if line.contains(":::") {
            doc.drop("`:::` classes");
        }
        // A chain of node groups (`A & B`) joined by arrows.
        let group = clusters.last().copied();
        let mut rest = line;
        let mut prev: Option<Vec<usize>> = None;
        let mut pending: Option<(bool, bool, Option<String>)> = None;
        loop {
            let mut here = Vec::new();
            loop {
                let Some((id, text, circle, r)) = mermaid_node(rest) else { return Err(format!("line {}: cannot read `{raw}`", i + 1)) };
                let v = doc.node(&id, group);
                if let Some(t) = text {
                    doc.nodes[v].label = Some(strip_html(&t, &mut doc.dropped));
                }
                doc.nodes[v].circle |= circle;
                here.push(v);
                rest = r.trim_start();
                match rest.strip_prefix('&') {
                    Some(r) => rest = r,
                    None => break,
                }
            }
            if let Some(from) = prev.take() {
                let (directed, dashed, label) = pending.take().expect("arrow read before targets");
                for &a in &from {
                    for &b in &here {
                        doc.edges.push(IEdge { a, b, label: label.clone(), directed, dashed, color: None });
                    }
                }
            }
            if rest.is_empty() {
                break;
            }
            let Some((directed, dashed, label, r)) = mermaid_arrow(rest, &mut doc.dropped) else { return Err(format!("line {}: cannot read `{raw}`", i + 1)) };
            pending = Some((directed, dashed, label));
            prev = Some(here);
            rest = r;
        }
    }
    if !header {
        return Err("empty Mermaid file".into());
    }
    Ok(doc)
}

// ---- D2 ----

/// D2Seg is one D2 statement, with its line and whether a `{` block follows it, or a `}`.
enum D2Seg {
    Stmt(String, usize, bool),
    Close,
}

/// d2_lex splits D2 source into statements at newlines, `;`, `{`, and `}` outside quotes,
/// dropping `#` comments. A `|`-delimited block string becomes one quoted value.
fn d2_lex(text: &str) -> Result<Vec<D2Seg>, String> {
    let mut out = Vec::new();
    let (mut buf, mut line, mut quote, mut depth) = (String::new(), 1, None, 0usize);
    let mut it = text.chars().peekable();
    let flush = |buf: &mut String, out: &mut Vec<D2Seg>, line: usize, opens: bool| {
        if !buf.trim().is_empty() || opens {
            out.push(D2Seg::Stmt(buf.trim().to_string(), line, opens));
        }
        buf.clear();
    };
    while let Some(c) = it.next() {
        if let Some(q) = quote {
            buf.push(c);
            if c == '\\' {
                buf.extend(it.next());
            } else if c == q {
                quote = None;
            } else if c == '\n' {
                return Err(format!("line {line}: unterminated string"));
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                buf.push(c);
            }
            // `${name}` substitutes a variable rather than opening a block.
            '$' if it.peek() == Some(&'{') => {
                buf.push(c);
                for c in it.by_ref() {
                    buf.push(c);
                    if c == '}' {
                        break;
                    }
                }
            }
            '#' => {
                while it.peek().is_some_and(|&c| c != '\n') {
                    it.next();
                }
            }
            '|' if buf.trim_end().ends_with(':') => {
                // `|md ... |`: as many pipes close it as opened it.
                let mut n = 1;
                while it.next_if_eq(&'|').is_some() {
                    n += 1;
                }
                let close = "|".repeat(n);
                let mut body = String::new();
                while !body.ends_with(&close) {
                    let Some(c) = it.next() else { return Err(format!("line {line}: unterminated block string")) };
                    line += usize::from(c == '\n');
                    body.push(c);
                }
                body.truncate(body.len() - n);
                // A language tag (`md`, `latex`) runs to the first space or newline; a
                // Markdown heading keeps its text.
                let (lang, body) = body.split_once(char::is_whitespace).unwrap_or(("", ""));
                let lines: Vec<&str> = body.trim().lines().map(|l| if lang == "md" { l.trim().trim_start_matches('#').trim() } else { l.trim() }).collect();
                buf += &format!(" \"{}\"", lines.join("\n").replace('\\', "\\\\").replace('"', "\\\""));
            }
            // `;` separates statements, except inside an array such as `class: [a; b]`.
            '[' | ']' => {
                depth = if c == '[' { depth + 1 } else { depth.saturating_sub(1) };
                buf.push(c);
            }
            ';' if depth > 0 => buf.push(c),
            '\n' | ';' => {
                depth = 0;
                flush(&mut buf, &mut out, line, false);
                line += usize::from(c == '\n');
            }
            '{' => flush(&mut buf, &mut out, line, true),
            '}' => {
                flush(&mut buf, &mut out, line, false);
                out.push(D2Seg::Close);
            }
            _ => buf.push(c),
        }
    }
    flush(&mut buf, &mut out, line, false);
    Ok(out)
}

/// d2_split cuts `s` at each occurrence of a separator outside quotes, returning the pieces
/// and the separators between them. Earlier separators in `seps` win at the same spot.
fn d2_split<'a>(s: &'a str, seps: &[&'a str]) -> (Vec<&'a str>, Vec<&'a str>) {
    let (mut parts, mut found) = (Vec::new(), Vec::new());
    let (mut start, mut quote, mut i) = (0, None, 0);
    let b = s.as_bytes();
    while i < s.len() {
        let c = b[i];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) if c == b'\\' => i += 1,
            Some(_) => {}
            None if c == b'"' || c == b'\'' => quote = Some(c),
            None => {
                if let Some(sep) = seps.iter().find(|sep| s[i..].starts_with(**sep)) {
                    parts.push(&s[start..i]);
                    found.push(*sep);
                    i += sep.len();
                    start = i;
                    continue;
                }
            }
        }
        i += 1;
    }
    parts.push(&s[start..]);
    (parts, found)
}

/// d2_unquote reads a key or value: a quoted string loses its quotes and escapes, a bare one
/// its surrounding space.
fn d2_unquote(s: &str) -> String {
    let s = s.trim();
    match s.chars().next() {
        Some(q @ ('"' | '\'')) if s.len() >= 2 && s.ends_with(q) => {
            let mut out = String::new();
            let mut it = s[1..s.len() - 1].chars();
            while let Some(c) = it.next() {
                match (c, c == '\\') {
                    (_, true) => match it.next() {
                        Some('n') => out.push('\n'),
                        Some(o) => out.push(o),
                        None => {}
                    },
                    _ => out.push(c),
                }
            }
            out
        }
        _ => s.to_string(),
    }
}

/// d2_subst replaces each `${name}` in `s` with its variable from `vars`.
fn d2_subst(s: &str, vars: &HashMap<String, String>, dropped: &mut BTreeSet<String>) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("${") {
        let Some(j) = rest[i..].find('}') else { break };
        out += &rest[..i];
        let name = rest[i + 2..i + j].trim();
        match vars.get(name) {
            Some(v) => out += v,
            None => {
                dropped.insert(format!("undefined variable `{name}`"));
                out += &rest[i..i + j + 1];
            }
        }
        rest = &rest[i + j + 1..];
    }
    out + rest
}

/// Keys that set a property of the object they follow rather than name a child.
const D2_PROPS: &[&str] = &[
    "shape", "label", "style", "icon", "near", "tooltip", "link", "width", "height", "class", "constraint", "direction", "grid-rows", "grid-columns",
    "grid-gap", "source-arrowhead", "target-arrowhead", "top", "left", "vertical-gap", "horizontal-gap",
];

#[derive(Default)]
struct D2Node {
    label: Option<String>,
    circle: bool,
    color: Option<String>,
    fill: Option<String>,
    parent: Option<usize>,
    container: bool,
    /// Names from `class`, applied once every class is known.
    classes: Vec<String>,
}

/// D2Scope is what the statements inside a `{` block apply to.
enum D2Scope {
    /// Children and properties of a container, by its path ("" for the top level).
    Container(String),
    /// Properties of the edges a statement drew.
    Edges(Vec<usize>),
    /// Properties under a prefix (`style: {`) of an object, or of edges when it is None.
    Props(Option<String>, Vec<usize>, String),
    /// The `classes` block, and inside it one class's properties under a prefix.
    Classes,
    Class(String, String),
    /// The `vars` block, under the path of the variable being defined.
    Vars(String),
    Skip,
}

struct D2Doc {
    paths: Vec<String>,
    index: HashMap<String, usize>,
    nodes: Vec<D2Node>,
    edges: Vec<IEdge>,
    direction: Option<String>,
    /// Each class's properties, as (path, value), in source order.
    classes: HashMap<String, Vec<(String, String)>>,
    vars: HashMap<String, String>,
    dropped: BTreeSet<String>,
}

impl D2Doc {
    /// object finds or makes the object at `path`, and its ancestors.
    fn object(&mut self, path: &str) -> usize {
        if let Some(&v) = self.index.get(path) {
            return v;
        }
        let parent = path.rsplit_once('.').map(|(p, _)| self.object(p));
        if let Some(p) = parent {
            self.nodes[p].container = true;
        }
        self.index.insert(path.to_string(), self.nodes.len());
        self.paths.push(path.to_string());
        self.nodes.push(D2Node { parent, ..D2Node::default() });
        self.nodes.len() - 1
    }

    /// resolve joins a key to the container it is written in; `_` steps out one level.
    fn resolve(prefix: &str, key: &[String]) -> String {
        let mut segs: Vec<String> = prefix.split('.').filter(|s| !s.is_empty()).map(String::from).collect();
        for k in key {
            if k == "_" {
                segs.pop();
            } else {
                segs.push(k.replace('.', "_"));
            }
        }
        segs.join(".")
    }

    /// prop applies a property path (`style.fill`) to an object, or to edges when `obj` is None.
    fn prop(&mut self, obj: Option<usize>, edges: &[usize], prop: &str, value: &str) {
        let hex_or_drop = |d: &mut BTreeSet<String>, v: &str| {
            let h = hex(v);
            if h.is_none() {
                d.insert(format!("color `{v}` (only #hex carries over)"));
            }
            h
        };
        match (obj, prop) {
            (None, "label") => edges.iter().for_each(|&e| self.edges[e].label = Some(value.to_string())),
            (None, "style.stroke-dash") => edges.iter().for_each(|&e| self.edges[e].dashed = value.trim() != "0"),
            (None, "style.stroke") => {
                let c = hex_or_drop(&mut self.dropped, value);
                edges.iter().for_each(|&e| self.edges[e].color = c.clone());
            }
            (Some(v), "label") => self.nodes[v].label = Some(value.to_string()),
            (Some(v), "shape") => match value {
                "circle" | "oval" => self.nodes[v].circle = true,
                "rectangle" | "square" => {}
                s => {
                    self.dropped.insert(format!("shape `{s}` (drawn as a box)"));
                }
            },
            (Some(v), "style.fill") => self.nodes[v].fill = hex_or_drop(&mut self.dropped, value),
            (Some(v), "style.stroke") => self.nodes[v].color = hex_or_drop(&mut self.dropped, value),
            (Some(v), "class") => {
                let names = value.trim().trim_start_matches('[').trim_end_matches(']');
                self.nodes[v].classes.extend(names.split(';').map(|c| d2_unquote(c.trim())).filter(|c| !c.is_empty()));
            }
            (None, "class") => {
                self.dropped.insert("classes on connections".into());
            }
            (_, p) => {
                self.dropped.insert(format!("`{p}`"));
            }
        }
    }
}

/// from_d2 reads D2: objects, labels, connections, containers (as groups), classes, vars, and the shape,
/// label, fill, and stroke of objects and connections. D2 lays out top to bottom by default.
fn from_d2(text: &str) -> Result<Doc, String> {
    let mut d = D2Doc {
        paths: Vec::new(),
        index: HashMap::new(),
        nodes: Vec::new(),
        edges: Vec::new(),
        direction: None,
        classes: HashMap::new(),
        vars: HashMap::new(),
        dropped: BTreeSet::new(),
    };
    let mut stack = vec![D2Scope::Container(String::new())];
    for seg in d2_lex(text)? {
        let (text, line, opens) = match seg {
            D2Seg::Close => {
                if stack.len() > 1 {
                    stack.pop();
                }
                continue;
            }
            D2Seg::Stmt(t, l, o) => (t, l, o),
        };
        let (kv, _) = d2_split(&text, &[":"]);
        let (left, value) = match kv.split_first() {
            Some((l, rest)) if !rest.is_empty() => (l.trim(), Some(d2_unquote(&text[l.len() + 1..]))),
            _ => (text.trim(), None),
        };
        let value = value.filter(|v| !v.is_empty());
        let keys = |s: &str| d2_split(s, &["."]).0.into_iter().map(d2_unquote).collect::<Vec<_>>();
        let scope = match stack.last().unwrap() {
            D2Scope::Skip => {
                if opens {
                    stack.push(D2Scope::Skip);
                }
                continue;
            }
            D2Scope::Edges(es) => D2Scope::Props(None, es.clone(), String::new()),
            D2Scope::Classes => {
                let k = keys(left);
                D2Scope::Class(k[0].clone(), k[1..].join("."))
            }
            D2Scope::Class(name, pre) => D2Scope::Class(name.clone(), if pre.is_empty() { keys(left).join(".") } else { format!("{pre}.{}", keys(left).join(".")) }),
            D2Scope::Props(o, es, pre) => D2Scope::Props(o.clone(), es.clone(), pre.clone()),
            D2Scope::Vars(pre) => D2Scope::Vars(if pre.is_empty() { keys(left).join(".") } else { format!("{pre}.{}", keys(left).join(".")) }),
            D2Scope::Container(p) => D2Scope::Container(p.clone()),
        };
        match scope {
            D2Scope::Vars(name) if name == "d2-config" => {
                d.dropped.insert("`d2-config`".into());
                if opens {
                    stack.push(D2Scope::Skip);
                }
            }
            D2Scope::Vars(name) => match (value, opens) {
                (_, true) => stack.push(D2Scope::Vars(name)),
                (Some(val), false) => {
                    d.vars.insert(name, val);
                }
                (None, false) => {}
            },
            // `name: {` or `name.style: {` opens a class; inside, a property opens a prefix or is set.
            D2Scope::Class(name, prop) => match (value, opens) {
                (_, true) => stack.push(D2Scope::Class(name, prop)),
                (Some(val), false) if !prop.is_empty() => d.classes.entry(name).or_default().push((prop, val)),
                _ => {
                    d.dropped.insert("`classes` without a block".into());
                }
            },
            D2Scope::Props(obj, edges, pre) => {
                let prop = if pre.is_empty() { keys(left).join(".") } else { format!("{pre}.{}", keys(left).join(".")) };
                let v = obj.as_ref().map(|p| d.object(p));
                match (&value, opens) {
                    (_, true) => stack.push(D2Scope::Props(obj, edges, prop)),
                    (Some(val), false) => d.prop(v, &edges, &prop, val),
                    (None, false) => {
                        d.dropped.insert(format!("`{prop}`"));
                    }
                }
            }
            D2Scope::Container(prefix) => {
                let (ends, ops) = d2_split(left, &["<->", "->", "<-", "--"]);
                if prefix.is_empty() && left == "classes" && opens {
                    stack.push(D2Scope::Classes);
                    continue;
                }
                if prefix.is_empty() && left == "vars" && opens {
                    stack.push(D2Scope::Vars(String::new()));
                    continue;
                }
                if left.starts_with('(') || ["vars", "classes", "layers", "scenarios", "steps", "imports", "...@"].iter().any(|k| left.starts_with(k)) {
                    d.dropped.insert(format!("`{}`", left.split(['.', ' ', '[']).next().unwrap_or(left)));
                    if opens {
                        stack.push(D2Scope::Skip);
                    }
                    continue;
                }
                if !ops.is_empty() {
                    let objs: Vec<usize> = ends.iter().map(|e| d.object(&D2Doc::resolve(&prefix, &keys(e)))).collect();
                    let mut drawn = Vec::new();
                    for (k, op) in ops.iter().enumerate() {
                        let (a, b) = if *op == "<-" { (objs[k + 1], objs[k]) } else { (objs[k], objs[k + 1]) };
                        if *op == "<->" {
                            d.dropped.insert("two-headed arrows (drawn undirected)".into());
                        }
                        drawn.push(d.edges.len());
                        d.edges.push(IEdge { a, b, label: value.clone(), directed: matches!(*op, "->" | "<-"), dashed: false, color: None });
                    }
                    if opens {
                        stack.push(D2Scope::Edges(drawn));
                    }
                    continue;
                }
                let segs = keys(left);
                if segs.iter().any(|s| s.is_empty()) {
                    return Err(format!("line {line}: cannot read `{text}`"));
                }
                let k = segs.iter().position(|s| D2_PROPS.contains(&s.as_str())).unwrap_or(segs.len());
                let path = D2Doc::resolve(&prefix, &segs[..k]);
                if k == segs.len() {
                    let v = d.object(&path);
                    match value.as_deref() {
                        Some("null") => {
                            d.dropped.insert("`null` deletions".into());
                        }
                        Some(l) => d.nodes[v].label = Some(l.to_string()),
                        None => {}
                    }
                    if opens {
                        stack.push(D2Scope::Container(path));
                    }
                    continue;
                }
                let prop = segs[k..].join(".");
                if path.is_empty() {
                    match (prop.as_str(), value.as_deref()) {
                        ("direction", Some(dir)) => d.direction = Some(dir.to_string()),
                        _ => {
                            d.dropped.insert(format!("`{prop}`"));
                        }
                    }
                    if opens {
                        stack.push(D2Scope::Skip);
                    }
                    continue;
                }
                if opens {
                    stack.push(D2Scope::Props(Some(path), Vec::new(), prop));
                    continue;
                }
                let v = d.object(&path);
                match value {
                    Some(_) if prop == "direction" => {
                        d.dropped.insert("container direction".into());
                    }
                    Some(val) => d.prop(Some(v), &[], &prop, &val),
                    None => {
                        d.dropped.insert(format!("`{prop}`"));
                    }
                }
            }
            D2Scope::Edges(_) | D2Scope::Classes | D2Scope::Skip => unreachable!("mapped to Props or Class, or skipped above"),
        }
    }
    // An object's own style wins over its classes', and a later class over an earlier one.
    for v in 0..d.nodes.len() {
        let (fill, color, label) = (d.nodes[v].fill.is_some(), d.nodes[v].color.is_some(), d.nodes[v].label.is_some());
        for name in std::mem::take(&mut d.nodes[v].classes) {
            let Some(props) = d.classes.get(&name).cloned() else {
                d.dropped.insert(format!("undefined class `{name}`"));
                continue;
            };
            for (p, val) in props {
                match p.as_str() {
                    "style.fill" if fill => {}
                    "style.stroke" if color => {}
                    "label" if label => {}
                    "style.fill" | "style.stroke" | "shape" | "label" => d.prop(Some(v), &[], &p, &val),
                    _ => {
                        d.dropped.insert(format!("class property `{p}`"));
                    }
                }
            }
        }
    }

    let labels = d.nodes.iter_mut().map(|n| &mut n.label).chain(d.edges.iter_mut().map(|e| &mut e.label));
    for l in labels.flatten() {
        *l = d2_subst(l, &d.vars, &mut d.dropped);
    }

    let mut doc = Doc { dropped: d.dropped, ..Doc::default() };
    doc.down = Some(match d.direction.as_deref() {
        Some("right") => false,
        Some("left") => {
            doc.drop("direction left (drawn left to right)");
            false
        }
        Some("up") => {
            doc.drop("direction up (drawn top to bottom)");
            true
        }
        _ => true,
    });
    let mut group_of = vec![None; d.nodes.len()];
    for (v, n) in d.nodes.iter().enumerate() {
        if n.container {
            let last = d.paths[v].rsplit('.').next().unwrap_or("").to_string();
            group_of[v] = Some(doc.groups.len());
            let parent = n.parent.map(|p| group_of[p].expect("parents come first"));
            doc.groups.push(IGroup { title: Some(n.label.clone().unwrap_or(last)), name: d.paths[v].clone(), parent });
        }
    }
    let mut at = vec![None; d.nodes.len()];
    for (v, n) in d.nodes.iter().enumerate().filter(|(_, n)| !n.container) {
        let path = &d.paths[v];
        let w = doc.node(path, n.parent.and_then(|p| group_of[p]));
        let last = path.rsplit('.').next().unwrap_or(path);
        doc.nodes[w].label = n.label.clone().or_else(|| (last != path).then(|| last.to_string()));
        (doc.nodes[w].circle, doc.nodes[w].color, doc.nodes[w].fill) = (n.circle, n.color.clone(), n.fill.clone());
        at[v] = Some(w);
    }
    // A container stands for its first member, the first object written inside it.
    let mut first = at.clone();
    for v in (0..d.nodes.len()).rev() {
        if let (Some(p), Some(_)) = (d.nodes[v].parent, first[v]) {
            first[p] = first[v];
        }
    }
    for e in d.edges {
        if d.nodes[e.a].container || d.nodes[e.b].container {
            doc.drop("connections to containers (drawn to their first member)");
        }
        match (first[e.a], first[e.b]) {
            (Some(a), Some(b)) if a != b => doc.edges.push(IEdge { a, b, ..e }),
            _ => doc.drop("connections between a container and its first member"),
        }
    }
    Ok(doc)
}

/// Mind map node shapes as (open, close, circle); a bare line is a plain node.
const MINDMAP_SHAPES: [(&str, &str, bool); 6] = [("((", "))", true), ("))", "((", false), (")", "(", false), ("{{", "}}", false), ("[", "]", false), ("(", ")", false)];

/// from_mindmap reads a Mermaid `mindmap`, whose hierarchy is its indentation, as a radial map
/// rooted at its first node. Every line is its own node, even when two share a text.
fn from_mindmap(text: &str) -> Result<Doc, String> {
    let mut doc = Doc::default();
    let mut header = false;
    // Indent and node of each open ancestor.
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.split("%%").next().unwrap_or("").trim_end();
        let body = line.trim_start();
        if body.is_empty() {
            continue;
        }
        if !header {
            if body != "mindmap" {
                return Err(format!("line {}: a mind map starts with `mindmap`", i + 1));
            }
            header = true;
            continue;
        }
        if body.starts_with("::icon(") {
            doc.drop("icons");
            continue;
        }
        let mut body = body;
        if let Some(j) = body.find(":::") {
            doc.drop("`:::` classes");
            body = body[..j].trim_end();
        }
        // `id[text]` and the other shapes; otherwise the whole line is the text.
        let shaped = MINDMAP_SHAPES.iter().find_map(|&(open, close, circle)| {
            let j = body.find(open)?;
            let id = &body[..j];
            let inner = body[j + open.len()..].strip_suffix(close)?;
            (!id.contains(char::is_whitespace)).then(|| (id.to_string(), inner.trim(), circle))
        });
        let (id, label, circle) = shaped.unwrap_or((String::new(), body, false));
        let label = strip_html(label.trim_matches('"').trim_matches('`'), &mut doc.dropped);
        let key = if id.is_empty() { label.clone() } else { id };
        let key = if doc.index.contains_key(&key) { format!("{key}_{}", i + 1) } else { key };
        let v = doc.node(&key, None);
        doc.nodes[v].label = Some(label).filter(|l| *l != key);
        doc.nodes[v].circle = circle;
        let indent = line.len() - line.trim_start().len();
        while stack.last().is_some_and(|&(d, _)| d >= indent) {
            stack.pop();
        }
        match stack.last() {
            Some(&(_, p)) => doc.edges.push(IEdge { a: p, b: v, label: None, directed: true, dashed: false, color: None }),
            None if doc.root.is_some() => return Err(format!("line {}: a mind map has one root, and `{}` is a second", i + 1, raw.trim())),
            None => doc.root = Some(v),
        }
        stack.push((indent, v));
    }
    if doc.root.is_none() {
        return Err("empty mind map".into());
    }
    Ok(doc)
}

/// import translates DOT or Mermaid source to fitd, chosen by the file extension or, failing
/// that, by the first word. Returns the fitd text and what was dropped.
pub fn import(path: &str, text: &str) -> Result<(String, Vec<String>), String> {
    let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let first = text.lines().map(|l| l.trim()).find(|l| !l.is_empty() && !l.starts_with("%%") && !l.starts_with("//") && !l.starts_with('#')).unwrap_or("");
    let mermaid = match ext.as_str() {
        "mmd" | "mermaid" => true,
        "dot" | "gv" => false,
        _ => first.starts_with("flowchart") || first == "mindmap" || (first.starts_with("graph") && !first.contains('{')),
    };
    let mindmap = mermaid && first == "mindmap";
    let doc = if ext == "d2" {
        from_d2(text)
    } else if mindmap {
        from_mindmap(text)
    } else if mermaid {
        from_mermaid(text)
    } else {
        from_dot(text)
    }
    .map_err(|e| format!("{path}: {e}"))?;
    Ok(doc.write())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_carries_nodes_edges_and_clusters() {
        let dot = r##"digraph G {
            rankdir=LR; // comment
            node [shape=box];
            subgraph cluster_front { label="Front end"; lex; "parse" [label="Parser\nwith recovery"]; }
            lex -> parse -> sema [label="ast"];
            sema -> out [style=dashed, color="#C62828", penwidth=2];
            "2nd" -> "node";
        }"##;
        let (out, dropped) = import("x.dot", dot).unwrap();
        assert!(out.contains("layout layered dir=right\n"), "{out}");
        assert!(out.contains("node parse \"Parser\\nwith recovery\"\n"), "{out}");
        assert!(out.contains("lex -> parse \"ast\"\nparse -> sema \"ast\"\n"), "{out}");
        assert!(out.contains("sema ..> out color=#C62828\n"), "{out}");
        assert!(out.contains("node n_2nd \"2nd\"\n") && out.contains("node n_node \"node\"\n"), "{out}");
        assert!(out.contains("group \"Front end\" lex, parse\n"), "{out}");
        assert_eq!(dropped, ["edge attribute `penwidth=2`"]);
    }

    #[test]
    fn d2_carries_objects_connections_and_containers() {
        let d2 = "direction: right\nx: Start {shape: circle}\n'q a'.b -> c: go {style.stroke-dash: 3}\nc <- d\nk: {\n  m -> _.c\n  m.style.stroke: \"#c62828\"\n  m.label: |md\n    ## Main\n  |\n}\nk -> x\nclasses: { hot: { style.fill: \"#ffcdd2\"; shape: circle; icon: x }; cool.style: { stroke: \"#1565c0\" } }\nd.class: [cool; hot]\nc.class: ghost\n(d -> c)[0].class: hot\n";
        let (out, dropped) = import("x.d2", d2).unwrap();
        assert!(out.contains("layout layered dir=right\n"), "{out}");
        assert!(out.contains("node x \"Start\" shape=circle\n") && out.contains("node q_a_b \"b\"\n") && out.contains("node k_m \"Main\" color=#C62828\n"), "{out}");
        assert!(out.contains("q_a_b ..> c \"go\"\nd -> c\nk_m -> c\n"), "{out}");
        assert!(out.contains("group \"q a\" q_a_b\ngroup \"k\" k_m\n"), "{out}");
        assert!(out.contains("node d color=#1565C0 fill=#FFCDD2 shape=circle\n"), "{out}");
        assert!(out.contains("k_m -> x\n"), "{out}");
        assert_eq!(dropped, ["`(d`", "class property `icon`", "connections to containers (drawn to their first member)", "undefined class `ghost`"]);
    }

    #[test]
    fn d2_substitutes_vars() {
        let d2 = "vars: {\n  svc: API\n  team: { lead: Ana }\n  d2-config: { theme-id: 4 }\n}\na: ${svc} gateway\nb: \"Owner: ${team.lead}\"\na -> b: ${svc} call ${nope}\nk: { m; n }\nk -> k.m\n";
        let (out, dropped) = import("x.d2", d2).unwrap();
        assert!(out.contains("node a \"API gateway\"\n") && out.contains("node b \"Owner: Ana\"\n"), "{out}");
        assert!(out.contains("a -> b \"API call ${nope}\"\n"), "{out}");
        assert_eq!(dropped, ["`d2-config`", "connections between a container and its first member", "connections to containers (drawn to their first member)", "undefined variable `nope`"]);
    }

    #[test]
    fn clusters_nest() {
        let dot = "digraph { subgraph cluster_o { label=O; a; d; subgraph cluster_i { label=I; b; a; } } subgraph cluster_s { subgraph cluster_t { c; } } }";
        let (out, dropped) = import("x.dot", dot).unwrap();
        assert!(out.contains("group \"O\" a, d, b\ngroup \"I\" a, b\ngroup c\n"), "{out}");
        assert_eq!(dropped, ["cluster `cluster_t`, which holds the same nodes as the one around it"]);
        let d2 = "a: A { b: B { c }; d }\n";
        let (out, _) = import("x.d2", d2).unwrap();
        assert!(out.contains("group \"A\" a_b_c, a_d\ngroup \"B\" a_b_c\n"), "{out}");
        let mmd = "flowchart TD\n  subgraph O\n    A\n    subgraph I\n      B\n    end\n  end\n";
        let (out, _) = import("x.mmd", mmd).unwrap();
        assert!(out.contains("group \"O\" A, B\ngroup \"I\" B\n"), "{out}");
    }

    #[test]
    fn mindmap_indentation_becomes_a_radial_tree() {
        let mmd = "mindmap\n  root((Mind maps))\n    Origins\n      Long history\n      ::icon(fa fa-book)\n      Popular\n    Research\n      On effectiveness<br/>and features\n      History\n    Tools\n      History\n      a[History]\n";
        let (out, dropped) = import("m.mmd", mmd).unwrap();
        assert!(out.contains("layout radial root=n_root\n"), "{out}");
        assert!(out.contains("node n_root \"Mind maps\" shape=circle\n"), "{out}");
        assert!(out.contains("node Long_history \"Long history\"\n") && out.contains("node History\n") && out.contains("node History_11 \"History\"\n"), "{out}");
        assert!(out.contains("node On_effectiveness_and_features \"On effectiveness\\nand features\"\n"), "{out}");
        assert!(out.contains("n_root -> Origins\nOrigins -> Long_history\nOrigins -> Popular\nn_root -> Research\n"), "{out}");
        assert!(out.contains("Tools -> a\n") && out.contains("node a \"History\"\n"), "{out}");
        assert_eq!(dropped, ["icons"]);
        assert!(import("m.mmd", "mindmap\n  a\n  b\n").unwrap_err().contains("second"));
    }

    #[test]
    fn mermaid_carries_shapes_labels_and_subgraphs() {
        let mmd = "flowchart LR\n  %% comment\n  A[Start] --> B{Is it?}\n  B -->|Yes| C((Done)) & D\n  B -- No --> E[\"Retry <br> later\"]\n  E -.-> A\n  subgraph S1 [Checks]\n    B\n  end\n  classDef hot fill:#f00\n";
        let (out, dropped) = import("x.mmd", mmd).unwrap();
        assert!(out.contains("layout layered dir=right\n"), "{out}");
        assert!(out.contains("node A \"Start\"\n") && out.contains("node C \"Done\" shape=circle\n"), "{out}");
        assert!(out.contains("B -> C \"Yes\"\nB -> D \"Yes\"\nB -> E \"No\"\nE ..> A\n"), "{out}");
        assert!(out.contains("node E \"Retry \\n later\"\n"), "{out}");
        assert!(out.contains("group \"Checks\" B\n"), "{out}");
        assert_eq!(dropped, ["`classDef` statements"]);
    }
}
