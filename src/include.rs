//! `include "file.fitd"`: splices other files in, line for line, before parsing. The include
//! line stays (the parser skips it), so diagnostics can point at it, and `Source` maps every
//! spliced line back to its own file.

use crate::ast::StmtKind;
use crate::diag::{Diag, Seg, Source, Span};
use std::path::{Path, PathBuf};

struct Splice {
    text: String,
    segs: Vec<Seg>,
    diags: Vec<Diag>,
}

impl Splice {
    fn seg(&mut self, path: &str, file_at: usize, file_line: usize) {
        let (at, line) = (self.text.len(), self.text.matches('\n').count() + 1);
        // A file that adds no lines leaves a segment that the next one replaces.
        if self.segs.last().is_some_and(|s| s.line == line) {
            self.segs.pop();
        }
        self.segs.push(Seg { line, at, file_at, path: path.to_string(), file_line });
    }
}

/// expand splices every include reachable from `text`, which was read from `path`. Include
/// paths are relative to the including file. A cycle or unreadable file is E0011 at its
/// include line, which is then left out.
pub fn expand(path: &str, text: &str) -> (Source, Vec<Diag>) {
    let mut out = Splice { text: String::new(), segs: Vec::new(), diags: Vec::new() };
    let mut stack = vec![canonical(Path::new(path))];
    splice(&mut out, path, text, &mut stack);
    (Source::spliced(path, &out.text, out.segs), out.diags)
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn splice(out: &mut Splice, path: &str, text: &str, stack: &mut Vec<PathBuf>) {
    let (stmts, _) = crate::parse::parse(text);
    out.seg(path, 0, 1);
    let mut cursor = 0;
    for st in stmts {
        let StmtKind::Include(target) = &st.kind else { continue };
        let end = text[st.span.hi..].find('\n').map_or(text.len(), |i| st.span.hi + i + 1);
        let base = out.text.len() - cursor;
        out.text += &text[cursor..end];
        if !out.text.ends_with('\n') {
            out.text.push('\n');
        }
        let span = Span::new(base + st.span.lo, base + st.span.hi);
        let dir = Path::new(path).parent().unwrap_or(Path::new(""));
        let file = dir.join(target);
        let key = canonical(&file);
        if let Some(k) = stack.iter().position(|p| *p == key) {
            let chain: Vec<String> = stack[k..].iter().chain([&key]).map(|p| p.display().to_string()).collect();
            let d = Diag::error("E0011", format!("`{target}` includes itself")).primary(span, "this include closes a cycle");
            out.diags.push(d.note(format!("cycle: {}", chain.join(" -> "))));
        } else {
            match std::fs::read_to_string(&file) {
                Ok(child) => {
                    let name = file.to_string_lossy().into_owned();
                    stack.push(key);
                    splice(out, &name, &child, stack);
                    stack.pop();
                    if !out.text.ends_with('\n') {
                        out.text.push('\n');
                    }
                }
                Err(e) => out.diags.push(Diag::error("E0011", format!("cannot read `{}`: {e}", file.display())).primary(span, "included here")),
            }
        }
        cursor = end;
        out.seg(path, end, text[..end].matches('\n').count() + 1);
    }
    out.text += &text[cursor..];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spliced_lines_map_back_to_their_files() {
        let dir = std::env::temp_dir().join(format!("fitd-include-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("parts")).unwrap();
        std::fs::write(dir.join("parts/a.fitd"), "node a\ninclude \"b.fitd\"\nnode a2\n").unwrap();
        std::fs::write(dir.join("parts/b.fitd"), "node b").unwrap();
        std::fs::write(dir.join("loop.fitd"), "include \"loop.fitd\"\n").unwrap();
        let main = dir.join("main.fitd");
        let text = "node m\ninclude \"parts/a.fitd\"\nnode m2\ninclude \"loop.fitd\"\n";
        std::fs::write(&main, text).unwrap();
        let (src, diags) = expand(main.to_str().unwrap(), text);
        assert_eq!(src.text, "node m\ninclude \"parts/a.fitd\"\nnode a\ninclude \"b.fitd\"\nnode b\nnode a2\nnode m2\ninclude \"loop.fitd\"\ninclude \"loop.fitd\"\n");
        let at = |line: usize| {
            let (p, n) = src.file_line(line);
            (Path::new(p).file_name().unwrap().to_str().unwrap().to_string(), n)
        };
        let want = [("main.fitd", 1), ("main.fitd", 2), ("a.fitd", 1), ("a.fitd", 2), ("b.fitd", 1), ("a.fitd", 3), ("main.fitd", 3), ("main.fitd", 4), ("loop.fitd", 1)];
        for (i, (f, n)) in want.iter().enumerate() {
            assert_eq!(at(i + 1), (f.to_string(), *n), "line {}", i + 1);
        }
        let off = src.text.find("node a2").unwrap();
        assert_eq!(src.file_offset(off).1, "node a\ninclude \"b.fitd\"\n".len());
        assert_eq!(diags.len(), 1);
        assert!(diags[0].code == "E0011" && diags[0].msg.contains("includes itself"), "{}", diags[0].msg);
        assert_eq!(at(src.line_col(diags[0].labels[0].span.lo).0), ("loop.fitd".to_string(), 1));
    }
}
