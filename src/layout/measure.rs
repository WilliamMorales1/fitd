use super::*;
use crate::graph::{Cell, EdgeKind};

pub fn label_size(fonts: &Fonts, text: &str, body: f64) -> (f64, f64) {
    (
        fonts.width(text, 0.8 * body, false) + 0.4 * body,
        1.04 * body,
    )
}

pub(super) fn words(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth, mut prev) = (Vec::new(), String::new(), 0, ' ');
    for c in s.chars() {
        if c == '{' && (prev == '^' || prev == '_') {
            depth += 1;
        } else if c == '}' && depth > 0 {
            depth -= 1;
        }
        if c == ' ' && depth == 0 {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
        prev = c;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn wrap(fonts: &Fonts, text: &str, size: f64, bold: bool, maxw: f64) -> Vec<String> {
    let words = words(text);
    if words.len() < 2 {
        return vec![words.into_iter().next().unwrap_or_default()];
    }
    // A word inside a `**` span is measured bold even when the span opened in an earlier word.
    let mut on = false;
    let ww: Vec<f64> = words
        .iter()
        .map(|w| {
            let open = if on { "**" } else { "" };
            on ^= w.matches("**").count() % 2 == 1;
            fonts.marked_width(&format!("{open}{w}"), size, bold)
        })
        .collect();
    let space = fonts.width(" ", size, bold);
    let width = |i: usize, j: usize| ww[i..j].iter().sum::<f64>() + space * (j - i - 1) as f64;
    let fits = |i: usize, j: usize| j == i + 1 || width(i, j) <= maxw;
    let mut lines = 0;
    let mut i = 0;
    while i < words.len() {
        let mut j = i + 1;
        while j < words.len() && fits(i, j + 1) {
            j += 1;
        }
        lines += 1;
        i = j;
    }
    let n = words.len();
    let mut cost = vec![vec![f64::INFINITY; n + 1]; lines + 1];
    let mut brk = vec![vec![0; n + 1]; lines + 1];
    cost[0][n] = 0.0;
    for k in 1..=lines {
        for i in 0..n {
            for j in (i + 1..=n).take_while(|&j| fits(i, j)) {
                let c = cost[k - 1][j] + (maxw - width(i, j)).max(0.0).powi(2);
                if c < cost[k][i] {
                    (cost[k][i], brk[k][i]) = (c, j);
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    for k in (1..=lines).rev() {
        let j = brk[k][i];
        out.push(words[i..j].join(" "));
        i = j;
    }
    // A `**` span broken across lines is closed at the break and reopened on the next line.
    let mut on = false;
    for l in &mut out {
        let flips = l.matches("**").count() % 2 == 1;
        if on {
            l.insert_str(0, "**");
        }
        on ^= flips;
        if on {
            l.push_str("**");
        }
    }
    out
}

/// INDENT is the body indent per leading space, in body sizes.
const INDENT: f64 = 0.6;

pub fn line_height(size: f64) -> f64 {
    1.22 * size
}

pub fn measure(
    fonts: &Fonts,
    g: &Graph,
    v: usize,
    role: Role,
    b: f64,
    cfg: &Config,
    beside: bool,
) -> Boxed {
    let (ts, bs, wrap_w, shape, pad) = match role {
        Role::Root => (1.73 * b, 0.95 * b, 16.0 * b, Shape::Circle, 0.35 * b),
        Role::Hub => (1.44 * b, 0.9 * b, 11.0 * b, Shape::Circle, 0.35 * b),
        Role::Leaf => (1.2 * b, b, cfg.wrap_em * b, Shape::Rect, 0.5 * b),
    };
    let node = &g.nodes[v];
    let wrap_w = node.wrap_em.map_or(wrap_w, |em| em * b);
    let shape = match g.settings.venn {
        true => node.shape.unwrap_or(Shape::Plain),
        false => node.shape.unwrap_or(shape),
    };
    // A frameless node has no border to clear, so it keeps only enough padding to breathe.
    // An empty frameless node is a point, such as a message end on a lifeline.
    let point = shape == Shape::Plain
        && node.title.is_empty()
        && node.body.is_empty()
        && node.figure.is_empty();
    let pad = match shape {
        _ if point => 0.05 * b,
        Shape::Plain => 0.15 * b,
        // A diamond already leaves room at its corners.
        Shape::Diamond => 0.25 * b,
        _ => pad,
    };
    let mut raw: Vec<(String, f64, bool, f64)> = Vec::new();
    for part in node.title.split('\n').filter(|_| !point) {
        for l in wrap(fonts, part, ts, true, wrap_w) {
            raw.push((l, ts, true, 0.0));
        }
    }
    let ntitle = raw.len();
    // Leading spaces indent a body line, as in pseudocode; centred shapes have no left edge to indent from.
    let left = matches!(shape, Shape::Rect | Shape::Plain);
    for part in &node.body {
        let text = part.trim_start_matches(' ');
        let indent = match left {
            true => (part.len() - text.len()) as f64 * INDENT * bs,
            false => 0.0,
        };
        for l in wrap(fonts, text, bs, false, (wrap_w - indent).max(bs)) {
            raw.push((l, bs, false, indent));
        }
    }
    let title_gap = if raw.len() > ntitle { 0.25 * b } else { 0.0 };
    let mut lines = Vec::new();
    let mut top = 0.0;
    for (i, (text, size, bold, indent)) in raw.into_iter().enumerate() {
        if i == ntitle {
            top += title_gap;
        }
        let runs = fonts.runs(&text, bold);
        let width = fonts.runs_width(&runs, size);
        lines.push(TextLine {
            runs,
            size,
            bold,
            width,
            x: indent,
            baseline: top + 0.96 * size,
        });
        top += line_height(size);
    }
    let text_h = top;
    let (mut cells, fw, fh) = figure(fonts, &node.figure, bs);
    let fig_gap = if fh > 0.0 && text_h > 0.0 {
        0.3 * b
    } else {
        0.0
    };
    let text_w = lines.iter().map(|l| l.x + l.width).fold(0.0, f64::max);
    let beside = beside && fig_gap > 0.0 && matches!(shape, Shape::Rect | Shape::Plain);
    let content_w = if beside {
        text_w + 2.0 * fig_gap + fw
    } else {
        text_w.max(fw)
    };
    let content_h = if beside {
        text_h.max(fh)
    } else {
        text_h + fig_gap + fh
    };
    let (w, h, dx, dy) = match shape {
        _ if beside => {
            for l in &mut lines {
                l.x += pad;
                l.baseline += pad;
            }
            (
                content_w + 2.0 * pad,
                content_h + 2.0 * pad,
                pad + text_w + 2.0 * fig_gap,
                pad + (content_h - fh) / 2.0,
            )
        }
        Shape::Rect | Shape::Plain => {
            for l in &mut lines {
                l.x += pad;
                l.baseline += pad;
            }
            (
                content_w + 2.0 * pad,
                content_h + 2.0 * pad,
                pad + (content_w - fw) / 2.0,
                pad + text_h + fig_gap,
            )
        }
        Shape::Diamond => {
            // Scale the padded content rect until every line corner lies inside the rhombus.
            let (a, c) = (content_w / 2.0 + pad, content_h / 2.0 + pad);
            let corner =
                |hw: f64, y0: f64, y1: f64| (hw + pad) / a + (y0.abs().max(y1.abs()) + pad) / c;
            let mut s: f64 = 0.0;
            for l in &lines {
                let y0 = l.baseline - 0.96 * l.size - content_h / 2.0;
                s = s.max(corner(l.width / 2.0, y0, y0 + line_height(l.size)));
            }
            if fh > 0.0 {
                let y0 = text_h + fig_gap - content_h / 2.0;
                s = s.max(corner(fw / 2.0, y0, y0 + fh));
            }
            let (rx, ry) = (s * a, s * c);
            for l in &mut lines {
                l.x = rx - l.width / 2.0;
                l.baseline += ry - content_h / 2.0;
            }
            (
                2.0 * rx,
                2.0 * ry,
                rx - fw / 2.0,
                ry - content_h / 2.0 + text_h + fig_gap,
            )
        }
        Shape::Circle => {
            let mut r: f64 = 0.0;
            for l in &lines {
                let top_y = l.baseline - 0.96 * l.size - content_h / 2.0;
                let far = top_y.abs().max((top_y + line_height(l.size)).abs());
                r = r.max((l.width / 2.0).hypot(far));
            }
            if fh > 0.0 {
                let y0 = text_h + fig_gap - content_h / 2.0;
                r = r.max((fw / 2.0).hypot(y0.abs().max((y0 + fh).abs())));
            }
            let r = r + pad;
            for l in &mut lines {
                l.x = r - l.width / 2.0;
                l.baseline += r - content_h / 2.0;
            }
            (
                2.0 * r,
                2.0 * r,
                r - fw / 2.0,
                r - content_h / 2.0 + text_h + fig_gap,
            )
        }
    };
    // Stacked copies sit up and to the right, so the box grows and its content slides down.
    let stack = node.stack.unwrap_or(1).max(1);
    let shift = if stack > 1 {
        0.42 * b * (stack - 1) as f64
    } else {
        0.0
    };
    for l in &mut lines {
        l.baseline += shift;
    }
    for c in &mut cells {
        c.x += dx;
        c.y += dy + shift;
        if let Some(l) = &mut c.label {
            l.x += dx;
            l.baseline += dy + shift;
        }
    }
    Boxed {
        shape,
        role,
        w: w + shift,
        h: h + shift,
        pad,
        stack,
        shift,
        lines,
        cells,
    }
}

/// figure lays out a node's `~` rows as a grid, returning cells plus the grid's size.
fn figure(fonts: &Fonts, rows: &[Vec<Cell>], bs: f64) -> (Vec<CellBox>, f64, f64) {
    if rows.is_empty() {
        return (Vec::new(), 0.0, 0.0);
    }
    let (side, gap, ls) = (1.35 * bs, 0.16 * bs, 0.82 * bs);
    let cols = rows
        .iter()
        .map(|r| r.iter().map(|c| c.span).sum::<usize>())
        .max()
        .unwrap_or(0);
    let need = |c: &Cell| {
        c.label
            .as_ref()
            .map_or(0.0, |t| fonts.width(t, ls, false) + 0.7 * bs)
    };
    let mut cw = vec![side; cols];
    for row in rows {
        let mut j = 0;
        for c in row {
            if c.span == 1 && j < cols {
                cw[j] = cw[j].max(need(c));
            }
            j += c.span;
        }
    }
    // A spanning label that still does not fit widens only the last column it covers.
    for row in rows {
        let mut j = 0;
        for c in row {
            if c.span > 1 && j + c.span <= cols {
                let have = cw[j..j + c.span].iter().sum::<f64>() + gap * (c.span - 1) as f64;
                if need(c) > have {
                    cw[j + c.span - 1] += need(c) - have;
                }
            }
            j += c.span;
        }
    }
    let rh = side.max(line_height(ls) + 0.3 * bs);
    let mut cells = Vec::new();
    let mut y = 0.0;
    for row in rows {
        let (mut x, mut j) = (0.0, 0);
        for c in row {
            let end = (j + c.span).min(cols);
            let w =
                cw[j.min(cols)..end].iter().sum::<f64>() + gap * end.saturating_sub(j + 1) as f64;
            if !c.gap {
                let label = c.label.as_ref().map(|t| {
                    let runs = fonts.runs(t, false);
                    let width = fonts.runs_width(&runs, ls);
                    TextLine {
                        runs,
                        size: ls,
                        bold: false,
                        width,
                        x: x + (w - width) / 2.0,
                        baseline: y + rh / 2.0 + 0.34 * ls,
                    }
                });
                cells.push(CellBox {
                    x,
                    y,
                    w,
                    h: rh,
                    shade: c.shade,
                    label,
                });
            }
            x += w + gap;
            j = end;
        }
        y += rh + gap;
    }
    let fw = cw.iter().sum::<f64>() + gap * (cols - 1) as f64;
    (cells, fw, y - gap)
}

pub(super) fn measure_all(g: &Graph, t: &Tree, fonts: &Fonts, cfg: &Config, b: f64) -> Vec<Boxed> {
    let n = g.nodes.len();
    match g.settings.layout.unwrap_or(LayoutKind::Radial) {
        LayoutKind::Radial => (0..n)
            .map(|v| measure(fonts, g, v, radial::role(t, v), b, cfg, false))
            .collect(),
        LayoutKind::Layered(dir) => {
            // A strip node set beside the flow is wider than tall to spare the cross axis.
            let beside = |v: usize| dir == Dir::Right && in_strip(g, v);
            let mut boxes: Vec<Boxed> = (0..n)
                .map(|v| measure(fonts, g, v, Role::Leaf, b, cfg, beside(v)))
                .collect();
            even_layers(g, dir, &mut boxes);
            boxes
        }
    }
}

/// in_strip reports whether an aside node really leaves the flow.
fn in_strip(g: &Graph, v: usize) -> bool {
    let node = &g.nodes[v];
    node.aside
        && !g.edges.iter().any(|e| {
            e.kind == EdgeKind::Tree
                && (e.a == v || e.b == v)
                && !(g.nodes[e.a].aside && g.nodes[e.b].aside)
        })
        && !g
            .constraints
            .iter()
            .any(|c| matches!(&c.rel, Relation::Group { items, .. } if items.contains(&v)))
}

/// even_layers gives look-alike nodes in one layer the same length along the flow.
fn even_layers(g: &Graph, dir: Dir, boxes: &mut [Boxed]) {
    let layer = match g.settings.lanes {
        true => crate::lanes::layer(g, dir),
        false => crate::layered::layering(g, dir).layer,
    };
    let n = boxes.len();
    let alike = |u: usize, v: usize| {
        let (a, c) = (&g.nodes[u], &g.nodes[v]);
        layer[u] == layer[v]
            && a.color == c.color
            && a.fill == c.fill
            && a.shape == c.shape
            && a.stack == c.stack
            && !in_strip(g, u)
            && !in_strip(g, v)
    };
    let along = |bx: &Boxed| if dir == Dir::Right { bx.w } else { bx.h };
    let target: Vec<f64> = (0..n)
        .map(|v| {
            (0..n)
                .filter(|&u| alike(u, v))
                .map(|u| along(&boxes[u]))
                .fold(along(&boxes[v]), f64::max)
        })
        .collect();
    for (bx, t) in boxes.iter_mut().zip(target) {
        let extra = t - along(bx);
        if extra <= 0.0 {
            continue;
        }
        // Text stays left and top, except in a diamond, where it is centred; the figure recenters.
        if bx.shape == Shape::Diamond {
            for l in &mut bx.lines {
                if dir == Dir::Right {
                    l.x += extra / 2.0;
                } else {
                    l.baseline += extra / 2.0;
                }
            }
        }
        for c in &mut bx.cells {
            let l = c.label.as_mut();
            if dir == Dir::Right {
                c.x += extra / 2.0;
                if let Some(l) = l {
                    l.x += extra / 2.0;
                }
            } else {
                c.y += extra / 2.0;
                if let Some(l) = l {
                    l.baseline += extra / 2.0;
                }
            }
        }
        if dir == Dir::Right {
            bx.w = t;
        } else {
            bx.h = t;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_balances_lines_without_adding_any() {
        let fonts = Fonts::load(None).unwrap();
        let text = "lakes wetlands and soil moisture buffer floods";
        let w = fonts.width("lakes wetlands and soil moisture", 10.0, false);
        let lines = wrap(&fonts, text, 10.0, false, w);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines.iter().all(|l| l.contains(' ')),
            "lone word in {lines:?}"
        );
        assert!(
            lines.iter().all(|l| fonts.width(l, 10.0, false) <= w),
            "{lines:?}"
        );
        assert_eq!(lines.join(" "), text);
    }

    #[test]
    fn wrap_reopens_a_bold_span_on_the_next_line() {
        let fonts = Fonts::load(None).unwrap();
        let w = fonts.width("plain one two", 10.0, false);
        let lines = wrap(&fonts, "plain **one two three four** end", 10.0, false, w);
        assert!(lines.len() > 1, "{lines:?}");
        assert!(
            lines.iter().all(|l| l.matches("**").count() % 2 == 0),
            "{lines:?}"
        );
        assert!(lines[1].starts_with("**"), "{lines:?}");
    }
}
