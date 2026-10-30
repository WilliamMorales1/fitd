use std::collections::{BTreeSet, HashMap};
use std::process::Command;
use std::sync::Mutex;

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
    /// Face index inside a font collection (`.ttc`).
    pub index: u32,
    pub data: Vec<u8>,
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
    // Mutexes, not RefCells, so layouts at several fonts can be measured in parallel.
    cache: Mutex<HashMap<(bool, char), (f64, usize)>>,
    pub missing: Mutex<BTreeSet<char>>,
}

const REGULAR: &[&str] = &["DejaVu Serif:style=Book", "DejaVu Math TeX Gyre", "Noto Sans Math", "DejaVu Sans"];
const BOLD: &[&str] = &["DejaVu Serif:style=Bold", "DejaVu Sans:style=Bold", "DejaVu Math TeX Gyre", "Noto Sans Math"];

fn locate(pattern: &str) -> Option<Font> {
    let out = Command::new("fc-match").args(["-f", "%{file}\n%{family[0]}\n%{style[0]}\n%{index}", pattern]).output().ok()?;
    let s = String::from_utf8(out.stdout).ok()?;
    let mut it = s.lines();
    let path = it.next()?.to_string();
    let family = it.next()?.to_string();
    let style = it.next().unwrap_or("").to_string();
    let index = it.next().and_then(|i| i.parse().ok()).unwrap_or(0);
    let data = std::fs::read(&path).ok()?;
    ttf_parser::Face::parse(&data, index).ok()?;
    Some(Font { family, bold: style.contains("Bold"), path, index, data })
}

fn chain(patterns: &[&str]) -> Vec<Font> {
    let mut out: Vec<Font> = Vec::new();
    for p in patterns {
        if let Some(f) = locate(p)
            && !out.iter().any(|o| (&o.path, o.index) == (&f.path, f.index))
        {
            out.push(f);
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
    /// load puts `family`, when given and installed, ahead of the default fallback chain.
    pub fn load(family: Option<&str>) -> Result<Fonts, String> {
        let mut fonts = Fonts { regular: Vec::new(), bold: Vec::new(), cache: Mutex::new(HashMap::new()), missing: Mutex::new(BTreeSet::new()) };
        if let Some(fam) = family {
            // fc-match always answers, so a substitute for a missing family must be rejected here.
            let wanted = |f: &Font| f.family.eq_ignore_ascii_case(fam);
            match locate(&format!("{fam}:weight=regular")).filter(wanted) {
                Some(f) => fonts.regular.push(f),
                None => return Err(format!("font family `{fam}` not found by fc-match (see `fc-list : family`)")),
            }
            fonts.bold.extend(locate(&format!("{fam}:weight=bold")).filter(wanted));
        }
        for (dst, patterns) in [(&mut fonts.regular, REGULAR), (&mut fonts.bold, BOLD)] {
            for f in chain(patterns) {
                if !dst.iter().any(|o| (&o.path, o.index) == (&f.path, f.index)) {
                    dst.push(f);
                }
            }
        }
        if fonts.regular.is_empty() || fonts.bold.is_empty() {
            return Err("no usable fonts found via fc-match (install ttf-dejavu)".into());
        }
        Ok(fonts)
    }

    pub fn font(&self, bold: bool, i: usize) -> &Font {
        if bold { &self.bold[i] } else { &self.regular[i] }
    }

    fn advance(&self, bold: bool, c: char) -> (f64, usize) {
        if let Some(&v) = self.cache.lock().unwrap().get(&(bold, c)) {
            return v;
        }
        let fonts = if bold { &self.bold } else { &self.regular };
        let mut found = None;
        for (i, f) in fonts.iter().enumerate() {
            let face = ttf_parser::Face::parse(&f.data, f.index).expect("validated in locate");
            if let Some(adv) = face.glyph_index(c).and_then(|g| face.glyph_hor_advance(g)) {
                found = Some((adv as f64 / face.units_per_em() as f64, i));
                break;
            }
        }
        let v = found.unwrap_or_else(|| {
            self.missing.lock().unwrap().insert(c);
            (0.6, 0)
        });
        self.cache.lock().unwrap().insert((bold, c), v);
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
