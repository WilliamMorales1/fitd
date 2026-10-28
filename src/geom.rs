pub type P = (f64, f64);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    pub fn centered(c: P, w: f64, h: f64) -> Rect {
        Rect { x0: c.0 - w / 2.0, y0: c.1 - h / 2.0, x1: c.0 + w / 2.0, y1: c.1 + h / 2.0 }
    }

    pub fn inflate(&self, g: f64) -> Rect {
        Rect { x0: self.x0 - g, y0: self.y0 - g, x1: self.x1 + g, y1: self.y1 + g }
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1
    }

    pub fn contains(&self, p: P) -> bool {
        p.0 >= self.x0 && p.0 <= self.x1 && p.1 >= self.y0 && p.1 <= self.y1
    }

    pub fn within(&self, outer: &Rect) -> bool {
        self.x0 >= outer.x0 && self.x1 <= outer.x1 && self.y0 >= outer.y0 && self.y1 <= outer.y1
    }
}

/// segment_hits_rect reports whether segment ab passes through the interior of r (Liang-Barsky).
pub fn segment_hits_rect(a: P, b: P, r: &Rect) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-dx, a.0 - r.x0), (dx, r.x1 - a.0), (-dy, a.1 - r.y0), (dy, r.y1 - a.1)] {
        if p == 0.0 {
            if q <= 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 { t0 = t0.max(t) } else { t1 = t1.min(t) }
        }
    }
    t0 < t1
}

fn orient(a: P, b: P, c: P) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// segments_cross reports a proper crossing; segments sharing an endpoint never cross.
pub fn segments_cross(a: P, b: P, c: P, d: P) -> bool {
    let near = |p: P, q: P| (p.0 - q.0).abs() < 1e-6 && (p.1 - q.1).abs() < 1e-6;
    if near(a, c) || near(a, d) || near(b, c) || near(b, d) {
        return false;
    }
    orient(a, b, c) * orient(a, b, d) < 0.0 && orient(c, d, a) * orient(c, d, b) < 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_through_and_beside_rect() {
        let r = Rect::centered((0.0, 0.0), 2.0, 2.0);
        assert!(segment_hits_rect((-5.0, 0.0), (5.0, 0.0), &r));
        assert!(!segment_hits_rect((-5.0, 3.0), (5.0, 3.0), &r));
        assert!(!segment_hits_rect((-5.0, 0.0), (-2.0, 0.0), &r));
    }

    #[test]
    fn crossing_ignores_shared_endpoints() {
        assert!(segments_cross((0.0, 0.0), (2.0, 2.0), (0.0, 2.0), (2.0, 0.0)));
        assert!(!segments_cross((0.0, 0.0), (2.0, 2.0), (0.0, 0.0), (2.0, 0.0)));
        assert!(!segments_cross((0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)));
    }
}
