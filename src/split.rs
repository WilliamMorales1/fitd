use crate::ast::{Attr, Name, Rel, StmtKind};
use crate::diag::Level;
use std::collections::{BTreeMap, BTreeSet, HashMap};

type Page = usize;
const OVERVIEW: Page = usize::MAX;

pub struct Split {
    pub files: Vec<(String, String)>,
    pub dropped: Vec<String>,
}

pub fn split(path: &str, text: &str, stem: &str) -> Result<Split, String> {
    let crate::macros::Loaded {
        parse, mut diags, ..
    } = crate::macros::load(path, text, crate::macros::DEFAULT_BUDGET);
    let (stmts, parse_diags) = crate::parse::parse(&parse);
    diags.extend(parse_diags);
    let (g, sema_diags) = crate::sema::analyze(&stmts);
    diags.extend(sema_diags);
    if diags.iter().any(|d| d.level == Level::Error) {
        return Err(format!("{path} has errors; run `fitd {path}` to see them"));
    }
    let t = crate::tree::build(&g);
    let heads = &t.children[t.root];
    if heads.len() < 2 {
        return Err(format!(
            "{path}: the root `{}` has fewer than two branches, so there is nothing to split",
            g.nodes[t.root].id
        ));
    }
    let index = |n: &Name| g.nodes.iter().position(|v| v.id == n.text);
    let pages_of = |v: usize| -> BTreeSet<Page> {
        match t.branch(v) {
            None => BTreeSet::from([OVERVIEW]),
            Some(h) => {
                let k = heads.iter().position(|&x| x == h).unwrap();
                if h == v {
                    BTreeSet::from([OVERVIEW, k])
                } else {
                    BTreeSet::from([k])
                }
            }
        }
    };
    let file = |p: Page| {
        if p == OVERVIEW {
            format!("{stem}.fitd")
        } else {
            format!("{stem}-{}.fitd", g.nodes[heads[p]].id)
        }
    };
    let snippet = |lo: usize, hi: usize| parse[lo..hi].trim_end().to_string();

    let mut shared = String::new();
    let mut title = None;
    // Node lines with their rows come first on a page, so a style block opens once for all of them.
    let mut body: Vec<Vec<(Looks, String)>> = vec![Vec::new(); heads.len() + 1];
    let mut rest: Vec<Vec<(Looks, String)>> = vec![Vec::new(); heads.len() + 1];
    let slot = |p: Page| if p == OVERVIEW { heads.len() } else { p };
    let mut dropped = Vec::new();
    let mut framed: BTreeMap<Page, Vec<BTreeSet<String>>> = BTreeMap::new();
    let mut stubs: Vec<Vec<usize>> = vec![Vec::new(); heads.len() + 1];
    let mut group_stubs: Vec<Vec<&str>> = vec![Vec::new(); heads.len() + 1];
    let home = |v: usize| {
        *pages_of(v)
            .iter()
            .find(|&&p| p != OVERVIEW)
            .unwrap_or(&OVERVIEW)
    };
    let groups: HashMap<&str, (&Option<String>, Vec<usize>)> = stmts
        .iter()
        .filter_map(|st| match &st.kind {
            StmtKind::Group {
                id: Some(id),
                title,
                items,
                ..
            } => Some((
                id.text.as_str(),
                (title, items.iter().filter_map(index).collect()),
            )),
            _ => None,
        })
        .collect();
    // A group ID stands for the pages its members are written on.
    let pages_named = |n: &Name| -> Option<BTreeSet<Page>> {
        match index(n) {
            Some(v) => Some(pages_of(v)),
            None => groups
                .get(n.text.as_str())
                .map(|(_, m)| m.iter().map(|&v| home(v)).collect()),
        }
    };
    let meet = |names: &[&Name]| -> BTreeSet<Page> {
        let mut on: Option<BTreeSet<Page>> = None;
        for n in names {
            let Some(p) = pages_named(n) else { continue };
            on = Some(on.map_or(p.clone(), |o| o.intersection(&p).copied().collect()));
        }
        on.unwrap_or_default()
    };
    let parts = |items: &[Name]| -> Vec<(Page, Vec<Name>)> {
        let mut out: Vec<(Page, Vec<Name>)> = Vec::new();
        for n in items {
            let Some(v) = index(n) else { continue };
            match out.iter_mut().find(|o| o.0 == home(v)) {
                Some(o) => o.1.push(n.clone()),
                None => out.push((home(v), vec![n.clone()])),
            }
        }
        out
    };
    let decl: Vec<(Looks, String)> = {
        let mut d = vec![(None, String::new()); g.nodes.len()];
        for st in &stmts {
            if let StmtKind::Node { id, classes, .. } = &st.kind
                && let Some(v) = index(id)
            {
                d[v] = (Some(names(classes)), snippet(st.span.lo, st.span.hi));
            }
        }
        d
    };
    let quoted = |s: &str| {
        format!(
            "\"{}\"",
            s.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
        )
    };
    let mut last: BTreeSet<Page> = BTreeSet::new();
    // A nested child's edge spans only the child, so its operator is rebuilt from the edge kind.
    let nested = |k: &StmtKind| match k {
        StmtKind::Edge {
            from,
            to,
            link,
            directed,
            ..
        } if to.first().is_some_and(|t| t.span == from.span) => {
            Some(["--", "->", "..", "..>"][2 * *link as usize + *directed as usize])
        }
        _ => None,
    };
    for st in &stmts {
        let own = match (&st.kind, nested(&st.kind)) {
            (StmtKind::Edge { from, to, .. }, Some(op)) => {
                format!("{} {op} {}", from.text, to[0].text)
            }
            _ => snippet(st.span.lo, st.span.hi),
        };
        let looks: Looks = match &st.kind {
            StmtKind::Node { classes, .. }
            | StmtKind::Edge { classes, .. }
            | StmtKind::Group { classes, .. }
            | StmtKind::Key { classes, .. } => Some(names(classes)),
            _ => None,
        };
        let names: Vec<&Name> = match &st.kind {
            StmtKind::Title(_) => {
                title = Some(own);
                continue;
            }
            StmtKind::Root(_) | StmtKind::Include(_) | StmtKind::Open(_) => {
                continue;
            }
            StmtKind::Layout { kind, attrs } => {
                shared += &format!("layout {}", kind.text);
                for a in attrs.iter().filter(|a| a.key.text != "root") {
                    shared += &format!(" {}", snippet(a.key.span.lo, a.value.span().hi));
                }
                shared.push('\n');
                continue;
            }
            StmtKind::Page { .. }
            | StmtKind::Font { .. }
            | StmtKind::Theme { .. }
            | StmtKind::Style { .. }
            | StmtKind::Key { .. } => {
                shared += &blocks(&[(looks, own)]);
                continue;
            }
            StmtKind::Body(_) | StmtKind::Cells(_) => {
                for &p in last.iter().filter(|&&p| p != OVERVIEW || last.len() == 1) {
                    body[slot(p)].push((None, own.clone()));
                }
                continue;
            }
            StmtKind::Node { id, .. } => vec![id],
            StmtKind::Edge { from, to, .. } => std::iter::once(from).chain(to).collect(),
            StmtKind::Group { items, .. } => items.iter().collect(),
            StmtKind::Constraint { rel, .. } => match rel {
                Rel::Chain { nodes, .. } => nodes.iter().collect(),
                Rel::Align { items, .. } => items.iter().collect(),
                Rel::At { a, .. } => vec![a],
            },
        };
        let on = meet(&names);
        if matches!(st.kind, StmtKind::Node { .. }) {
            for &p in &on {
                body[slot(p)].push((looks.clone(), own.clone()));
            }
            if on.len() == 2 {
                let page = file(*on.iter().find(|&&p| p != OVERVIEW).unwrap());
                body[slot(OVERVIEW)].push((
                    None,
                    format!("| see page {}", page.trim_end_matches(".fitd")),
                ));
            }
            last = on;
            continue;
        }
        if let Some(&p) = on.iter().find(|&&p| p != OVERVIEW).or(on.first()) {
            if let StmtKind::Group { items, .. } = &st.kind {
                let set: BTreeSet<String> = items.iter().map(|n| n.text.clone()).collect();
                let sets = framed.entry(p).or_default();
                if sets.contains(&set) {
                    dropped.push(format!(
                        "{own} (frames the same nodes as another group on its page)"
                    ));
                    continue;
                }
                sets.push(set);
            }
            rest[slot(p)].push((looks, own));
            continue;
        }
        let attrs_of = |attrs: &[Attr]| {
            attrs
                .iter()
                .map(|a| format!(" {}", snippet(a.key.span.lo, a.value.span().hi)))
                .collect::<String>()
        };
        match &st.kind {
            StmtKind::Edge { from, to, .. } => {
                let (op, tail) = match nested(&st.kind) {
                    Some(op) => (op.to_string(), String::new()),
                    None => (
                        parse[from.span.hi..to[0].span.lo].trim().to_string(),
                        snippet(to.last().unwrap().span.hi, st.span.hi),
                    ),
                };
                for t in to {
                    let line = format!("{} {op} {}{tail}", from.text, t.text);
                    let on = meet(&[from, t]);
                    if let Some(&p) = on.iter().find(|&&p| p != OVERVIEW).or(on.first()) {
                        rest[slot(p)].push((looks.clone(), line));
                        continue;
                    }
                    if pages_named(from).is_none() || pages_named(t).is_none() {
                        continue;
                    }
                    let end_home = |n: &Name| match index(n) {
                        Some(v) => home(v),
                        None => groups[n.text.as_str()]
                            .1
                            .first()
                            .map_or(OVERVIEW, |&v| home(v)),
                    };
                    for (here, there) in [(from, t), (t, from)] {
                        let p = end_home(here);
                        rest[slot(p)].push((looks.clone(), line.clone()));
                        let members = match index(there) {
                            Some(v) => vec![v],
                            None => {
                                if !group_stubs[slot(p)].contains(&there.text.as_str()) {
                                    group_stubs[slot(p)].push(there.text.as_str());
                                }
                                groups[there.text.as_str()].1.clone()
                            }
                        };
                        for v in members.into_iter().filter(|&v| home(v) != p) {
                            if !stubs[slot(p)].contains(&v) {
                                stubs[slot(p)].push(v);
                            }
                        }
                    }
                }
            }
            StmtKind::Group {
                id,
                title,
                items,
                attrs,
                ..
            } => {
                let title = match (id, title) {
                    (Some(n), Some(t)) => format!(" {} {}", n.text, quoted(t)),
                    (Some(n), None) => format!(" {} \"\"", n.text),
                    (None, Some(t)) => format!(" {}", quoted(t)),
                    (None, None) => String::new(),
                };
                for (p, part) in parts(items) {
                    let ids: Vec<&str> = part.iter().map(|n| n.text.as_str()).collect();
                    let line = format!("group{title} {}{}", ids.join(", "), attrs_of(attrs));
                    let set: BTreeSet<String> = ids.iter().map(|i| i.to_string()).collect();
                    let sets = framed.entry(p).or_default();
                    if sets.contains(&set) {
                        dropped.push(format!(
                            "{line} (frames the same nodes as another group on its page)"
                        ));
                        continue;
                    }
                    sets.push(set);
                    rest[slot(p)].push((looks.clone(), line));
                }
            }
            StmtKind::Constraint { hard, rel, attrs } => {
                let kw = if *hard { "require" } else { "prefer" };
                let attrs = attrs_of(attrs);
                match rel {
                    Rel::Chain { nodes, ops } => {
                        let mut run: Option<(Page, String)> = None;
                        for (k, op) in ops.iter().enumerate() {
                            let pair =
                                format!("{} {} {}", nodes[k].text, op.text, nodes[k + 1].text);
                            let on = meet(&[&nodes[k], &nodes[k + 1]]);
                            match on.iter().find(|&&p| p != OVERVIEW).or(on.first()) {
                                Some(&p) if run.as_ref().is_some_and(|r| r.0 == p) => {
                                    run.as_mut().unwrap().1 +=
                                        &format!(" {} {}", op.text, nodes[k + 1].text)
                                }
                                Some(&p) => {
                                    if let Some((q, r)) = run.replace((p, pair)) {
                                        rest[slot(q)].push((None, format!("{kw} {r}{attrs}")));
                                    }
                                }
                                None => {
                                    if let Some((q, r)) = run.take() {
                                        rest[slot(q)].push((None, format!("{kw} {r}{attrs}")));
                                    }
                                    dropped.push(format!("{kw} {pair}{attrs}"));
                                }
                            }
                        }
                        if let Some((q, r)) = run {
                            rest[slot(q)].push((None, format!("{kw} {r}{attrs}")));
                        }
                    }
                    Rel::Align { side, items } => {
                        let mut alone = Vec::new();
                        for (p, part) in parts(items) {
                            let ids: Vec<&str> = part.iter().map(|n| n.text.as_str()).collect();
                            match part.len() {
                                1 => alone.push(ids[0].to_string()),
                                _ => rest[slot(p)].push((
                                    None,
                                    format!("{kw} align {}({}){attrs}", side.text, ids.join(", ")),
                                )),
                            }
                        }
                        if !alone.is_empty() {
                            dropped.push(format!("{own} (for {})", alone.join(", ")));
                        }
                    }
                    Rel::At { .. } => dropped.push(own),
                }
            }
            _ => dropped.push(own),
        }
    }

    let from = std::path::Path::new(path)
        .file_name()
        .map_or(path.to_string(), |f| f.to_string_lossy().into_owned());
    let mut files = vec![(
        format!("{stem}-shared.fitd"),
        format!(
            "# Settings and styles shared by every page of {from}, written by `fitd split`.\n{shared}"
        ),
    )];
    let page = |p: Page, title: String, root: usize| {
        let mut s = format!(
            "# Written by `fitd split` from {from}.\ninclude \"{stem}-shared.fitd\"\n{title}\nroot {}\n\n",
            g.nodes[root].id
        );
        s += &blocks(&[&body[slot(p)][..], &rest[slot(p)][..]].concat());
        if !stubs[slot(p)].is_empty() {
            s += "\n# Nodes on other pages that edges here reach.\n";
        }
        let mut stub = Vec::new();
        for &v in &stubs[slot(p)] {
            stub.push(decl[v].clone());
            stub.push((
                None,
                format!("| see page {}", file(home(v)).trim_end_matches(".fitd")),
            ));
        }
        for &id in &group_stubs[slot(p)] {
            let (title, members) = &groups[id];
            let ids: Vec<&str> = members.iter().map(|&v| g.nodes[v].id.as_str()).collect();
            let title = title
                .as_deref()
                .map_or(String::new(), |t| format!(" {}", quoted(t)));
            stub.push((
                Some(Vec::new()),
                format!("group {id}{title} {}", ids.join(", ")),
            ));
        }
        s += &blocks(&stub);
        (file(p), s)
    };
    files.push(page(
        OVERVIEW,
        title.unwrap_or_else(|| format!("title {}", quoted(&g.nodes[t.root].title))),
        t.root,
    ));
    for (k, &h) in heads.iter().enumerate() {
        files.push(page(k, format!("title {}", quoted(&g.nodes[h].title)), h));
    }
    Ok(Split { files, dropped })
}

