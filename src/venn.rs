use crate::geom::{P, Rect};
use crate::graph::{EdgeKind, Graph};
use crate::layout::{Boxed, Cand, Drawn, EdgeStyle, Kind, Plan};
use crate::metrics::Fonts;

/// Circle is one set of a venn diagram.
#[derive(Clone, Copy, Debug)]
pub struct Circle {
    pub c: P,
    pub r: f64,
}

/// Title is a set name drawn beside its circle.
#[derive(Clone, Debug)]
pub struct Title {
    pub text: String,
    pub at: P,
    pub size: f64,
    pub rect: Rect,
}

/// Venn is the fixed geometry of a `layout venn` diagram: circles, titles, and each node's region.
#[derive(Clone, Debug)]
pub struct Venn {
    pub circles: Vec<Circle>,
    pub titles: Vec<Option<Title>>,
    /// Bit i set when the node belongs to set i.
    pub mask: Vec<u8>,
    pub margin: f64,
    page: Rect,
}

/// MAX_EULER caps the sets of a `layout euler` diagram, one mask bit each.
pub const MAX_EULER: usize = 8;

const SPREADS: [f64; 4] = [1.0, 0.8, 1.2, 0.6];
const MAX_CANDS: usize = 400;

impl Venn {
    /// admits tells whether `r` lies in the region `mask`, `m` clear of every circle edge and title.
    pub fn admits(&self, mask: u8, r: &Rect, m: f64) -> bool {
        if !r.within(&self.page) {
            return false;
        }
        let corners = [(r.x0, r.y0), (r.x0, r.y1), (r.x1, r.y0), (r.x1, r.y1)];
        for (i, c) in self.circles.iter().enumerate() {
            if mask & (1 << i) != 0 {
                let far = corners
                    .iter()
                    .map(|p| (p.0 - c.c.0).hypot(p.1 - c.c.1))
                    .fold(0.0, f64::max);
                if far > c.r - m {
                    return false;
                }
            } else {
                let qx = c.c.0.clamp(r.x0, r.x1);
                let qy = c.c.1.clamp(r.y0, r.y1);
                if (qx - c.c.0).hypot(qy - c.c.1) < c.r + m {
                    return false;
                }
            }
        }
        self.titles
            .iter()
            .flatten()
            .all(|t| !t.rect.inflate(m).intersects(r))
    }

    /// spots lists grid points where a `w` by `h` box fits the region `mask`.
    fn spots(&self, mask: u8, w: f64, h: f64, step: f64) -> Vec<(i64, i64, P)> {
        let mut area = self.page;
        for (i, c) in self.circles.iter().enumerate() {
            if mask & (1 << i) != 0 {
                area = Rect {
                    x0: area.x0.max(c.c.0 - c.r),
                    y0: area.y0.max(c.c.1 - c.r),
                    x1: area.x1.min(c.c.0 + c.r),
                    y1: area.y1.min(c.c.1 + c.r),
                };
            }
        }
        let mut out = Vec::new();
        let (i0, i1) = (
            (area.x0 / step).floor() as i64,
            (area.x1 / step).ceil() as i64,
        );
        let (j0, j1) = (
            (area.y0 / step).floor() as i64,
            (area.y1 / step).ceil() as i64,
        );
        for j in j0..=j1 {
            for i in i0..=i1 {
                let p = (i as f64 * step, j as f64 * step);
                if self.admits(mask, &Rect::centered(p, w, h), self.margin) {
                    out.push((i, j, p));
                }
            }
        }
        out
    }
}

/// masks gives each node a bit per set it belongs to.
fn masks(g: &Graph) -> Vec<u8> {
    let mut mask = vec![0u8; g.nodes.len()];
    for (i, s) in g.sets.iter().enumerate() {
        for &v in &s.items {
            mask[v] |= 1 << i;
        }
    }
    mask
}

