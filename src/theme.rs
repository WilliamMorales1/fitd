//! Colors the emitters draw with. A preset supplies every value; a `theme` statement may
//! override any of them.

pub const PRESETS: &[&str] = &["light", "dark", "mono"];

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub background: String,
    pub text: String,
    /// Directed arrows of layered diagrams.
    pub edge: String,
    /// Dashed cross links.
    pub link: String,
    /// Edge label text.
    pub label: String,
    /// Fill of the radial root circle.
    pub root: String,
    /// Branch colors, assigned to the root's children in order.
    pub palette: Vec<String>,
    /// Opacity of a box's accent-color tint over the background.
    pub tint: f64,
}

/// Overrides holds the theme values written in the file; unset ones keep the preset's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overrides {
    pub background: Option<String>,
    pub text: Option<String>,
    pub edge: Option<String>,
    pub link: Option<String>,
    pub label: Option<String>,
    pub root: Option<String>,
    pub palette: Option<Vec<String>>,
}

pub const KEYS: &[&str] = &[
    "background",
    "text",
    "edge",
    "link",
    "label",
    "root",
    "palette",
];

fn strs(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

impl Theme {
    pub fn preset(name: &str) -> Option<Theme> {
        let (bg, text, edge, link, label, root, palette, tint) = match name {
            "light" => (
                "#FFFFFF",
                "#1F2328",
                "#57606A",
                "#6E7781",
                "#57606A",
                "#F6F8FA",
                &[
                    "#9A6700", "#1A7F37", "#0969DA", "#8250DF", "#CF222E", "#1F883D", "#BC4C00",
                    "#57606A",
                ],
                0.08,
            ),
            "dark" => (
                "#0D1117",
                "#E6EDF3",
                "#8B949E",
                "#6E7681",
                "#8B949E",
                "#21262D",
                &[
                    "#D29922", "#3FB950", "#4493F8", "#AB7DF8", "#F85149", "#56D364", "#DB6D28",
                    "#8B949E",
                ],
                0.16,
            ),
            "mono" => (
                "#FFFFFF",
                "#000000",
                "#444444",
                "#666666",
                "#444444",
                "#E0E0E0",
                &[
                    "#202020", "#404040", "#5A5A5A", "#303030", "#4A4A4A", "#606060", "#383838",
                    "#505050",
                ],
                0.06,
            ),
            _ => return None,
        };
        Some(Theme {
            background: bg.into(),
            text: text.into(),
            edge: edge.into(),
            link: link.into(),
            label: label.into(),
            root: root.into(),
            palette: strs(palette),
            tint,
        })
    }

    pub fn with(mut self, o: &Overrides) -> Theme {
        let set = |dst: &mut String, src: &Option<String>| {
            if let Some(c) = src {
                *dst = c.clone();
            }
        };
        set(&mut self.background, &o.background);
        set(&mut self.text, &o.text);
        set(&mut self.edge, &o.edge);
        set(&mut self.link, &o.link);
        set(&mut self.label, &o.label);
        set(&mut self.root, &o.root);
        if let Some(p) = &o.palette {
            self.palette = p.clone();
        }
        self
    }
}

/// is_color accepts `#RGB` and `#RRGGBB`.
pub fn is_color(s: &str) -> bool {
    s.strip_prefix('#')
        .is_some_and(|h| (h.len() == 3 || h.len() == 6) && h.chars().all(|c| c.is_ascii_hexdigit()))
}

/// rgb expands a color accepted by `is_color` to six uppercase hex digits.
pub fn rgb(c: &str) -> String {
    let h = c.trim_start_matches('#');
    let h = if h.len() == 3 {
        h.chars().flat_map(|c| [c, c]).collect()
    } else {
        h.to_string()
    };
    h.to_uppercase()
}

fn luminance(c: &str) -> f64 {
    let h = rgb(c);
    let ch = |i: usize| {
        let v = u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f64 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(0) + 0.7152 * ch(2) + 0.0722 * ch(4)
}

/// on picks near-black or white text, whichever contrasts more with `fill` (WCAG ratio).
pub fn on(fill: &str) -> &'static str {
    let l = luminance(fill);
    if (l + 0.05) / 0.05 >= 1.05 / (l + 0.05) {
        "#111111"
    } else {
        "#FFFFFF"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_and_overrides() {
        for p in PRESETS {
            let t = Theme::preset(p).unwrap();
            assert!(
                [&t.background, &t.text, &t.edge, &t.link, &t.label, &t.root]
                    .iter()
                    .all(|c| is_color(c))
            );
            assert!(t.palette.iter().all(|c| is_color(c)));
        }
        let o = Overrides {
            background: Some("#000".into()),
            palette: Some(vec!["#f00".into()]),
            ..Default::default()
        };
        let t = Theme::preset("light").unwrap().with(&o);
        assert_eq!(
            (t.background.as_str(), t.text.as_str(), t.palette.len()),
            ("#000", "#1F2328", 1)
        );
    }

    #[test]
    fn contrast_text() {
        assert_eq!(on("#0969DA"), "#FFFFFF");
        assert_eq!(on("#FFB74D"), "#111111");
        assert_eq!(rgb("#abc"), "AABBCC");
        assert!(!is_color("#12345") && !is_color("red"));
    }
}
