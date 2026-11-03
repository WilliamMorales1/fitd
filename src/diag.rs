//! Diagnostics with source spans, rendered in the style of rustc.

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Span {
    pub lo: usize,
    pub hi: usize,
}

impl Span {
    pub fn new(lo: usize, hi: usize) -> Span {
        Span { lo, hi }
    }

    pub fn to(self, o: Span) -> Span {
        Span {
            lo: self.lo.min(o.lo),
            hi: self.hi.max(o.hi),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Error,
    Warning,
    Note,
}

pub struct Label {
    pub span: Span,
    pub msg: String,
    pub primary: bool,
}

pub struct Diag {
    pub level: Level,
    pub code: &'static str,
    pub msg: String,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
    pub help: Option<String>,
}

impl Diag {
    pub fn error(code: &'static str, msg: impl Into<String>) -> Diag {
        Diag {
            level: Level::Error,
            code,
            msg: msg.into(),
            labels: Vec::new(),
            notes: Vec::new(),
            help: None,
        }
    }

    pub fn warning(code: &'static str, msg: impl Into<String>) -> Diag {
        Diag {
            level: Level::Warning,
            ..Diag::error(code, msg)
        }
    }

    pub fn info(msg: impl Into<String>) -> Diag {
        Diag {
            level: Level::Note,
            ..Diag::error("", msg)
        }
    }

    pub fn primary(mut self, span: Span, msg: impl Into<String>) -> Diag {
        self.labels.push(Label {
            span,
            msg: msg.into(),
            primary: true,
        });
        self
    }

    pub fn secondary(mut self, span: Span, msg: impl Into<String>) -> Diag {
        self.labels.push(Label {
            span,
            msg: msg.into(),
            primary: false,
        });
        self
    }

    pub fn note(mut self, msg: impl Into<String>) -> Diag {
        self.notes.push(msg.into());
        self
    }

    pub fn help(mut self, msg: impl Into<String>) -> Diag {
        self.help = Some(msg.into());
        self
    }
}

/// Source maps byte offsets to lines and columns, and lines back to the files that were
/// spliced together by `include`.
pub struct Source {
    pub path: String,
    pub text: String,
    starts: Vec<usize>,
    segs: Vec<Seg>,
}

/// Seg is a run of whole lines copied from one file, from `line` of the spliced text on.
#[derive(Clone, Debug)]
pub struct Seg {
    pub line: usize,
    pub path: String,
    pub file_line: usize,
}

impl Source {
    pub fn new(path: &str, text: &str) -> Source {
        Source::spliced(
            path,
            text,
            vec![Seg {
                line: 1,
                path: path.to_string(),
                file_line: 1,
            }],
        )
    }

    pub fn spliced(path: &str, text: &str, segs: Vec<Seg>) -> Source {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Source {
            path: path.to_string(),
            text: text.to_string(),
            starts,
            segs,
        }
    }

    fn seg(&self, line: usize) -> &Seg {
        &self.segs[self.segs.partition_point(|s| s.line <= line).max(1) - 1]
    }

    /// file_line maps a line of the spliced text to its file and its line there.
    pub fn file_line(&self, line: usize) -> (&str, usize) {
        let s = self.seg(line);
        (&s.path, s.file_line + line - s.line)
    }

    /// line_col is 1-based; the column counts characters, not bytes.
    pub fn line_col(&self, off: usize) -> (usize, usize) {
        let line = self.starts.partition_point(|&s| s <= off) - 1;
        let col = self.text[self.starts[line]..off.min(self.text.len())]
            .chars()
            .count();
        (line + 1, col + 1)
    }

    pub fn line_text(&self, line: usize) -> &str {
        let lo = self.starts[line - 1];
        let hi = self.starts.get(line).copied().unwrap_or(self.text.len());
        self.text[lo..hi].trim_end_matches(['\n', '\r'])
    }
}

impl Diag {
    /// pos orders diagnostics by where they point, so output reads top to bottom.
    pub fn pos(&self) -> usize {
        self.labels
            .iter()
            .find(|l| l.primary)
            .or(self.labels.first())
            .map_or(usize::MAX, |l| l.span.lo)
    }
}

/// mark_rows draws one line's marks. Marks that do not overlap share a row, and messages hang
/// below on connectors, as rustc does; overlapping marks each get their own row.
fn mark_rows(pad: &str, marks: &[(usize, String, &str)]) -> String {
    let mut s = String::new();
    let overlap = marks
        .windows(2)
        .any(|w| w[0].0 + w[0].1.chars().count() > w[1].0);
    if overlap {
        for (col, mark, msg) in marks {
            let msg = if msg.is_empty() {
                String::new()
            } else {
                format!(" {msg}")
            };
            s += &format!("{pad} | {}{mark}{msg}\n", " ".repeat(*col));
        }
        return s;
    }
    let mut row = String::new();
    for (col, mark, _) in marks {
        row += &" ".repeat(col - row.chars().count());
        row += mark;
    }
    let mut hang: Vec<(usize, &str)> = marks
        .iter()
        .filter(|m| !m.2.is_empty())
        .map(|m| (m.0, m.2))
        .collect();
    // The rightmost message fits on the mark row itself.
    if let Some(&(col, msg)) = hang.last()
        && marks.last().is_some_and(|m| m.0 == col)
    {
        row += &format!(" {msg}");
        hang.pop();
    }
    s += &format!("{pad} | {row}\n");
    while let Some((col, msg)) = hang.pop() {
        let bars = |upto: usize| {
            let mut r = String::new();
            for &(c, _) in hang.iter().filter(|h| h.0 < upto) {
                r += &" ".repeat(c - r.chars().count());
                r += "|";
            }
            r
        };
        let mut conn = bars(col);
        conn += &" ".repeat(col - conn.chars().count());
        s += &format!("{pad} | {conn}|\n");
        let mut text = bars(col);
        text += &" ".repeat(col - text.chars().count());
        s += &format!("{pad} | {text}{msg}\n");
    }
    s
}

pub fn render(src: &Source, diags: &[Diag]) -> String {
    let mut s = String::new();
    for d in diags {
        match d.level {
            Level::Error => s += &format!("error[{}]: {}\n", d.code, d.msg),
            Level::Warning => s += &format!("warning[{}]: {}\n", d.code, d.msg),
            Level::Note => s += &format!("note: {}\n", d.msg),
        }
        let mut labels: Vec<&Label> = d.labels.iter().collect();
        labels.sort_by_key(|l| (src.line_col(l.span.lo).0, !l.primary));
        let width = labels
            .iter()
            .map(|l| src.file_line(src.line_col(l.span.lo).0).1.to_string().len())
            .max()
            .unwrap_or(1);
        let pad = " ".repeat(width);
        let mut file = "";
        if let Some(l) = labels.iter().find(|l| l.primary).or(labels.first()) {
            let (line, col) = src.line_col(l.span.lo);
            let (path, fline) = src.file_line(line);
            s += &format!("{pad}--> {path}:{fline}:{col}\n{pad} |\n");
            file = path;
        }
        let mut last = 0;
        for (i, l) in labels.iter().enumerate() {
            let line = src.line_col(l.span.lo).0;
            if line == last {
                continue;
            }
            let (path, fline) = src.file_line(line);
            // A line from another file gets its own header, as rustc does.
            if path != file {
                s += &format!("{pad}::: {path}:{fline}:{}\n", src.line_col(l.span.lo).1);
                file = path;
            } else if last != 0 && line > last + 1 {
                s += &format!("{pad} ...\n");
            }
            let text = src.line_text(line);
            s += &format!("{fline:>width$} | {text}\n");
            last = line;
            // (start column, mark, message) for every label on this line, left to right.
            let mut marks: Vec<(usize, String, &str)> = labels[i..]
                .iter()
                .take_while(|l| src.line_col(l.span.lo).0 == line)
                .map(|l| {
                    let col = src.line_col(l.span.lo).1 - 1;
                    let end = if src.line_col(l.span.hi).0 == line {
                        src.line_col(l.span.hi).1 - 1
                    } else {
                        text.chars().count()
                    };
                    let n = end.saturating_sub(col).max(1);
                    (
                        col,
                        (if l.primary { "^" } else { "-" }).repeat(n),
                        l.msg.as_str(),
                    )
                })
                .collect();
            marks.sort_by_key(|m| m.0);
            s += &mark_rows(&pad, &marks);
        }
        if !labels.is_empty() {
            s += &format!("{pad} |\n");
        }
        for n in &d.notes {
            s += &format!("{pad} = note: {n}\n");
        }
        if let Some(h) = &d.help {
            s += &format!("{pad} = help: {h}\n");
        }
        s += "\n";
    }
    s
}

/// render_short gives each error or warning one line, `path:line:col: level[CODE]: message`,
/// for agents and editors. Notes are left out; other lines a diagnostic points at are listed.
pub fn render_short(src: &Source, diags: &[Diag]) -> String {
    let mut s = String::new();
    for d in diags.iter().filter(|d| d.level != Level::Note) {
        let level = if d.level == Level::Error {
            "error"
        } else {
            "warning"
        };
        let (line, col) = d
            .labels
            .iter()
            .find(|l| l.primary)
            .or(d.labels.first())
            .map_or((0, 0), |l| src.line_col(l.span.lo));
        let (path, fline) = if line == 0 {
            (src.path.as_str(), 0)
        } else {
            src.file_line(line)
        };
        s += &format!("{path}:{fline}:{col}: {level}[{}]: {}", d.code, d.msg);
        let mut also: Vec<usize> = d
            .labels
            .iter()
            .map(|l| src.line_col(l.span.lo).0)
            .filter(|&l| l != line)
            .collect();
        also.dedup();
        if !also.is_empty() {
            let at = |l: usize| match src.file_line(l) {
                (p, n) if p == path => n.to_string(),
                (p, n) => format!("{p}:{n}"),
            };
            s += &format!(
                "; also line {}",
                also.iter().map(|&l| at(l)).collect::<Vec<_>>().join(", ")
            );
        }
        if let Some(h) = &d.help {
            s += &format!("; {h}");
        }
        s += "\n";
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_caret_under_span() {
        let src = Source::new("t.fitd", "node a\na -> bb\n");
        let d =
            Diag::error("E0001", "unknown node `bb`").primary(Span::new(12, 14), "not declared");
        let out = render(&src, &[d]);
        assert!(out.contains(" --> t.fitd:2:6"), "{out}");
        assert!(
            out.contains("2 | a -> bb\n  |      ^^ not declared"),
            "{out}"
        );
    }

    #[test]
    fn unlabeled_marks_share_a_row() {
        let src = Source::new("t.fitd", "a x b y c\n");
        let d = Diag::error("E0008", "m")
            .secondary(Span::new(2, 3), "")
            .secondary(Span::new(6, 7), "");
        let out = render(&src, &[d]);
        assert!(out.contains("1 | a x b y c\n  |   -   -\n"), "{out}");
    }

    #[test]
    fn left_message_hangs_on_a_connector() {
        let src = Source::new("t.fitd", "a x b y c\n");
        let d = Diag::error("E0008", "m")
            .primary(Span::new(2, 3), "here")
            .secondary(Span::new(6, 7), "");
        let out = render(&src, &[d]);
        assert!(out.contains("  |   ^   -\n  |   |\n  |   here\n"), "{out}");
    }

    #[test]
    fn short_form_is_one_line_per_diagnostic() {
        let src = Source::new("t.fitd", "node a\na -> bb\nrequire a above bb\n");
        let d = Diag::error("E0110", "m")
            .primary(Span::new(7, 8), "")
            .secondary(Span::new(15, 33), "")
            .help("h");
        let out = render_short(&src, &[d, Diag::info("skipped")]);
        assert_eq!(out, "t.fitd:2:1: error[E0110]: m; also line 3; h\n");
    }
}
