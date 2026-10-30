//! Whether a user relation holds for given node boxes.

use super::*;

pub(super) fn aligned(side: Side, a: &Rect, b: &Rect, tol: f64) -> bool {
    let d = match side {
        Side::Top => a.y1 - b.y1,
        Side::Bottom => a.y0 - b.y0,
        Side::Left => a.x0 - b.x0,
        Side::Right => a.x1 - b.x1,
        Side::Row => (a.y0 + a.y1 - b.y0 - b.y1) / 2.0,
        Side::Column => (a.x0 + a.x1 - b.x0 - b.x1) / 2.0,
    };
    d.abs() <= tol + 1e-9
}

/// relation_holds is the meaning of a user constraint, shared by the solver and the checker.
/// `rect` gives a node's region; y grows upward.
pub fn relation_holds(rel: &Relation, rect: &dyn Fn(usize) -> Rect, body: f64, page: &Rect, tol: f64) -> bool {
    match rel {
        Relation::LeftOf { a, b, gap } => rect(*a).x1 + gap.pt(body) <= rect(*b).x0 + 1e-9,
        Relation::Above { a, b, gap } => rect(*a).y0 >= rect(*b).y1 + gap.pt(body) - 1e-9,
        Relation::Near { a, b, within } => {
            let (ra, rb) = (rect(*a), rect(*b));
            let dx = (ra.x0 - rb.x1).max(rb.x0 - ra.x1).max(0.0);
            let dy = (ra.y0 - rb.y1).max(rb.y0 - ra.y1).max(0.0);
            dx.hypot(dy) <= within.pt(body) + 1e-9
        }
        Relation::Align { side, items } => items.windows(2).all(|w| aligned(*side, &rect(w[0]), &rect(w[1]), tol)),
        Relation::At { a, place } => {
            let r = rect(*a);
            let (cx, cy) = ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
            let (w, h) = ((page.x1 - page.x0) / 3.0, (page.y1 - page.y0) / 3.0);
            let (x0, y1) = (page.x0 + w * place.0 as f64, page.y1 - h * place.1 as f64);
            cx >= x0 - 1e-9 && cx <= x0 + w + 1e-9 && cy <= y1 + 1e-9 && cy >= y1 - h - 1e-9
        }
    }
}
