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

/// Space between a group's members and its frame, and between the frame and any other box or
/// frame, in ems.
pub const FRAME_PAD: f64 = 0.5;
pub const FRAME_CLEAR: f64 = 0.4;

/// frame is a group's border: around its members' boxes and nested groups' frames, with a
/// title band on top that is wide enough for the title. Only defined for `Relation::Group`.
pub fn frame(rel: &Relation, rect: &dyn Fn(usize) -> Rect, body: f64) -> Rect {
    let Relation::Group { items, title, title_em, inner, .. } = rel else { unreachable!("frame of a non-group") };
    let mut r = rect(items[0]);
    for b in items[1..].iter().map(|&v| rect(v)).chain(inner.iter().map(|g| frame(g, rect, body))) {
        r = Rect { x0: r.x0.min(b.x0), y0: r.y0.min(b.y0), x1: r.x1.max(b.x1), y1: r.y1.max(b.y1) };
    }
    let mut r = r.inflate(FRAME_PAD * body);
    if title.is_some() {
        r.y1 += TITLE_BAND * body;
        let short = (title_em + 2.0 * FRAME_PAD) * body - (r.x1 - r.x0);
        if short > 0.0 {
            (r.x0, r.x1) = (r.x0 - short / 2.0, r.x1 + short / 2.0);
        }
    }
    r
}

/// Height of a group's title band in ems; the title is drawn at 0.8em.
pub const TITLE_BAND: f64 = 1.1;

/// relation_holds is the meaning of a user constraint, shared by the solver and the checker.
/// `rect` gives a node's region; y grows upward. For a group it checks only that the frame is
/// on the page; `frame_clear` checks it against everything else.
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
        Relation::Group { .. } => frame(rel, rect, body).within(page),
    }
}

/// frame_clear reports whether a group's frame keeps clear of `other`: a box outside the group,
/// or the frame of a group that shares none of its nodes.
pub fn frame_clear(frame: &Rect, other: &Rect, body: f64) -> bool {
    !frame.inflate(FRAME_CLEAR * body).intersects(other)
}
