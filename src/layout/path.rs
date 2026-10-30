//! Edge paths: the curve each edge is drawn along, label spots on it, and path tests.

use super::*;

/// Bends tried for dashed links, preferred first, and label positions along any edge.
pub(super) const BOWS: [f64; 8] = [0.22, -0.22, 0.4, -0.4, 0.1, -0.1, 0.6, -0.6];

pub(super) const SPOTS: [f64; 9] = [0.5, 0.42, 0.58, 0.34, 0.66, 0.26, 0.74, 0.18, 0.82];

pub(super) fn inside_box(c: P, shape: Shape, w: f64, h: f64, p: P) -> bool {
    match shape {
        Shape::Circle => (p.0 - c.0).hypot(p.1 - c.1) <= w / 2.0,
        Shape::Rect => Rect::centered(c, w, h).contains(p),
    }
}

/// curve is the path of an edge: a quadratic Bezier from `pa` to `pb` bent by `bow` times its
/// length away from the page center (0 is straight), trimmed to the part outside both boxes.
pub fn curve(pa: P, in_a: &dyn Fn(P) -> bool, pb: P, in_b: &dyn Fn(P) -> bool, bow: f64) -> Vec<P> {
    let m = ((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0);
    let d = (pb.0 - pa.0).hypot(pb.1 - pa.1).max(1e-9);
    let mut n = (-(pb.1 - pa.1) / d, (pb.0 - pa.0) / d);
    if n.0 * m.0 + n.1 * m.1 < 0.0 {
        n = (-n.0, -n.1);
    }
    let c = (m.0 + bow * d * n.0, m.1 + bow * d * n.1);
    let pts: Vec<P> = (0..=60)
        .map(|i| {
            let t = i as f64 / 60.0;
            let (u, w) = ((1.0 - t) * (1.0 - t), 2.0 * (1.0 - t) * t);
            (u * pa.0 + w * c.0 + t * t * pb.0, u * pa.1 + w * c.1 + t * t * pb.1)
        })
        .collect();
    let s = pts.iter().position(|&p| !in_a(p)).unwrap_or(0);
    let e = pts.iter().rposition(|&p| !in_b(p)).unwrap_or(pts.len() - 1).max(s);
    pts[s..=e].to_vec()
}

/// label_rect is the box of a label centered at fraction `t` of a path.
pub fn label_rect(path: &[P], t: f64, size: (f64, f64)) -> Rect {
    let i = ((path.len() - 1) as f64 * t).round() as usize;
    Rect::centered(path[i], size.0, size.1)
}

/// paths_meet reports whether two paths cross or run within `d` of each other, ignoring the
/// parts inside `skip` (around shared endpoints, where links naturally meet).
pub fn paths_meet(p: &[P], q: &[P], d: f64, skip: &[Rect]) -> bool {
    if !bounds(p).inflate(d).intersects(&bounds(q)) {
        return false;
    }
    let free = |x: &P| skip.iter().all(|r| !r.contains(*x));
    let (fp, fq): (Vec<bool>, Vec<bool>) = (p.iter().map(free).collect(), q.iter().map(free).collect());
    for i in 0..p.len() {
        if !fp[i] {
            continue;
        }
        for j in 0..q.len() {
            if !fq[j] {
                continue;
            }
            if (p[i].0 - q[j].0).hypot(p[i].1 - q[j].1) < d {
                return true;
            }
            if i + 1 < p.len() && j + 1 < q.len() && fp[i + 1] && fq[j + 1] && segments_cross(p[i], p[i + 1], q[j], q[j + 1]) {
                return true;
            }
        }
    }
    false
}

pub(super) fn bounds(p: &[P]) -> Rect {
    p.iter().fold(Rect { x0: f64::INFINITY, y0: f64::INFINITY, x1: f64::NEG_INFINITY, y1: f64::NEG_INFINITY }, |r, &(x, y)| Rect {
        x0: r.x0.min(x),
        y0: r.y0.min(y),
        x1: r.x1.max(x),
        y1: r.y1.max(y),
    })
}
