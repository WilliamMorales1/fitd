//! `fitd import`: translates Graphviz DOT and Mermaid flowcharts to fitd. Nodes, labels,
//! edges, and clusters or subgraphs (as `group`) carry over; everything else is dropped and
//! listed, since fitd places nodes itself.

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
}

/// Doc is a diagram read from another language, before it is written as fitd.
#[derive(Default)]
struct Doc {
    /// None for radial; Some(true) for a layered flow down, Some(false) for right.
    down: Option<bool>,
    nodes: Vec<INode>,
    index: HashMap<String, usize>,
    edges: Vec<IEdge>,
    groups: Vec<IGroup>,
    /// Groups with members in clusters nested inside them, which fitd cannot nest.
    nested: BTreeSet<usize>,
    dropped: BTreeSet<String>,
}

impl Doc {
    fn node(&mut self, id: &str, group: Option<usize>) -> usize {
        if let Some(&v) = self.index.get(id) {
            if group.is_some() && self.nodes[v].group.is_some() && self.nodes[v].group != group {
                self.dropped.insert(format!("membership of `{id}` in a second cluster"));
            }
            if self.nodes[v].group.is_none() {
                self.nodes[v].group = group;
            }
            return v;
        }
        self.index.insert(id.to_string(), self.nodes.len());
        self.nodes.push(INode { id: id.to_string(), group, ..INode::default() });
        self.nodes.len() - 1
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
            None => o += "layout radial\n",
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
        for (k, g) in self.groups.iter().enumerate() {
            let members: Vec<&str> = (0..self.nodes.len()).filter(|&v| self.nodes[v].group == Some(k)).map(|v| ids[v].as_str()).collect();
            if self.nested.contains(&k) {
                self.dropped.insert(format!("nesting of clusters (`{}` frames only its own nodes)", g.name));
            }
            if members.is_empty() {
                continue;
            }
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
                if let Some(&outer) = self.clusters.last() {
                    self.doc.nested.insert(outer);
                }
                self.doc.groups.push(IGroup { title: None, name: name.clone() });
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
                if let Some(&outer) = clusters.last() {
                    doc.nested.insert(outer);
                }
                doc.groups.push(IGroup { title: Some(title).filter(|t| !t.is_empty()), name });
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

/// import translates DOT or Mermaid source to fitd, chosen by the file extension or, failing
/// that, by the first word. Returns the fitd text and what was dropped.
pub fn import(path: &str, text: &str) -> Result<(String, Vec<String>), String> {
    let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let first = text.lines().map(|l| l.trim()).find(|l| !l.is_empty() && !l.starts_with("%%") && !l.starts_with("//") && !l.starts_with('#')).unwrap_or("");
    let mermaid = match ext.as_str() {
        "mmd" | "mermaid" => true,
        "dot" | "gv" => false,
        _ => first.starts_with("flowchart") || (first.starts_with("graph") && !first.contains('{')),
    };
    let doc = if mermaid { from_mermaid(text) } else { from_dot(text) }.map_err(|e| format!("{path}: {e}"))?;
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