/// circles places `k` circles of radius r with centers `d` apart, centered on the page.
fn circles(k: usize, page: &Rect, band: f64, q: f64) -> Vec<Circle> {
    let (w, h) = (page.x1 - page.x0, page.y1 - page.y0);
    let (cx, cy) = page.center();
    let s3 = 3f64.sqrt() / 2.0;
    match k {
        1 => {
            let r = (w / 2.0).min((h - band) / 2.0);
            vec![Circle {
                c: (cx, cy - band / 2.0),
                r,
            }]
        }
        2 => {
            let r = (w / (2.0 + q)).min((h - band) / 2.0);
            let d = q * r;
            [-0.5, 0.5]
                .iter()
                .map(|f| Circle {
                    c: (cx + f * d, cy - band / 2.0),
                    r,
                })
                .collect()
        }
        _ => {
            let r = (w / (2.0 + q)).min((h - 2.0 * band) / (2.0 + q * s3));
            let d = q * r;
            let ya = cy + d * s3 / 2.0;
            vec![
                Circle {
                    c: (cx - d / 2.0, ya),
                    r,
                },
                Circle {
                    c: (cx + d / 2.0, ya),
                    r,
                },
                Circle {
                    c: (cx, ya - d * s3),
                    r,
                },
            ]
        }
    }
}

/// shape picks the circle spread that leaves the most room in the most crowded region.
pub fn shape(g: &Graph, fonts: &Fonts, boxes: &[Boxed], page: Rect, b: f64) -> Venn {
    if g.settings.euler {
        return euler(g, fonts, boxes, page, b);
    }
    let k = g.sets.len().clamp(1, 3);
    let size = 1.2 * b;
    let band = 2.0 * size;
    let mask = masks(g);
    let mut best: Option<(f64, Venn)> = None;
    for q in SPREADS {
        let cs = circles(k, &page, band, q);
        if cs[0].r <= 0.0 {
            continue;
        }
        let titles = (0..k)
            .map(|i| {
                let text = g.sets.get(i)?.title.clone()?;
                let c = cs[i];
                let side = match (k, i) {
                    (1, _) | (3, 2) => 0.0,
                    (_, 0) => -1.0,
                    _ => 1.0,
                };
                let below = k == 3 && i == 2;
                let y = if below {
                    c.c.1 - c.r - band / 2.0
                } else {
                    c.c.1 + c.r + band / 2.0
                };
                let at = (c.c.0 + side * 0.35 * c.r, y);
                let tw = fonts.width(&text, size, true);
                Some(Title {
                    text,
                    at,
                    size,
                    rect: Rect::centered(at, tw, 1.25 * size),
                })
            })
            .collect();
        let v = Venn {
            circles: cs,
            titles,
            mask: mask.clone(),
            margin: 0.4 * b,
            page,
        };
        let step = v.circles[0].r / 12.0;
        let mut regions: Vec<u8> = mask.iter().copied().filter(|&m| m != 0).collect();
        regions.sort_unstable();
        regions.dedup();
        let score = regions
            .iter()
            .map(|&m| {
                let members: Vec<usize> = (0..mask.len()).filter(|&u| mask[u] == m).collect();
                let need: f64 = members.iter().map(|&u| boxes[u].w * boxes[u].h).sum();
                let big = members
                    .iter()
                    .max_by(|&&a, &&c| {
                        (boxes[a].w * boxes[a].h).total_cmp(&(boxes[c].w * boxes[c].h))
                    })
                    .unwrap();
                let room =
                    v.spots(m, boxes[*big].w, boxes[*big].h, step).len() as f64 * step * step;
                room / need.max(1e-9)
            })
            .fold(f64::INFINITY, f64::min);
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, v));
        }
    }
    best.map(|(_, v)| v).unwrap_or_else(|| Venn {
        circles: vec![
            Circle {
                c: page.center(),
                r: 0.0,
            };
            k
        ],
        titles: vec![None; k],
        mask,
        margin: 0.4 * b,
        page,
    })
}

