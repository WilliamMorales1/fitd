//! `fitd split`: turns one map into an overview page and one page per top-level branch. Each
//! page is ordinary `.fitd` that includes a shared file of settings and styles, so any page
//! builds alone and a style edited once changes every page.

use crate::ast::{Name, Rel, StmtKind};
use crate::diag::Level;
use std::collections::BTreeSet;

/// Page is where a statement goes: the overview, or the page of a top-level branch, by its
/// index among the root's children.
type Page = usize;
const OVERVIEW: Page = usize::MAX;

/// Split is the written pages: the shared file, then the overview, then one per branch, as
/// (file name, text); and statements that span pages and so were left out.
pub struct Split {
    pub files: Vec<(String, String)>,
    pub dropped: Vec<String>,
}

/// split reads the map at `path` (whose text is `text`) and lays it out over pages named after
/// `stem`. Fails when the map has errors or its root has fewer than two branches.
pub fn split(path: &str, text: &str, stem: &str) -> Result<Split, String> {
    let (src, mut diags) = crate::include::expand(path, text);
    let (stmts, parse_diags) = crate::parse::parse(&src.text);
    diags.extend(parse_diags);
    let (g, sema_diags) = crate::sema::analyze(&stmts);
    diags.extend(sema_diags);
    if diags.iter().any(|d| d.level == Level::Error) {
        return Err(format!("{path} has errors; run `fitd {path}` to see them"));
    }
    let t = crate::tree::build(&g);
    let heads = &t.children[t.root];
    if heads.len() < 2 {
        return Err(format!("{path}: the root `{}` has fewer than two branches, so there is nothing to split", g.nodes[t.root].id));
    }
    let index = |n: &Name| g.nodes.iter().position(|v| v.id == n.text);
    // The pages a node appears on: a branch head is both a stub on the overview and its own
    // page's root.
    let pages_of = |v: usize| -> BTreeSet<Page> {
        match t.branch(v) {
            None => BTreeSet::from([OVERVIEW]),
            Some(h) => {
                let k = heads.iter().position(|&x| x == h).unwrap();
                if h == v { BTreeSet::from([OVERVIEW, k]) } else { BTreeSet::from([k]) }
            }
        }
    };
    let file = |p: Page| if p == OVERVIEW { format!("{stem}.fitd") } else { format!("{stem}-{}.fitd", g.nodes[heads[p]].id) };
    let snippet = |lo: usize, hi: usize| src.text[lo..hi].trim_end().to_string();

    let mut shared = String::new();
    let mut title = None;
    let mut body: Vec<Vec<String>> = vec![Vec::new(); heads.len() + 1];
    let slot = |p: Page| if p == OVERVIEW { heads.len() } else { p };
    let mut dropped = Vec::new();
    // Pages the last `node` went to, for the `|` body lines under it; the overview stub
    // drops them, since its page holds the full node.
    let mut last: BTreeSet<Page> = BTreeSet::new();
    for st in &stmts {
        let own = snippet(st.span.lo, st.span.hi);
        let names: Vec<&Name> = match &st.kind {
            StmtKind::Title(_) => {
                title = Some(own);
                continue;
            }
            StmtKind::Root(_) | StmtKind::Include(_) => continue,
            StmtKind::Layout { kind, attrs } => {
                // The root is set per page, and the map's root is missing from branch pages.
                shared += &format!("layout {}", kind.text);
                for a in attrs.iter().filter(|a| a.key.text != "root") {
                    shared += &format!(" {}", snippet(a.key.span.lo, a.value.span().hi));
                }
                shared.push('\n');
                continue;
            }
            StmtKind::Page { .. } | StmtKind::Font { .. } | StmtKind::Theme { .. } | StmtKind::Style { .. } => {
                shared += &own;
                shared.push('\n');
                continue;
            }
            StmtKind::Body(_) => {
                for &p in last.iter().filter(|&&p| p != OVERVIEW || last.len() == 1) {
                    body[slot(p)].push(own.clone());
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
        let mut on: Option<BTreeSet<Page>> = None;
        for n in names {
            let Some(v) = index(n) else { continue };
            let p = pages_of(v);
            on = Some(on.map_or(p.clone(), |o| o.intersection(&p).copied().collect()));
        }
        let on = on.unwrap_or_default();
        if matches!(st.kind, StmtKind::Node { .. }) {
            for &p in &on {
                body[slot(p)].push(own.clone());
            }
            if on.len() == 2 {
                let page = file(*on.iter().find(|&&p| p != OVERVIEW).unwrap());
                body[slot(OVERVIEW)].push(format!("| see page {}", page.trim_end_matches(".fitd")));
            }
            last = on;
            continue;
        }
        // A statement on a head alone belongs with its page, where the head is the root.
        match on.iter().find(|&&p| p != OVERVIEW).or(on.first()) {
            Some(&p) => body[slot(p)].push(own),
            None => dropped.push(own),
        }
    }

    let from = std::path::Path::new(path).file_name().map_or(path.to_string(), |f| f.to_string_lossy().into_owned());
    let mut files = vec![(format!("{stem}-shared.fitd"), format!("# Settings and styles shared by every page of {from}, written by `fitd split`.\n{shared}"))];
    let page = |p: Page, title: String, root: usize| {
        let mut s = format!("# Written by `fitd split` from {from}.\ninclude \"{stem}-shared.fitd\"\n{title}\nroot {}\n\n", g.nodes[root].id);
        for line in &body[slot(p)] {
            s += line;
            s.push('\n');
        }
        (file(p), s)
    };
    let quoted = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"));
    files.push(page(OVERVIEW, title.unwrap_or_else(|| format!("title {}", quoted(&g.nodes[t.root].title))), t.root));
    for (k, &h) in heads.iter().enumerate() {
        files.push(page(k, format!("title {}", quoted(&g.nodes[h].title)), h));
    }
    Ok(Split { files, dropped })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branches_get_pages_and_stubs_name_them() {
        let text = "title \"Map\"\nlayout radial root=r\nstyle s color=#1565C0\nnode r \"Root\"\nnode a \"A\" .s\n| about a\nnode a1\nnode b\nnode b1\nr -> a, b\na -> a1\nb -> b1\na1 ..> b1\nrequire a1 left-of a\nrequire a1 near b1\n";
        let out = split("m.fitd", text, "m").unwrap();
        let names: Vec<&str> = out.files.iter().map(|f| f.0.as_str()).collect();
        assert_eq!(names, ["m-shared.fitd", "m.fitd", "m-a.fitd", "m-b.fitd"]);
        let get = |n: &str| &out.files.iter().find(|f| f.0 == n).unwrap().1;
        assert!(get("m-shared.fitd").contains("layout radial\nstyle s color=#1565C0\n"), "{}", get("m-shared.fitd"));
        let overview = get("m.fitd");
        assert!(overview.contains("title \"Map\"\nroot r\n") && overview.contains("node a \"A\" .s\n| see page m-a\n") && overview.contains("r -> a, b"), "{overview}");
        assert!(!overview.contains("about a") && !overview.contains("a1"), "{overview}");
        let a = get("m-a.fitd");
        assert!(a.contains("include \"m-shared.fitd\"\ntitle \"A\"\nroot a\n") && a.contains("| about a") && a.contains("a -> a1") && a.contains("require a1 left-of a"), "{a}");
        assert_eq!(out.dropped, ["a1 ..> b1", "require a1 near b1"]);
    }
}
