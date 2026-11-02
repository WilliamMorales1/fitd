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
    /// Things worth telling the user about how fonts were chosen.
    pub notes: Vec<String>,
}

/// EMBEDDED is the default font set, compiled in so the layout does not depend on which fonts
/// (or whether fontconfig) are installed.
#[cfg(feature = "embedded-fonts")]
const EMBEDDED: [(&str, &str, &[u8]); 4] = [
    ("DejaVu Serif", "DejaVuSerif.ttf", include_bytes!("../fonts/DejaVuSerif.ttf")),
    ("DejaVu Serif", "DejaVuSerif-Bold.ttf", include_bytes!("../fonts/DejaVuSerif-Bold.ttf")),
    // Bold symbols the serif lacks (such as ≇) read better in bold sans than in regular math.
    ("DejaVu Sans", "DejaVuSans-Bold.ttf", include_bytes!("../fonts/DejaVuSans-Bold.ttf")),
    ("DejaVu Math TeX Gyre", "DejaVuMathTeXGyre.ttf", include_bytes!("../fonts/DejaVuMathTeXGyre.ttf")),
];

/// fallbacks is the chain behind any named family, as indices into EMBEDDED.
#[cfg(feature = "embedded-fonts")]
fn fallbacks(bold: bool) -> Vec<Font> {
    let chain: &[usize] = if bold { &[1, 2, 3] } else { &[0, 3] };
    chain.iter().map(|&i| {
        let (family, file, data) = EMBEDDED[i];
        Font { family: family.into(), bold, path: format!("embedded:{file}"), index: 0, data: data.to_vec() }
    }).collect()
}

#[cfg(not(feature = "embedded-fonts"))]
fn fallbacks(bold: bool) -> Vec<Font> {
    chain(if bold { BOLD } else { REGULAR })
}

fn fontconfig() -> bool {
    Command::new("fc-match").arg("--version").output().is_ok_and(|o| o.status.success())
}

/// fnv1a hashes font files for the SVG fingerprint; stable across builds and platforms.
fn fnv1a(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

#[cfg(not(feature = "embedded-fonts"))]
const REGULAR: &[&str] = &["DejaVu Serif:style=Book", "DejaVu Math TeX Gyre", "Noto Sans Math", "DejaVu Sans"];
#[cfg(not(feature = "embedded-fonts"))]
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

#[cfg(not(feature = "embedded-fonts"))]
fn chain(patterns: &[&str]) -> Vec<Font> {
    // Each lookup spawns fc-match, so they run at once.
    let found: Vec<Option<Font>> = std::thread::scope(|s| {
        let handles: Vec<_> = patterns.iter().map(|p| s.spawn(move || locate(p))).collect();
        handles.into_iter().map(|h| h.join().expect("font lookup thread")).collect()
    });
    let mut out: Vec<Font> = Vec::new();
    for f in found.into_iter().flatten() {
        if !out.iter().any(|o| (&o.path, o.index) == (&f.path, f.index)) {
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
        let mut fonts = Fonts { regular: Vec::new(), bold: Vec::new(), cache: Mutex::new(HashMap::new()), missing: Mutex::new(BTreeSet::new()), notes: Vec::new() };
        match family {
            Some(fam) if cfg!(feature = "embedded-fonts") && !fontconfig() => {
                fonts.notes.push(format!("font family `{fam}` ignored: fc-match is not installed, so only the built-in DejaVu Serif is available"));
            }
            Some(fam) => {
                // fc-match always answers, so a substitute for a missing family must be rejected here.
                let wanted = |f: &Font| f.family.eq_ignore_ascii_case(fam);
                match locate(&format!("{fam}:weight=regular")).filter(wanted) {
                    Some(f) => fonts.regular.push(f),
                    None => return Err(format!("font family `{fam}` not found by fc-match (see `fc-list : family`)")),
                }
                fonts.bold.extend(locate(&format!("{fam}:weight=bold")).filter(wanted));
            }
            None => {}
        }
        let (regular, bold) = std::thread::scope(|s| {
            let bold = s.spawn(|| fallbacks(true));
            (fallbacks(false), bold.join().expect("font lookup thread"))
        });
        for (dst, found) in [(&mut fonts.regular, regular), (&mut fonts.bold, bold)] {
            for f in found {
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

    /// fingerprint names each font file in use with a hash of its bytes, so a layout difference
    /// between machines can be traced to fonts.
    pub fn fingerprint(&self) -> String {
        let mut seen: Vec<(u64, u32)> = Vec::new();
        let mut out = Vec::new();
        for f in self.regular.iter().chain(&self.bold) {
            let hash = fnv1a(&f.data);
            if !seen.contains(&(hash, f.index)) {
                seen.push((hash, f.index));
                let file = f.path.rsplit('/').next().unwrap_or(&f.path);
                out.push(format!("{} ({file}) {hash:016x}", f.family));
            }
        }
        out.join(", ")
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