/// plan offers each node grid spots inside its region, nearest the region's middle first.
pub fn plan(g: &Graph, venn: &Venn, boxes: &[Boxed], b: f64) -> Plan {
    let n = g.nodes.len();
    let step = 0.5 * b;
    let sep = (0.6 * b).max(3.0);
    let (cx, cy) = venn
        .circles
        .iter()
        .fold((0.0, 0.0), |a, c| (a.0 + c.c.0, a.1 + c.c.1));
    let mid = (
        cx / venn.circles.len() as f64,
        cy / venn.circles.len() as f64,
    );
    let mut cands: Vec<Vec<Cand>> = Vec::with_capacity(n);
    #[allow(clippy::needless_range_loop)]
    for v in 0..n {
        let m = venn.mask[v];
        let spots = venn.spots(m, boxes[v].w, boxes[v].h, step);
        let aim = if m == 0 || spots.is_empty() {
            mid
        } else {
            let k = spots.len() as f64;
            let (sx, sy) = spots
                .iter()
                .fold((0.0, 0.0), |a, s| (a.0 + s.2.0, a.1 + s.2.1));
            (sx / k, sy / k)
        };
        let every = ((spots.len() as f64 / MAX_CANDS as f64).sqrt().ceil() as i64).max(1);
        let mut c: Vec<Cand> = spots
            .iter()
            .filter(|s| s.0.rem_euclid(every) == 0 && s.1.rem_euclid(every) == 0)
            .map(|s| Cand {
                cost: (s.2.0 - aim.0).hypot(s.2.1 - aim.1),
                at: s.2,
                r: s.2.0.hypot(s.2.1),
            })
            .collect();
        c.sort_by(|a, b| a.cost.total_cmp(&b.cost));
        if m == 0 {
            c.truncate(MAX_CANDS);
        }
        cands.push(c);
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&v| cands[v].len());
    let mut cons = Vec::new();
    for u in 0..n {
        for v in u + 1..n {
            if venn.mask[u] == venn.mask[v] {
                cons.push((vec![u, v], Kind::Apart(sep), 0));
            }
        }
    }
    let edges = g
        .edges
        .iter()
        .map(|e| {
            let style = match e.kind {
                EdgeKind::Tree => EdgeStyle::Arrow,
                EdgeKind::Link => EdgeStyle::Link,
            };
            Drawn::new(e.a, e.b, style, 0.1, e.label.clone())
        })
        .collect();
    Plan {
        order,
        cands,
        cons,
        edges,
        kx: 1.0,
        ky: 1.0,
        tol: 0.5 * step,
        crowded: None,
    }
}

/// Pack is the natural size of one euler set: its child rows, content box, and circle radius.
#[derive(Clone, Default)]
struct Pack {
    rows: Vec<Vec<usize>>,
    w: f64,
    h: f64,
    r: f64,
    band: f64,
}

/// pack sizes a set holding `kids` and `own` node boxes, with its title above; fewest-radius row split wins.
fn pack(
    kids: &[usize],
    packs: &[Pack],
    own: &[(f64, f64)],
    title_w: f64,
    band: f64,
    gap: f64,
    fit: impl Fn(f64, f64) -> f64,
) -> Pack {
    let area: f64 = own.iter().map(|&(w, h)| (w + gap) * (h + gap)).sum();
    let wide = own.iter().map(|o| o.0 + gap).fold(0.0, f64::max);
    let ow = (area * 1.6).sqrt().max(wide);
    let oh = if own.is_empty() { 0.0 } else { area / ow };
    let mut best: Option<(f64, Pack)> = None;
    for nrows in 1..=kids.len().max(1) {
        let per = kids.len().div_ceil(nrows).max(1);
        let rows: Vec<Vec<usize>> = kids.chunks(per).map(<[usize]>::to_vec).collect();
        let (mut kw, mut kh) = (0.0f64, 0.0);
        for r in &rows {
            let w: f64 =
                r.iter().map(|&k| 2.0 * packs[k].r).sum::<f64>() + gap * (r.len() - 1) as f64;
            kw = kw.max(w);
            kh += r.iter().map(|&k| 2.0 * packs[k].r).fold(0.0, f64::max) + gap;
        }
        let w = kw.max(ow).max(title_w);
        let h = band + kh + oh;
        let p = Pack {
            rows,
            w,
            h,
            r: w.hypot(h) / 2.0 + gap / 2.0,
            band,
        };
        let cost = fit(p.w, p.h);
        if best.as_ref().is_none_or(|(c, _)| cost < *c) {
            best = Some((cost, p));
        }
        if kids.is_empty() {
            break;
        }
    }
    best.map(|(_, p)| p).unwrap_or_default()
}

