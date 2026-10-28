use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Script {
    Normal,
    Sup,
    Sub,
}

impl Script {
    pub fn scale(self) -> f64 {
        if self == Script::Normal { 1.0 } else { 0.7 }
    }

    /// shift is the baseline offset in ems, positive upward.
    pub fn shift(self) -> f64 {
        match self {
            Script::Normal => 0.0,
            Script::Sup => 0.35,
            Script::Sub => -0.15,
        }
    }
}

pub struct Font {
    pub family: String,
    pub bold: bool,
    pub path: String,
    data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Run {
    pub text: String,
    /// Index into the regular or bold fallback chain.
    pub font: usize,
    pub script: Script,
}

/// Fonts measures text with real advance widths so layout never needs a TeX round trip.
pub struct Fonts {
    pub regular: Vec<Font>,
    pub bold: Vec<Font>,
    cache: RefCell<HashMap<(bool, char), (f64, usize)>>,
    pub missing: RefCell<BTreeSet<char>>,
}

const REGULAR: &[&str] = &["DejaVu Serif:style=Book", "DejaVu Math TeX Gyre", "Noto Sans Math", "DejaVu Sans"];
const BOLD: &[&str] = &["DejaVu Serif:style=Bold", "DejaVu Sans:style=Bold", "DejaVu Math TeX Gyre", "Noto Sans Math"];

fn locate(pattern: &str) -> Option<Font> {
    let out = Command::new("fc-match").args(["-f", "%{file}\n%{family[0]}\n%{style[0]}", pattern]).output().ok()?;
    let s = String::from_utf8(out.stdout).ok()?;
    let mut it = s.lines();
    let path = it.next()?.to_string();
    let family = it.next()?.to_string();
    let style = it.next().unwrap_or("");
    let data = std::fs::read(&path).ok()?;
    ttf_parser::Face::parse(&data, 0).ok()?;
    Some(Font { family, bold: style.contains("Bold"), path, data })
}

fn chain(patterns: &[&str]) -> Vec<Font> {
    let mut out: Vec<Font> = Vec::new();
    for p in patterns {
        if let Some(f) = locate(p) {
            if !out.iter().any(|o| o.path == f.path) {
                out.push(f);
            }
        }
    }
    out
}

/// spans splits `^{...}` and `_{...}` markup from plain text.
pub fn spans(s: &str) -> Vec<(Script, String)> {
    let cs: Vec<char> = s.chars().collect();
    let (mut out, mut cur, mut i) = (Vec::new(), String::new(), 0);
    while i < cs.len() {
        let c = cs[i];
        if (c == '^' || c == '_') && cs.get(i + 1) == Some(&'{') {
            let (mut j, mut depth, mut inner) = (i + 2, 1, String::new());
            while j < cs.len() {
                if cs[j] == '{' {
                    depth += 1;
                } else if cs[j] == '}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                inner.push(cs[j]);
                j += 1;
            }
            if !cur.is_empty() {
                out.push((Script::Normal, std::mem::take(&mut cur)));
            }
            out.push((if c == '^' { Script::Sup } else { Script::Sub }, inner));
            i = j + 1;
            continue;
        }
        cur.push(c);
        i += 1;
    }
    if !cur.is_empty() {
        out.push((Script::Normal, cur));
    }
    out
}

impl Fonts {
    pub fn load() -> Result<Fonts, String> {
        let (regular, bold) = (chain(REGULAR), chain(BOLD));
        if regular.is_empty() || bold.is_empty() {
            return Err("no usable fonts found via fc-match (install ttf-dejavu)".into());
        }
        Ok(Fonts { regular, bold, cache: RefCell::new(HashMap::new()), missing: RefCell::new(BTreeSet::new()) })
    }

    pub fn font(&self, bold: bool, i: usize) -> &Font {
        if bold { &self.bold[i] } else { &self.regular[i] }
    }

    fn advance(&self, bold: bool, c: char) -> (f64, usize) {
        if let Some(&v) = self.cache.borrow().get(&(bold, c)) {
            return v;
        }
        let fonts = if bold { &self.bold } else { &self.regular };
        let mut found = None;
        for (i, f) in fonts.iter().enumerate() {
            let face = ttf_parser::Face::parse(&f.data, 0).expect("validated in locate");
            if let Some(adv) = face.glyph_index(c).and_then(|g| face.glyph_hor_advance(g)) {
                found = Some((adv as f64 / face.units_per_em() as f64, i));
                break;
            }
        }
        let v = found.unwrap_or_else(|| {
            self.missing.borrow_mut().insert(c);
            (0.6, 0)
        });
        self.cache.borrow_mut().insert((bold, c), v);
        v
    }

    /// runs splits text into same-font, same-script pieces for emitters.
    pub fn runs(&self, text: &str, bold: bool) -> Vec<Run> {
        let mut out: Vec<Run> = Vec::new();
        for (script, s) in spans(text) {
            for c in s.chars() {
                let font = self.advance(bold, c).1;
                match out.last_mut() {
                    Some(r) if r.font == font && r.script == script => r.text.push(c),
                    _ => out.push(Run { text: c.to_string(), font, script }),
                }
            }
        }
        out
    }

    pub fn runs_width(&self, runs: &[Run], size: f64, bold: bool) -> f64 {
        runs.iter()
            .map(|r| r.text.chars().map(|c| self.advance(bold, c).0).sum::<f64>() * size * r.script.scale())
            .sum()
    }

    pub fn width(&self, text: &str, size: f64, bold: bool) -> f64 {
        self.runs_width(&self.runs(text, bold), size, bold)
    }
}