/// Looks holds the styles of the blocks around a line, or None for a line that keeps any open block.
type Looks = Option<Vec<String>>;

fn names(classes: &[Name]) -> Vec<String> {
    classes.iter().map(|c| c.text.clone()).collect()
}

/// blocks writes lines with the style blocks they need, opening and closing only where the styles change.
fn blocks(lines: &[(Looks, String)]) -> String {
    let (mut s, mut open) = (String::new(), Vec::<String>::new());
    for (looks, line) in lines {
        if let Some(want) = looks {
            let keep = open.iter().zip(want).take_while(|(a, b)| a == b).count();
            open.truncate(keep);
            for c in &want[keep..] {
                s += &format!("{}{c}\n", "  ".repeat(open.len()));
                open.push(c.clone());
            }
        }
        s += &format!("{}{}\n", "  ".repeat(open.len()), line.trim_start());
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branches_get_pages_and_stubs_name_them() {
        let text = "title \"Map\"\nlayout radial root=r\nstyle s color=#1565C0\nnode r \"Root\"\ns\n  node a \"A\"\n  | about a\nnode a1\nnode b\nnode b1\nr -> a, b\na -> a1\nb -> b1\na1 ..> b1\nrequire a1 left-of a\nrequire a1 near b1\n";
        let out = split("m.fitd", text, "m").unwrap();
        let names: Vec<&str> = out.files.iter().map(|f| f.0.as_str()).collect();
        assert_eq!(names, ["m-shared.fitd", "m.fitd", "m-a.fitd", "m-b.fitd"]);
        let get = |n: &str| &out.files.iter().find(|f| f.0 == n).unwrap().1;
        assert!(
            get("m-shared.fitd").contains("layout radial\nstyle s color=#1565C0\n"),
            "{}",
            get("m-shared.fitd")
        );
        let overview = get("m.fitd");
        assert!(
            overview.contains("title \"Map\"\nroot r\n")
                && overview.contains("s\n  node a \"A\"\n  | see page m-a\n")
                && overview.contains("r -> a, b"),
            "{overview}"
        );
        assert!(
            !overview.contains("about a") && !overview.contains("a1"),
            "{overview}"
        );
        let a = get("m-a.fitd");
        assert!(
            a.contains("include \"m-shared.fitd\"\ntitle \"A\"\nroot a\n")
                && a.contains("| about a")
                && a.contains("a -> a1")
                && a.contains("require a1 left-of a"),
            "{a}"
        );
        assert!(
            a.contains("a1 ..> b1\n")
                && a.ends_with(
                    "\n# Nodes on other pages that edges here reach.\nnode b1\n| see page m-b\n"
                ),
            "{a}"
        );
        assert!(get("m-b.fitd").ends_with("a1 ..> b1\n\n# Nodes on other pages that edges here reach.\nnode a1\n| see page m-a\n"), "{}", get("m-b.fitd"));
        assert_eq!(out.dropped, ["require a1 near b1"]);
    }

    #[test]
    fn group_edges_and_style_blocks_survive_a_split() {
        let text = "layout radial root=r\nstyle s color=#0072B2\nr \"Root\"\n  a \"A\"\n    a1\n    a2\n  b \"B\"\n    s\n      b1\n      b2\ngroup G \"Gr\" a1, a2\nG -> b1\n";
        let out = split("m.fitd", text, "m").unwrap();
        let get = |n: &str| &out.files.iter().find(|f| f.0 == n).unwrap().1;
        let (a, b) = (get("m-a.fitd"), get("m-b.fitd"));
        assert!(
            a.contains("G -> b1\n") && a.contains("s\n  b1\n  | see page m-b\n"),
            "{a}"
        );
        assert!(
            b.contains("s\n  b1\n  b2\nb -- b1\n")
                && b.contains("G -> b1\n")
                && b.ends_with("a1\n| see page m-a\na2\n| see page m-a\ngroup G \"Gr\" a1, a2\n"),
            "{b}"
        );
        assert!(out.dropped.is_empty(), "{:?}", out.dropped);
    }

    #[test]
    fn groups_and_chains_keep_each_page_part() {
        let text = "node r\nnode a\nnode a1\nnode a2\nnode b\nnode b1\nnode b2\nr -> a, b\na -> a1, a2\nb -> b1, b2\na ..> b1, b2 \"x\"\ngroup \"G\" a1, a2, b1 color=#6D4C41\nrequire a1 left-of a2 left-of b1 left-of b2 gap=1em\nprefer align top(a1, a2, b1)\n";
        let out = split("m.fitd", text, "m").unwrap();
        let get = |n: &str| &out.files.iter().find(|f| f.0 == n).unwrap().1;
        let (a, b) = (get("m-a.fitd"), get("m-b.fitd"));
        assert!(
            a.contains("a ..> b1 \"x\"\na ..> b2 \"x\"\n")
                && a.contains("node b1\n| see page m-b\nnode b2\n| see page m-b\n"),
            "{a}"
        );
        assert!(
            b.contains("a ..> b1 \"x\"\na ..> b2 \"x\"\n")
                && b.contains("node a\n| see page m-a\n")
                && !b.contains("node a\n| see page m-a\nnode a\n"),
            "{b}"
        );
        assert!(
            a.contains("group \"G\" a1, a2 color=#6D4C41\n")
                && b.contains("group \"G\" b1 color=#6D4C41\n"),
            "{a}\n{b}"
        );
        assert!(
            a.contains("require a1 left-of a2 gap=1em\n")
                && b.contains("require b1 left-of b2 gap=1em\n")
                && a.contains("prefer align top(a1, a2)\n"),
            "{a}\n{b}"
        );
        assert_eq!(
            out.dropped,
            [
                "require a2 left-of b1 gap=1em",
                "prefer align top(a1, a2, b1) (for b1)"
            ]
        );
        let nested = "node r\nnode a\nnode a1\nnode b\nnode b1\nr -> a, b\na -> a1\nb -> b1\ngroup \"O\" a, a1, b1\ngroup \"I\" a, a1\n";
        let out = split("m.fitd", nested, "m").unwrap();
        let a = &out.files.iter().find(|f| f.0 == "m-a.fitd").unwrap().1;
        assert!(
            a.contains("group \"O\" a, a1\n") && !a.contains("\"I\""),
            "{a}"
        );
        assert_eq!(
            out.dropped,
            ["group \"I\" a, a1 (frames the same nodes as another group on its page)"]
        );
    }
}
