use super::*;

pub struct Anchor {
    pub body: f64,
    pub page: (f64, f64),
    pub rot: f64,
    pub pos: Vec<Option<P>>,
}

const MOVE_COST: f64 = 40.0;
const MOVE_PER_EM: f64 = 2.0;

impl Anchor {
    pub fn parse(g: &Graph, text: &str) -> Result<Anchor, String> {
        let mut a = Anchor {
            body: 0.0,
            page: (0.0, 0.0),
            rot: 0.0,
            pos: vec![None; g.nodes.len()],
        };
        let num = |s: Option<&str>| {
            s.and_then(|s| s.parse::<f64>().ok())
                .filter(|x| x.is_finite())
        };
        for (i, line) in text.lines().enumerate() {
            let mut w = line.split_whitespace();
            let bad = || format!("line {}: cannot read `{line}`", i + 1);
            match w.next() {
                None => {}
                Some(s) if s.starts_with('#') => {}
                Some("font") => a.body = num(w.next()).ok_or_else(bad)?,
                Some("page") => {
                    a.page = (
                        num(w.next()).ok_or_else(bad)?,
                        num(w.next()).ok_or_else(bad)?,
                    )
                }
                Some("rot") => a.rot = num(w.next()).ok_or_else(bad)?,
                Some("node") => {
                    let id = w.next().ok_or_else(bad)?;
                    let p = (
                        num(w.next()).ok_or_else(bad)?,
                        num(w.next()).ok_or_else(bad)?,
                    );
                    if let Some(v) = g.nodes.iter().position(|n| n.id == id) {
                        a.pos[v] = Some(p);
                    }
                }
                Some(_) => return Err(bad()),
            }
        }
        if a.body <= 0.0 || a.page.0 <= 0.0 {
            return Err("missing `font` or `page` line".into());
        }
        Ok(a)
    }

    pub fn render(g: &Graph, lay: &Layout, cfg: &Config) -> String {
        let mut s = String::from(
            "# fitd lock: node centers in pt from the page center, y up. Delete to re-layout freely.\n",
        );
        s += &format!(
            "font {}\npage {} {}\nrot {}\n",
            lay.body, cfg.page_w, cfg.page_h, lay.rot
        );
        for (v, n) in g.nodes.iter().enumerate() {
            s += &format!("node {} {} {}\n", n.id, lay.pos[v].0, lay.pos[v].1);
        }
        s
    }

    pub(super) fn cost(&self, v: usize, at: P, body: f64) -> f64 {
        match self.pos[v] {
            Some(q) if (at.0 - q.0).hypot(at.1 - q.1) > 0.01 => {
                MOVE_COST + MOVE_PER_EM * (at.0 - q.0).hypot(at.1 - q.1) / body
            }
            _ => 0.0,
        }
    }

    pub fn drift(&self, lay: &Layout) -> f64 {
        (0..lay.pos.len())
            .map(|v| self.cost(v, lay.pos[v], lay.body))
            .sum()
    }
}
