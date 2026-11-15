//! fmt: prints brace-syntax source in canonical layout.

/// HEADER lists the statements that lead a file, in their canonical order.
const HEADER: &[&str] = &["include", "title", "layout", "page", "font", "theme"];

/// WIDTH is the line length past which a `[...]` list wraps one item per line.
const WIDTH: usize = 100;

/// Scan counts a line's brackets: `lead` closers come before any opener, `net` is opened minus closed.
struct Scan {
    lead: usize,
    net: isize,
    /// braced is set by a `{` or `[`, which the indentation syntax never has outside strings.
    braced: bool,
}

/// code_end gives the byte offset where the `//` comment of line `t` starts, or its length.
pub fn code_end(t: &str) -> usize {
    let b = t.as_bytes();
    let (mut i, mut depth, mut in_str) = (0, 0usize, false);
    while i < b.len() {
        match b[i] {
            b'\\' if in_str => i += 1,
            b'"' => in_str = !in_str,
            b'$' if b.get(i + 1) == Some(&b'{') => {
                depth += 1;
                i += 1;
            }
            b'}' if depth > 0 && !in_str => depth -= 1,
            b'/' if !in_str && depth == 0 && b.get(i + 1) == Some(&b'/') => return i,
            _ => {}
        }
        i += 1;
    }
    b.len()
}

/// verbatim reports whether trimmed line `t` is a body line, whose text after `|` is content.
fn verbatim(t: &str) -> bool {
    t.starts_with('|')
}