/// euler nests each set's circle inside the smallest set holding it and sets the rest side by side.
fn euler(g: &Graph, fonts: &Fonts, boxes: &[Boxed], page: Rect, b: f64) -> Venn {
    let k = g.sets.len();
    let size = 1.2 * b;
    let gap = 0.8 * b;
    let mask = masks(g);
    let held = |i: usize, j: usize| {
        g.sets[i].items.len() < g.sets[j].items.len()
            && g.sets[i].items.iter().all(|v| g.sets[j].items.contains(v))
    };
    let parent: Vec<Option<usize>> = (0..k)
        .map(|i| {
            (0..k)
                .filter(|&j| held(i, j))
                .min_by_key(|&j| g.sets[j].items.len())
        })
        .collect();
    let owner = |u: usize| {
        (0..k)
            .filter(|&i| mask[u] & (1 << i) != 0)
            .min_by_key(|&i| g.sets[i].items.len())
            .unwrap_or(k)
    };
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_by_key(|&i| g.sets[i].items.len());
    let mut packs = vec![Pack::default(); k + 1];
    let mut titles: Vec<Option<(String, f64)>> = vec![None; k];
    let (pw, ph) = (page.x1 - page.x0 - 2.0 * b, page.y1 - page.y0 - 2.0 * b);
    for &i in order.iter().chain(std::iter::once(&k)) {
        let kids: Vec<usize> = (0..k).filter(|&c| parent[c] == Some(i)).collect();
        let kids = if i == k {
            (0..k).filter(|&c| parent[c].is_none()).collect()
        } else {
            kids
        };
        let own: Vec<(f64, f64)> = (0..g.nodes.len())
            .filter(|&u| owner(u) == i)
            .map(|u| (boxes[u].w, boxes[u].h))
            .collect();
        let title = (i < k).then(|| g.sets[i].title.clone()).flatten();
        let tw = title
            .as_ref()
            .map_or(0.0, |t| fonts.width(t, size, true) + gap);
        let band = if title.is_some() {
            1.25 * size + 0.3 * b
        } else {
            0.0
        };
        packs[i] = if i == k {
            pack(&kids, &packs, &own, 0.0, 0.0, gap, |w, h| {
                -(pw / w).min(ph / h)
            })
        } else {
            pack(&kids, &packs, &own, tw, band, gap, |w, h| w.hypot(h))
        };
        if let Some(t) = title {
            titles[i] = Some((t, tw));
        }
    }
    let root = &packs[k];
    let s = (pw / root.w).min(ph / root.h).max(1e-6);
    let mut circles = vec![
        Circle {
            c: (0.0, 0.0),
            r: 0.0
        };
        k
    ];
    let mut at = vec![(0.0, 0.0); k];
    // Walk from the page in: each set's content box is centered on its circle.
    let mut stack = vec![(k, page.center())];
    while let Some((i, (cx, cy))) = stack.pop() {
        let p = &packs[i];
        let mut y = cy + p.h * s / 2.0 - p.band * s;
        for row in &p.rows {
            let rh = row.iter().map(|&c| 2.0 * packs[c].r).fold(0.0, f64::max);
            let rw: f64 =
                row.iter().map(|&c| 2.0 * packs[c].r).sum::<f64>() + gap * (row.len() - 1) as f64;
            let mut x = cx - rw * s / 2.0;
            for &c in row {
                let r = packs[c].r;
                let ctr = (x + r * s, y - rh * s / 2.0);
                circles[c] = Circle { c: ctr, r: r * s };
                stack.push((c, ctr));
                x += (2.0 * r + gap) * s;
            }
            y -= (rh + gap) * s;
        }
        if i < k {
            at[i] = (cx, cy + p.h * s / 2.0 - p.band * s / 2.0);
        }
    }
    let titles = (0..k)
        .map(|i| {
            let (text, tw) = titles[i].take()?;
            Some(Title {
                rect: Rect::centered(at[i], tw - gap, 1.25 * size),
                text,
                at: at[i],
                size,
            })
        })
        .collect();
    Venn {
        circles,
        titles,
        mask,
        margin: 0.4 * b,
        page,
    }
}
