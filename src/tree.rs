use crate::diag::Diag;
use crate::graph::{EdgeKind, Graph};
use std::collections::VecDeque;

#[derive(Clone)]
pub struct Link {
    pub a: usize,
    pub b: usize,
    pub label: Option<String>,
}

pub struct Tree {
    pub root: usize,
    pub parent: Vec<Option<usize>>,
    pub children: Vec<Vec<usize>>,
    pub depth: Vec<usize>,
    pub order: Vec<usize>,
    pub links: Vec<Link>,
    pub notes: Vec<Diag>,
}

impl Tree {
    pub fn leaves(&self, v: usize) -> usize {
        if self.children[v].is_empty() {
            1
        } else {
            self.children[v].iter().map(|&c| self.leaves(c)).sum()
        }
    }

    pub fn branch(&self, mut v: usize) -> Option<usize> {
        while let Some(p) = self.parent[v] {
            if p == self.root {
                return Some(v);
            }
            v = p;
        }
        None
    }
}

fn bfs_dist(adj: &[Vec<usize>], s: usize) -> Vec<Option<usize>> {
    let mut d = vec![None; adj.len()];
    d[s] = Some(0);
    let mut q = VecDeque::from([s]);
    while let Some(u) = q.pop_front() {
        for &w in &adj[u] {
            if d[w].is_none() {
                d[w] = Some(d[u].unwrap() + 1);
                q.push_back(w);
            }
        }
    }
    d
}

fn center(adj: &[Vec<usize>]) -> usize {
    let score = |v: usize| {
        let d = bfs_dist(adj, v);
        let reach = d.iter().flatten().count();
        let ecc = d.iter().flatten().max().copied().unwrap_or(0);
        (
            std::cmp::Reverse(reach),
            ecc,
            std::cmp::Reverse(adj[v].len()),
        )
    };
    (0..adj.len()).min_by_key(|&v| score(v)).unwrap_or(0)
}

pub fn build(g: &Graph) -> Tree {
    let n = g.nodes.len();
    let mut adj = vec![Vec::new(); n];
    for e in g.edges.iter().filter(|e| e.kind == EdgeKind::Tree) {
        adj[e.a].push(e.b);
        adj[e.b].push(e.a);
    }
    let root = g.root.unwrap_or_else(|| center(&adj));
    let mut t = Tree {
        root,
        parent: vec![None; n],
        children: vec![Vec::new(); n],
        depth: vec![0; n],
        order: Vec::new(),
        links: Vec::new(),
        notes: Vec::new(),
    };
    if g.root.is_none() {
        t.notes.push(Diag::info(format!(
            "no `root` given; chose graph center `{}`",
            g.nodes[root].id
        )));
    }
    let mut seen = vec![false; n];
    let mut used_link = vec![false; g.edges.len()];
    let mut start = Some((root, None::<usize>));
    while let Some((s, par)) = start.take() {
        seen[s] = true;
        if let Some(p) = par {
            t.parent[s] = Some(p);
            t.children[p].push(s);
            t.depth[s] = t.depth[p] + 1;
        }
        t.order.push(s);
        let mut q = VecDeque::from([s]);
        while let Some(u) = q.pop_front() {
            for &w in &adj[u] {
                if !seen[w] {
                    seen[w] = true;
                    t.parent[w] = Some(u);
                    t.children[u].push(w);
                    t.depth[w] = t.depth[u] + 1;
                    t.order.push(w);
                    q.push_back(w);
                }
            }
        }
        if let Some(v) = (0..n).find(|&v| !seen[v]) {
            let via = g.edges.iter().enumerate().find(|(_, e)| {
                e.kind == EdgeKind::Link && ((seen[e.a] && !seen[e.b]) || (seen[e.b] && !seen[e.a]))
            });
            start = Some(match via {
                Some((i, e)) => {
                    used_link[i] = true;
                    let (inside, out) = if seen[e.a] { (e.a, e.b) } else { (e.b, e.a) };
                    t.notes.push(
                        Diag::info(format!(
                            "`{}` reachable only by a link; attached under `{}`",
                            g.nodes[out].id, g.nodes[inside].id
                        ))
                        .primary(e.span, "this link"),
                    );
                    (out, Some(inside))
                }
                None => {
                    t.notes.push(
                        Diag::info(format!(
                            "`{}` is disconnected; attached under root",
                            g.nodes[v].id
                        ))
                        .primary(g.nodes[v].span, "no edges reach this node"),
                    );
                    (v, Some(root))
                }
            });
        }
    }
    for (i, e) in g.edges.iter().enumerate() {
        let tree_edge = t.parent[e.b] == Some(e.a) || t.parent[e.a] == Some(e.b);
        if used_link[i] || (e.kind == EdgeKind::Tree && tree_edge) {
            continue;
        }
        if e.kind == EdgeKind::Tree {
            t.notes.push(
                Diag::info(format!(
                    "edge `{}` -- `{}` closes a cycle; drawn as a cross link",
                    g.nodes[e.a].id, g.nodes[e.b].id
                ))
                .primary(e.span, "closes a cycle"),
            );
        }
        t.links.push(Link {
            a: e.a,
            b: e.b,
            label: e.label.clone(),
        });
    }
    t
}
