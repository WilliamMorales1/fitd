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