fn scan(t: &str) -> Scan {
    if verbatim(t) || t.starts_with("//") {
        return Scan {
            lead: 0,
            net: 0,
            braced: false,
        };
    }
    let code = &t[..code_end(t)];
    let b = code.as_bytes();
    let (mut lead, mut net, mut opened, mut braced) = (0usize, 0isize, false, false);
    let (mut i, mut interp, mut in_str) = (0, 0usize, false);
    while i < b.len() {
        let c = b[i];
        match c {
            b'\\' if in_str => i += 1,
            b'"' if interp == 0 => in_str = !in_str,
            b'$' if b.get(i + 1) == Some(&b'{') => {
                interp += 1;
                i += 1;
            }
            b'}' if interp > 0 => interp -= 1,
            _ if in_str || interp > 0 => {}
            b'{' | b'[' | b'(' => {
                net += 1;
                opened = true;
                braced |= c != b'(';
            }
            b'}' | b']' | b')' => {
                net -= 1;
                if !opened {
                    lead += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    Scan { lead, net, braced }
}

/// spaced puts one space on each side of every `->` and `..>` outside strings.
fn spaced(t: &str) -> String {
    if verbatim(t) {
        return t.to_string();
    }
    let cut = code_end(t);
    let (code, note) = t.split_at(cut);
    let cs: Vec<char> = code.chars().collect();
    let mut o = String::new();
    let (mut i, mut in_str, mut interp) = (0, false, 0usize);
    while i < cs.len() {
        let c = cs[i];
        let arrow = if in_str || interp > 0 {
            0
        } else if c == '-' && cs.get(i + 1) == Some(&'>') {
            2
        } else if c == '.' && cs.get(i + 1) == Some(&'.') && cs.get(i + 2) == Some(&'>') {
            3
        } else {
            0
        };
        if arrow > 0 {
            while o.ends_with(' ') {
                o.pop();
            }
            if !o.is_empty() {
                o.push(' ');
            }
            o.extend(&cs[i..i + arrow]);
            i += arrow;
            while cs.get(i) == Some(&' ') {
                i += 1;
            }
            if i < cs.len() {
                o.push(' ');
            }
            continue;
        }
        match c {
            '\\' if in_str => {
                o.push(c);
                if let Some(&n) = cs.get(i + 1) {
                    o.push(n);
                    i += 1;
                }
            }
            '"' if interp == 0 => {
                in_str = !in_str;
                o.push(c);
            }
            '$' if cs.get(i + 1) == Some(&'{') => {
                interp += 1;
                o.push_str("${");
                i += 1;
            }
            '}' if interp > 0 => {
                interp -= 1;
                o.push(c);
            }
            _ => o.push(c),
        }
        i += 1;
    }
    if note.is_empty() {
        o.trim_end().to_string()
    } else {
        format!("{} {}", o.trim_end(), note.trim())
            .trim_start()
            .to_string()
    }
}

/// wrap splits a line longer than WIDTH at the commas of its outermost `[...]` list.
fn wrap(pad: &str, t: &str) -> Vec<String> {
    let one = format!("{pad}{t}");
    if one.chars().count() <= WIDTH || verbatim(t) || code_end(t) < t.len() {
        return vec![one];
    }
    let b = t.as_bytes();
    let (mut i, mut in_str, mut interp, mut depth) = (0, false, 0usize, 0usize);
    let (mut open, mut cuts, mut close) = (None, Vec::new(), None);
    while i < b.len() {
        match b[i] {
            b'\\' if in_str => i += 1,
            b'"' if interp == 0 => in_str = !in_str,
            b'$' if b.get(i + 1) == Some(&b'{') => {
                interp += 1;
                i += 1;
            }
            b'}' if interp > 0 => interp -= 1,
            _ if in_str || interp > 0 => {}
            b'[' | b'(' | b'{' => {
                if depth == 0 && b[i] == b'[' && open.is_none() {
                    open = Some(i);
                }
                depth += 1;
            }
            b']' | b')' | b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && open.is_some() && close.is_none() {
                    close = Some(i);
                }
            }
            b',' if depth == 1 && open.is_some() && close.is_none() => cuts.push(i),
            _ => {}
        }
        i += 1;
    }
    let (Some(lo), Some(hi)) = (open, close) else {
        return vec![one];
    };
    if cuts.is_empty() {
        return vec![one];
    }
    let mut out = vec![format!("{pad}{}", &t[..=lo])];
    let mut from = lo + 1;
    for &c in cuts.iter().chain([&hi]) {
        let item = t[from..c].trim();
        let comma = if c == hi { "" } else { "," };
        out.push(format!("{pad}  {item}{comma}"));
        from = c + 1;
    }
    out.push(format!("{pad}{}", &t[hi..]));
    out
}

/// old reports whether `text` looks like the indentation syntax, which `fitd fix` converts.
pub fn old(text: &str) -> bool {
    let mut depth = 0isize;
    for l in text.lines() {
        let t = l.trim();
        if t.is_empty() || t.starts_with("//") || verbatim(t) || t.starts_with('~') {
            continue;
        }
        if t == "end" || (depth == 0 && l.starts_with([' ', '\t'])) {
            return true;
        }
        depth += scan(t).net;
    }
    false
}

/// braced reports whether `text` already uses braces or `[...]` outside strings, so `fitd fix` must leave it alone.
pub fn braced(text: &str) -> bool {
    text.lines().any(|l| scan(l.trim()).braced)
}

/// head gives the leading statement word of trimmed line `t`.
fn head(t: &str) -> &str {
    t.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .next()
        .unwrap_or("")
}

/// fmt reindents brace-syntax `text` two spaces per level, spaces arrows, moves the header to the top, and wraps long lists.
pub fn fmt(text: &str) -> String {
    let mut lines: Vec<(usize, String)> = Vec::new();
    let mut depth = 0usize;
    for l in join(text) {
        let t = l.trim();
        if t.is_empty() {
            if lines.last().is_some_and(|(_, s)| !s.is_empty()) {
                lines.push((0, String::new()));
            }
            continue;
        }
        let s = scan(t);
        let at = depth.saturating_sub(s.lead);
        if s.lead > 0 && lines.last().is_some_and(|(_, s)| s.is_empty()) {
            lines.pop();
        }
        lines.push((at, spaced(t)));
        depth = (depth as isize + s.net).max(0) as usize;
    }
    let mut k = 0;
    // A blank line right after an opener only pads the block.
    while k + 1 < lines.len() {
        let opener = lines[k].1.ends_with('{') && lines[k + 1].1.is_empty();
        if opener {
            lines.remove(k + 1);
        } else {
            k += 1;
        }
    }
    while lines.last().is_some_and(|(_, s)| s.is_empty()) {
        lines.pop();
    }
    let lines = hoist(lines);
    let mut o = String::new();
    for (d, s) in lines {
        if s.is_empty() {
            o.push('\n');
            continue;
        }
        for w in wrap(&"  ".repeat(d), &s) {
            o += &w;
            o.push('\n');
        }
    }
    o
}

/// join rejoins `[...]` and `(...)` lists that span lines, so wrapping starts fresh and the header moves whole.
fn join(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut open = 0isize;
    for l in text.lines() {
        let t = l.trim();
        if open > 0 {
            if t.is_empty() || t.starts_with("//") || code_end(t) < t.len() || t.contains('{') {
                open = 0;
                out.push(l.to_string());
                continue;
            }
            let last = out.last_mut().unwrap();
            if !(last.ends_with('[') || last.ends_with('(') || t.starts_with([']', ')'])) {
                last.push(' ');
            }
            last.push_str(t);
            open += scan(t).net;
            continue;
        }
        out.push(l.to_string());
        if t.ends_with('[') || t.ends_with('(') {
            open = scan(t).net;
        }
    }
    out
}

/// hoist moves top-level header statements, with the comments directly above them, under the file's opening comments.
fn hoist(lines: Vec<(usize, String)>) -> Vec<(usize, String)> {
    let rank = |s: &str| {
        HEADER
            .iter()
            .position(|h| head(s) == *h && s.len() > h.len())
    };
    let lead = lines
        .iter()
        .position(|(_, s)| !s.starts_with("//"))
        .unwrap_or(lines.len());
    let (top, rest) = lines.split_at(lead);
    let mut header: Vec<(usize, Vec<(usize, String)>)> = Vec::new();
    let mut body: Vec<(usize, String)> = Vec::new();
    for (d, s) in rest {
        match rank(s) {
            Some(r) if *d == 0 => {
                let mut notes = Vec::new();
                while body
                    .last()
                    .is_some_and(|(d, s)| *d == 0 && s.starts_with("//"))
                {
                    notes.insert(0, body.pop().unwrap());
                }
                notes.push((0, s.clone()));
                header.push((r, notes));
            }
            _ => body.push((*d, s.clone())),
        }
    }
    header.sort_by_key(|(r, _)| *r);
    let mut out: Vec<(usize, String)> = top.to_vec();
    out.extend(header.into_iter().flat_map(|(_, v)| v));
    let header_end = out.len();
    let mut skip = true;
    for l in body {
        if skip && l.1.is_empty() {
            continue;
        }
        skip = false;
        if l.1.is_empty() && out.last().is_some_and(|(_, s)| s.is_empty()) {
            continue;
        }
        out.push(l);
    }
    if header_end > lead && header_end < out.len() {
        out.insert(header_end, (0, String::new()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reindents_by_braces() {
        let src = "a \"A\" {\n| body  text\n      b->c\n}\n";
        assert_eq!(fmt(src), "a \"A\" {\n  | body  text\n  b -> c\n}\n");
    }

    #[test]
    fn else_closes_and_opens() {
        let src = "if x {\na\n} else {\nb\n}\n";
        assert_eq!(fmt(src), "if x {\n  a\n} else {\n  b\n}\n");
    }

    #[test]
    fn header_moves_up_in_order() {
        let src = "// About.\npage 4:3\n\na -> b\n// why\nlayout layered\ninclude \"flow\"\n";
        assert_eq!(
            fmt(src),
            "// About.\ninclude \"flow\"\n// why\nlayout layered\npage 4:3\n\na -> b\n"
        );
    }

    #[test]
    fn long_lists_wrap() {
        let item = "\"a fairly long cell label\"";
        let src = format!("~ [{item}, {item}, {item}, {item}]\n");
        let out = fmt(&src);
        assert_eq!(out.lines().count(), 6, "{out}");
        assert!(out.starts_with("~ [\n  \"a"));
        assert!(out.ends_with("label\"\n]\n"));
        assert_eq!(fmt(&out), out);
        assert_eq!(
            fmt(&format!("page 4:3\n{out}")),
            format!("page 4:3\n\n{out}")
        );
    }

    #[test]
    fn strings_and_comments_hold_no_structure() {
        let src = "a \"{ -> [\" { // }\nb\n}\n";
        assert_eq!(fmt(src), "a \"{ -> [\" { // }\n  b\n}\n");
    }

    #[test]
    fn detects_old_syntax() {
        assert!(old("a\n  b\n"));
        assert!(old("for i in 1..2\nn${i}\nend\n"));
        assert!(!old("a {\n  b\n}\n| x\n"));
        assert!(braced("a -> b [line=dashed]\n"));
        assert!(!braced("def t(a)\n${a} \"[x]\"\nend\n"));
    }
}
