use crate::graph::KeyEntry;
use crate::metrics::Fonts;

pub struct Row {
    pub width: f64,
    pub items: Vec<(f64, usize)>,
}

pub struct KeyPlan {
    pub size: f64,
    pub row_h: f64,
    pub pad: f64,
    pub rows: Vec<Row>,
}

impl KeyPlan {
    /// Wraps entries into centered rows no wider than `avail`.
    pub fn new(
        entries: &[KeyEntry],
        fonts: &Fonts,
        avail: f64,
        pad: f64,
        size: f64,
    ) -> Option<KeyPlan> {
        if entries.is_empty() {
            return None;
        }
        let gap = 1.5 * size;
        let mut rows: Vec<Row> = Vec::new();
        for (i, e) in entries.iter().enumerate() {
            let w = size * 1.5 + fonts.width(&e.label, size, false);
            match rows.last_mut() {
                Some(r) if r.width + gap + w <= avail => {
                    r.items.push((r.width + gap, i));
                    r.width += gap + w;
                }
                _ => rows.push(Row {
                    width: w,
                    items: vec![(0.0, i)],
                }),
            }
        }
        Some(KeyPlan {
            size,
            row_h: 1.7 * size,
            pad,
            rows,
        })
    }

    /// Vertical space the key takes inside the page margin.
    pub fn height(&self) -> f64 {
        0.8 * self.size + self.rows.len() as f64 * self.row_h
    }

    pub fn width(&self) -> f64 {
        self.rows.iter().map(|r| r.width).fold(0.0, f64::max)
    }
}
