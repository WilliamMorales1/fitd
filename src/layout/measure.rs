//! Text measurement: wrapping titles and bodies, and sizing each node's box.

use super::*;

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

/// wrap breaks text into lines no wider than `maxw` (a longer word gets a line of its own).
/// It uses as few lines as greedy filling would, then balances them, so the last line is not
/// left with a lone word.
pub fn wrap(fonts: &Fonts, text: &str, size: f64, bold: bool, maxw: f64) -> Vec<String> {
    let words = words(text);
    if words.len() < 2 {
        return vec![words.into_iter().next().unwrap_or_default()];
    }
    // Text is measured glyph by glyph without kerning, so widths add up.
    let ww: Vec<f64> = words.iter().map(|w| fonts.width(w, size, bold)).collect();
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
    // cost[k][i]: least sum of squared slack setting words i.. on exactly k lines.
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
    out
}

pub fn line_height(size: f64) -> f64 {
    1.22 * size
}

pub fn measure(fonts: &Fonts, g: &Graph, v: usize, role: Role, b: f64, cfg: &Config) -> Boxed {
    let (ts, bs, wrap_w, shape, pad) = match role {
        Role::Root => (1.73 * b, 0.95 * b, 16.0 * b, Shape::Circle, 0.35 * b),
        Role::Hub => (1.44 * b, 0.9 * b, 11.0 * b, Shape::Circle, 0.35 * b),
        Role::Leaf => (1.2 * b, b, cfg.wrap_em * b, Shape::Rect, 0.5 * b),
    };
    let node = &g.nodes[v];
    let shape = node.shape.unwrap_or(shape);
    let mut raw: Vec<(String, f64, bool)> = Vec::new();
    for part in node.title.split('\n') {
        for l in wrap(fonts, part, ts, true, wrap_w) {
            raw.push((l, ts, true));
        }
    }
    let ntitle = raw.len();
    for part in &node.body {
        for l in wrap(fonts, part, bs, false, wrap_w) {
            raw.push((l, bs, false));
        }
    }
    let title_gap = if raw.len() > ntitle { 0.25 * b } else { 0.0 };
    let mut lines = Vec::new();
    let mut top = 0.0;
    for (i, (text, size, bold)) in raw.into_iter().enumerate() {
        if i == ntitle {
            top += title_gap;
        }
        let runs = fonts.runs(&text, bold);
        let width = fonts.runs_width(&runs, size, bold);
        lines.push(TextLine {
            runs,
            size,
            bold,
            width,
            x: 0.0,
            baseline: top + 0.96 * size,
        });
        top += line_height(size);
    }
    let text_h = top;
    match shape {
        Shape::Rect => {
            let w = lines.iter().map(|l| l.width).fold(0.0, f64::max) + 2.0 * pad;
            for l in &mut lines {
                l.x = pad;
                l.baseline += pad;
            }
            Boxed {
                shape,
                role,
                w,
                h: text_h + 2.0 * pad,
                pad,
                lines,
            }
        }
        Shape::Circle => {
            // Radius from the farthest line corner, so text always fits inside the circle.
            let mut r: f64 = 0.0;
            for l in &lines {
                let top_y = l.baseline - 0.96 * l.size - text_h / 2.0;
                let far = top_y.abs().max((top_y + line_height(l.size)).abs());
                r = r.max((l.width / 2.0).hypot(far));
            }
            let r = r + pad;
            for l in &mut lines {
                l.x = r - l.width / 2.0;
                l.baseline += r - text_h / 2.0;
            }
            Boxed {
                shape,
                role,
                w: 2.0 * r,
                h: 2.0 * r,
                pad,
                lines,
            }
        }
    }
}

pub(super) fn measure_all(g: &Graph, t: &Tree, fonts: &Fonts, cfg: &Config, b: f64) -> Vec<Boxed> {
    let n = g.nodes.len();
    match g.settings.layout.unwrap_or(LayoutKind::Radial) {
        LayoutKind::Radial => (0..n)
            .map(|v| measure(fonts, g, v, radial::role(t, v), b, cfg))
            .collect(),
        LayoutKind::Layered(_) => (0..n)
            .map(|v| measure(fonts, g, v, Role::Leaf, b, cfg))
            .collect(),
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
}
