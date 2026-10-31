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
    trim(&pts, in_a, in_b)
}

/// trim keeps the part of a path from where it leaves box `a` to where it enters box `b`.
pub fn trim(pts: &[P], in_a: &dyn Fn(P) -> bool, in_b: &dyn Fn(P) -> bool) -> Vec<P> {
    let s = pts.iter().position(|&p| !in_a(p)).unwrap_or(0);
    let e = pts.iter().rposition(|&p| !in_b(p)).unwrap_or(pts.len() - 1).max(s);
    pts[s..=e].to_vec()
}

/// detours are paths from `pa` to `pb` around `boxes`, kept `clear` away from each and inside
/// `page`: the shortest route over the corners of the grown boxes (a visibility graph). The
/// first is planned three times as far out with its corners rounded within the extra room;
/// then the tight route with small rounded corners, and sharp in case rounding cuts into a
/// box. Empty when the straight line is already clear or no route exists. Paths are
/// untrimmed, about 60 points.
pub fn detours(pa: P, pb: P, boxes: &[Rect], page: &Rect, clear: f64) -> Vec<Vec<P>> {
    let Some(way) = route(pa, pb, boxes, page, clear) else { return Vec::new() };
    let mut out = Vec::new();
    if let Some(wide) = route(pa, pb, boxes, page, 3.0 * clear) {
        out.push(densify(&fillet(&wide, 2.0 * clear), 60));
    }
    out.extend([densify(&fillet(&way, clear), 60), densify(&way, 60)]);
    out
}

/// route is the shortest polyline from `pa` to `pb` over the corners of `boxes` grown by
/// `clear`, or None when the straight line is clear or nothing gets through.
fn route(pa: P, pb: P, boxes: &[Rect], page: &Rect, clear: f64) -> Option<Vec<P>> {
    let grown: Vec<Rect> = boxes.iter().map(|r| r.inflate(clear)).collect();
    // Slightly smaller than `grown`, so a segment along a grown box's side still counts as clear.
    let hard: Vec<Rect> = boxes.iter().map(|r| r.inflate(0.98 * clear)).collect();
    let seen = |p: P, q: P| {
        let bb = Rect { x0: p.0.min(q.0), y0: p.1.min(q.1), x1: p.0.max(q.0), y1: p.1.max(q.1) };
        hard.iter().all(|r| !r.intersects(&bb.inflate(1e-9)) || !segment_hits_rect(p, q, r))
    };
    if seen(pa, pb) {
        return None;
    }
    let mut pts = vec![pa, pb];
    for r in &grown {
        for c in [(r.x0, r.y0), (r.x1, r.y0), (r.x0, r.y1), (r.x1, r.y1)] {
            if page.contains(c) && hard.iter().all(|h| !(h.x0 < c.0 && c.0 < h.x1 && h.y0 < c.1 && c.1 < h.y1)) {
                pts.push(c);
            }
        }
    }
    let way = shortest(&pts, &seen)?;
    Some(way.iter().map(|&i| pts[i]).collect())
}

/// fillet replaces each inner corner of a polyline with a quadratic arc that starts at most
/// `reach` before it, which stays within half of `reach` of the corner.
fn fillet(way: &[P], reach: f64) -> Vec<P> {
    let mut out = vec![way[0]];
    for i in 1..way.len() - 1 {
        let (p, c, q) = (way[i - 1], way[i], way[i + 1]);
        let toward = |a: P, d: f64| {
            let len = (a.0 - c.0).hypot(a.1 - c.1).max(1e-9);
            let k = d.min(len / 2.0) / len;
            (c.0 + k * (a.0 - c.0), c.1 + k * (a.1 - c.1))
        };
        let (s, e) = (toward(p, reach), toward(q, reach));
        out.extend((0..=8).map(|j| {
            let t = j as f64 / 8.0;
            let (u, w) = ((1.0 - t) * (1.0 - t), 2.0 * (1.0 - t) * t);
            (u * s.0 + w * c.0 + t * t * e.0, u * s.1 + w * c.1 + t * t * e.1)
        }));
    }
    out.push(way[way.len() - 1]);
    out
}

/// shortest is Dijkstra from point 0 to point 1 over the legs `seen` allows. Legs are tested
/// for visibility only when relaxed.
fn shortest(pts: &[P], seen: &dyn Fn(P, P) -> bool) -> Option<Vec<usize>> {
    let n = pts.len();
    let (mut dist, mut prev, mut done) = (vec![f64::INFINITY; n], vec![usize::MAX; n], vec![false; n]);
    dist[0] = 0.0;
    loop {
        let u = (0..n).filter(|&u| !done[u] && dist[u].is_finite()).min_by(|&a, &b| dist[a].total_cmp(&dist[b]))?;
        if u == 1 {
            break;
        }
        done[u] = true;
        for v in 0..n {
            let d = dist[u] + (pts[v].0 - pts[u].0).hypot(pts[v].1 - pts[u].1);
            if !done[v] && d < dist[v] && seen(pts[u], pts[v]) {
                (dist[v], prev[v]) = (d, u);
            }
        }
    }
    let mut way = vec![1];
    while prev[*way.last().unwrap()] != usize::MAX {
        way.push(prev[*way.last().unwrap()]);
    }
    way.reverse();
    Some(way)
}

/// densify samples a polyline at about `n` evenly spaced points.
fn densify(way: &[P], n: usize) -> Vec<P> {
    let len: f64 = way.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum();
    let mut out = vec![way[0]];
    for w in way.windows(2) {
        let k = (((w[1].0 - w[0].0).hypot(w[1].1 - w[0].1) / len * n as f64).ceil() as usize).max(1);
        out.extend((1..=k).map(|i| {
            let t = i as f64 / k as f64;
            (w[0].0 + t * (w[1].0 - w[0].0), w[0].1 + t * (w[1].1 - w[0].1))
        }));
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detour_goes_around_a_box() {
        let page = Rect::centered((0.0, 0.0), 200.0, 200.0);
        let wall = Rect::centered((0.0, 0.0), 20.0, 120.0);
        assert!(detours((-50.0, 0.0), (50.0, 30.0), &[Rect::centered((0.0, 90.0), 5.0, 5.0)], &page, 4.0).is_empty(), "straight line is clear");
        let ways = detours((-50.0, 0.0), (50.0, 0.0), &[wall], &page, 4.0);
        assert_eq!(ways.len(), 3);
        for w in &ways {
            assert_eq!((w[0], *w.last().unwrap()), ((-50.0, 0.0), (50.0, 0.0)));
            assert!(w.windows(2).all(|s| !segment_hits_rect(s[0], s[1], &wall)), "{w:?}");
        }
        // A wall across the whole page leaves no way around.
        assert!(detours((-50.0, 0.0), (50.0, 0.0), &[Rect::centered((0.0, 0.0), 20.0, 400.0)], &page, 4.0).is_empty());
    }
}
