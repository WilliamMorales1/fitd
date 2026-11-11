//! Request handlers: hover, navigation, completion, and editing aids.

use super::docs;
use super::index::{self, Files, Hit, Index, Kind, Loc, Word, slice16};
use super::{offset, uri_of};
use serde_json::{Value, json};

/// Doc is the buffer a request is about.
pub struct Doc<'a> {
    pub uri: &'a str,
    pub path: &'a str,
    pub text: &'a str,
    pub open: &'a dyn Fn(&str) -> Option<String>,
}

impl Doc<'_> {
    pub fn index(&self) -> Index {
        let mut files = Files::new(self.open);
        index::build(self.path, self.text, &mut files)
    }

    fn uri(&self, path: &str) -> Value {
        if path == self.path {
            json!(self.uri)
        } else if crate::stdlib::get(path).is_some() {
            std_uri(path).map_or(Value::Null, Value::from)
        } else {
            json!(uri_of(path))
        }
    }

    fn location(&self, loc: &Loc) -> Option<Value> {
        let uri = self.uri(&loc.path);
        (!uri.is_null()).then(|| json!({ "uri": uri, "range": loc.range() }))
    }
}

/// std_uri writes a library module to a cache file, so editors can open it.
fn std_uri(target: &str) -> Option<String> {
    let text = crate::stdlib::get(target)?;
    let dir = std::env::temp_dir().join(format!("fitd-std-{}", env!("CARGO_PKG_VERSION")));
    let file = dir.join(target.strip_prefix("std/")?);
    if std::fs::read_to_string(&file).ok().as_deref() != Some(text) {
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(&file, text).ok()?;
    }
    Some(uri_of(file.to_str()?))
}

fn pos(p: &Value) -> (usize, usize) {
    (
        p["line"].as_u64().unwrap_or(0) as usize,
        p["character"].as_u64().unwrap_or(0) as usize,
    )
}

fn line_of(text: &str, n: usize) -> &str {
    text.split('\n').nth(n).unwrap_or("").trim_end_matches('\r')
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

fn md(value: String) -> Value {
    json!({ "kind": "markdown", "value": value })
}

fn fence(code: &str) -> String {
    format!("```fitd\n{code}\n```")
}

fn signature(d: &index::Def) -> String {
    format!("{}({})", d.name, d.params.join(", "))
}

fn values_line(a: &docs::Attr) -> String {
    if a.values.is_empty() || a.values[0].starts_with(|c: char| c.is_ascii_digit()) {
        return String::new();
    }
    let vs: Vec<String> = a.values.iter().map(|v| format!("`{v}`")).collect();
    format!("\n\nValues: {}", vs.join(", "))
}

fn module_doc(target: &str) -> Option<String> {
    let text = crate::stdlib::get(target)?;
    let head: Vec<&str> = text
        .lines()
        .take_while(|l| l.starts_with('#'))
        .map(|l| l.trim_start_matches('#').trim())
        .collect();
    let mut defs = Vec::new();
    let mut styles = Vec::new();
    for l in text.lines() {
        if let Some(r) = l.strip_prefix("def ") {
            defs.push(format!("`{}`", r.trim()));
        } else if let Some(r) = l.strip_prefix("style ") {
            styles.push(format!("`.{}`", r.split_whitespace().next().unwrap_or("")));
        }
    }
    let mut s = format!("**{target}**\n\n{}", head.join(" "));
    if !defs.is_empty() {
        s += &format!("\n\nTemplates: {}", defs.join(", "));
    }
    if !styles.is_empty() {
        s += &format!("\n\nStyles: {}", styles.join(" "));
    }
    Some(s)
}

fn info_md(idx: &Index, kind: Kind, name: &str, here: &str) -> String {
    let (info, what) = match kind {
        Kind::Node => (idx.nodes.get(name), "node"),
        Kind::Style => (idx.styles.get(name), "style"),
        Kind::Macro => {
            return match idx.def(name) {
                Some(d) if d.doc.is_empty() => fence(&format!("def {}", signature(d))),
                Some(d) => format!("{}\n\n{}", fence(&format!("def {}", signature(d))), d.doc),
                None => format!("Unknown template `{name}`."),
            };
        }
    };
    let Some(info) = info else {
        return format!("Undeclared {what} `{name}`.");
    };
    let mut s = fence(&info.text.join("\n"));
    // Template-generated uses have no source location, so a zero count would mislead.
    let uses = idx.all(kind, name).filter(|o| !o.decl).count();
    if uses > 0 {
        s += &format!("\n\n{uses} use{}", if uses == 1 { "" } else { "s" });
    }
    if let Some(d) = idx.all(kind, name).find(|o| o.decl)
        && d.loc.path != here
    {
        s += &format!(" · declared in `{}:{}`", d.loc.path, d.loc.start.0 + 1);
    }
    s
}

fn word_md(w: &Word) -> Option<String> {
    Some(match w {
        Word::Keyword(k) => {
            let (_, _, sig, doc) = docs::KEYWORDS.iter().find(|e| e.0 == k)?;
            format!("{}\n\n{doc}", fence(sig))
        }
        Word::Key { stmt, key } => {
            let a = docs::attr(stmt, key)?;
            format!("**{key}=** on `{stmt}`\n\n{}{}", a.doc, values_line(a))
        }
        Word::Value { key, text } => {
            let d = docs::word(docs::value_role(key), text)?;
            format!("**{key}={text}**\n\n{d}")
        }
        Word::Role(role, name) => format!("**{name}** ({role})\n\n{}", docs::word(role, name)?),
        Word::Include(t) => match module_doc(t) {
            Some(m) => m,
            None => format!("Includes `{t}`, relative to this file."),
        },
    })
}

/// operator_at finds an edge operator touching column `ch` of `line`.
fn operator_at(line: &str, ch: usize) -> Option<(&'static str, &'static str, usize)> {
    for &(op, doc) in docs::OPERATORS {
        let mut from = 0;
        while let Some(i) = line[from..].find(op).map(|i| from + i) {
            let lo = line[..i].encode_utf16().count();
            let hi = lo + op.len();
            if (lo..=hi).contains(&ch) {
                return Some((op, doc, lo));
            }
            from = i + op.len();
        }
    }
    None
}

fn word_at(line: &str, ch: usize) -> Option<(String, usize, usize)> {
    let chars: Vec<(usize, char)> = line
        .chars()
        .scan(0, |u, c| {
            let at = *u;
            *u += c.len_utf16();
            Some((at, c))
        })
        .collect();
    let i = chars
        .iter()
        .position(|&(u, _)| u >= ch)
        .unwrap_or(chars.len());
    let mut lo = i;
    while lo > 0 && is_word(chars[lo - 1].1) {
        lo -= 1;
    }
    let mut hi = i;
    while hi < chars.len() && is_word(chars[hi].1) {
        hi += 1;
    }
    let end = |k: usize| chars.get(k).map_or(line.encode_utf16().count(), |c| c.0);
    (lo < hi).then(|| {
        (
            chars[lo..hi].iter().map(|c| c.1).collect(),
            end(lo),
            end(hi),
        )
    })
}

pub fn hover(doc: &Doc, p: &Value, diags: &[Value]) -> Value {
    let (line, ch) = pos(p);
    let idx = doc.index();
    let text = line_of(doc.text, line);
    let mut parts = Vec::new();
    let mut range = None;
    match idx.at(doc.path, line, ch) {
        Some(Hit::Occ(o)) => {
            parts.push(info_md(&idx, o.kind, &o.name, doc.path));
            range = Some(o.loc.range());
        }
        Some(Hit::Spot(s)) => {
            parts.extend(word_md(&s.word));
            range = Some(s.loc.range());
        }
        None => {
            if let Some((op, d, lo)) = operator_at(text, ch) {
                parts.push(format!("**{op}**\n\n{d}"));
                range = Some(
                    json!({ "start": { "line": line, "character": lo }, "end": { "line": line, "character": lo + op.len() } }),
                );
            } else if let Some((w, lo, hi)) = word_at(text, ch)
                && let Some(d) = docs::word("relation", &w)
            {
                parts.push(format!("**{w}** (relation)\n\n{d}"));
                range = Some(
                    json!({ "start": { "line": line, "character": lo }, "end": { "line": line, "character": hi } }),
                );
            }
        }
    }
    for d in diags {
        let (s, e) = (pos(&d["range"]["start"]), pos(&d["range"]["end"]));
        if (s..=e).contains(&(line, ch))
            && let Some(code) = d["code"].as_str()
            && let Some(x) = docs::explain(code)
        {
            parts.push(format!(
                "**{code}**: {x}\n\nRun `fitd explain {code}` for more."
            ));
        }
    }
    if parts.is_empty() {
        return Value::Null;
    }
    let mut out = json!({ "contents": md(parts.join("\n\n---\n\n")) });
    if let Some(r) = range {
        out["range"] = r;
    }
    out
}

fn target<'i>(idx: &'i Index, doc: &Doc, p: &Value) -> Option<(Kind, &'i str)> {
    let (line, ch) = pos(p);
    if let Some(Hit::Occ(o)) = idx.at(doc.path, line, ch) {
        return Some((o.kind, o.name.as_str()));
    }
    // Lines the parser dropped (mid-edit) still resolve by the word under the cursor.
    let text = line_of(doc.text, line);
    let (w, lo, _) = word_at(text, ch)?;
    let kind = if slice16(text, lo.saturating_sub(1), lo) == Some(".") {
        Kind::Style
    } else {
        Kind::Node
    };
    idx.occs
        .iter()
        .find(|o| o.kind == kind && o.name == w && o.decl)
        .map(|o| (kind, o.name.as_str()))
}

pub fn definition(doc: &Doc, p: &Value) -> Value {
    let idx = doc.index();
    let (line, ch) = pos(p);
    if let Some(Hit::Spot(s)) = idx.at(doc.path, line, ch)
        && let Word::Include(t) = &s.word
    {
        let path = if crate::stdlib::get(t).is_some() {
            t.clone()
        } else {
            std::path::Path::new(doc.path)
                .parent()
                .unwrap_or(std::path::Path::new(""))
                .join(t)
                .to_string_lossy()
                .into_owned()
        };
        let uri = doc.uri(&path);
        return if uri.is_null() {
            Value::Null
        } else {
            json!({ "uri": uri, "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } } })
        };
    }
    let Some((kind, name)) = target(&idx, doc, p) else {
        return Value::Null;
    };
    let decls: Vec<&Loc> = idx
        .all(kind, name)
        .filter(|o| o.decl)
        .map(|o| &o.loc)
        .collect();
    decls
        .iter()
        .find(|l| l.path == doc.path)
        .or(decls.first())
        .and_then(|l| doc.location(l))
        .unwrap_or(Value::Null)
}

pub fn references(doc: &Doc, p: &Value, with_decl: bool) -> Value {
    let idx = doc.index();
    let Some((kind, name)) = target(&idx, doc, p) else {
        return Value::Null;
    };
    Value::Array(
        idx.all(kind, name)
            .filter(|o| with_decl || !o.decl)
            .filter_map(|o| doc.location(&o.loc))
            .collect(),
    )
}

pub fn highlights(doc: &Doc, p: &Value) -> Value {
    let idx = doc.index();
    let Some((kind, name)) = target(&idx, doc, p) else {
        return Value::Null;
    };
    Value::Array(
        idx.all(kind, name)
            .filter(|o| o.loc.path == doc.path)
            .map(|o| json!({ "range": o.loc.range(), "kind": if o.decl { 3 } else { 2 } }))
            .collect(),
    )
}

fn in_std(idx: &Index, kind: Kind, name: &str) -> bool {
    idx.all(kind, name)
        .any(|o| crate::stdlib::get(&o.loc.path).is_some())
}

pub fn prepare_rename(doc: &Doc, p: &Value) -> Value {
    let idx = doc.index();
    let (line, ch) = pos(p);
    match idx.at(doc.path, line, ch) {
        Some(Hit::Occ(o)) if !in_std(&idx, o.kind, &o.name) => {
            json!({ "range": o.loc.range(), "placeholder": o.name })
        }
        _ => Value::Null,
    }
}

fn valid_name(s: &str) -> bool {
    let cs: Vec<char> = s.chars().collect();
    !cs.is_empty()
        && !cs[0].is_ascii_digit()
        && cs.iter().enumerate().all(|(i, &c)| {
            c.is_alphanumeric()
                || c == '_'
                || c == '-'
                    && i > 0
                    && cs
                        .get(i + 1)
                        .is_some_and(|&n| n.is_alphanumeric() || n == '_')
        })
}

pub fn rename(doc: &Doc, p: &Value, new: &str) -> Result<Value, String> {
    let idx = doc.index();
    let Some((kind, name)) = target(&idx, doc, p) else {
        return Ok(Value::Null);
    };
    if in_std(&idx, kind, name) {
        return Err(format!("`{name}` comes from the bundled library"));
    }
    let ok = if kind == Kind::Macro {
        !new.is_empty() && new.chars().all(|c| c.is_alphanumeric() || c == '_')
    } else {
        valid_name(new)
    };
    if !ok {
        return Err(format!("`{new}` is not a valid name"));
    }
    let mut changes = serde_json::Map::new();
    for o in idx.all(kind, name) {
        let uri = doc.uri(&o.loc.path);
        let Some(u) = uri.as_str() else { continue };
        let edits = changes
            .entry(u.to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        edits
            .as_array_mut()
            .unwrap()
            .push(json!({ "range": o.loc.range(), "newText": new }));
    }
    Ok(json!({ "changes": changes }))
}

pub fn symbols(doc: &Doc) -> Value {
    let idx = doc.index();
    Value::Array(
        idx.symbols
            .iter()
            .map(|s| {
                json!({
                    "name": if s.name.is_empty() { "(untitled)" } else { &s.name },
                    "detail": s.detail,
                    "kind": s.kind,
                    "range": s.whole.range(),
                    "selectionRange": s.pick.range(),
                })
            })
            .collect(),
    )
}

/// folds covers macro blocks, nodes with body lines, and runs of comments.
pub fn folds(text: &str) -> Value {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut node: Option<usize> = None;
    let mut comment: Option<usize> = None;
    let mut push = |lo: usize, hi: usize, kind: Option<&str>| {
        if hi > lo {
            let mut r = json!({ "startLine": lo, "endLine": hi });
            if let Some(k) = kind {
                r["kind"] = json!(k);
            }
            out.push(r);
        }
    };
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        let w: String = t
            .chars()
            .take_while(|&c| c.is_alphanumeric() || c == '_')
            .collect();
        let body = t.starts_with('|') || t.starts_with('~');
        if !body && let Some(n) = node.take() {
            push(n, i - 1, None);
        }
        if t.starts_with('#') {
            comment.get_or_insert(i);
        } else if let Some(c) = comment.take() {
            push(c, i - 1, Some("comment"));
        }
        let rest = &t[w.len()..];
        let directive = !rest.starts_with('-') && !rest.trim_start().starts_with(['-', '.']);
        match w.as_str() {
            "for" | "if" | "def" if directive => open.push(i),
            "end" if directive => {
                if let Some(lo) = open.pop() {
                    push(lo, i, None);
                }
            }
            "else" if directive => {
                if let Some(lo) = open.pop() {
                    push(lo, i - 1, None);
                }
                open.push(i);
            }
            "node" => node = Some(i),
            _ => {}
        }
    }
    let n = lines.len().saturating_sub(1);
    if let Some(lo) = node {
        push(lo, n, None);
    }
    if let Some(c) = comment {
        push(c, n, Some("comment"));
    }
    Value::Array(out)
}

fn hex(c: &str) -> Option<(f64, f64, f64)> {
    let h = c.strip_prefix('#')?;
    let full: String = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect(),
        6 => h.to_string(),
        _ => return None,
    };
    let v = |i: usize| {
        u8::from_str_radix(&full[i..i + 2], 16)
            .ok()
            .map(|x| x as f64 / 255.0)
    };
    Some((v(0)?, v(2)?, v(4)?))
}

/// colors finds `=#hex` tokens and the hex entries of quoted palettes.
pub fn colors(text: &str) -> Value {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with('#') || t.starts_with('|') {
            continue;
        }
        let b = line.as_bytes();
        let mut quoted = false;
        for (i, &c) in b.iter().enumerate() {
            if c == b'"' {
                quoted = !quoted;
            }
            if c != b'#' || !(quoted || i > 0 && b[i - 1] == b'=') {
                continue;
            }
            let len = b[i + 1..]
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric())
                .count();
            let tok = &line[i..i + 1 + len];
            if let Some((r, g, bl)) = hex(tok) {
                let lo = line[..i].encode_utf16().count();
                out.push(json!({
                    "range": { "start": { "line": n, "character": lo }, "end": { "line": n, "character": lo + tok.len() } },
                    "color": { "red": r, "green": g, "blue": bl, "alpha": 1.0 },
                }));
            }
        }
    }
    Value::Array(out)
}

pub fn color_presentation(params: &Value) -> Value {
    let c = &params["color"];
    let byte = |k: &str| (c[k].as_f64().unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u8;
    let label = format!(
        "#{:02X}{:02X}{:02X}",
        byte("red"),
        byte("green"),
        byte("blue")
    );
    json!([{ "label": label, "textEdit": { "range": params["range"], "newText": label } }])
}

/// Call is a `use NAME(args)` line: name, each argument's UTF-16 start and text, and whether `)` closed it.
type Call = (String, Vec<(usize, String)>, bool);

fn call(line: &str) -> Option<Call> {
    let lead = line.len() - line.trim_start().len();
    let rest = line[lead..].strip_prefix("use")?;
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let name_lo = line.len() - rest.trim_start().len();
    let name: String = line[name_lo..]
        .chars()
        .take_while(|&c| c.is_alphanumeric() || c == '_')
        .collect();
    let after = name_lo + name.len();
    let open = after + line[after..].find('(')?;
    let (mut args, mut depth, mut quoted, mut start) = (Vec::new(), 0, false, open + 1);
    let mut closed = false;
    let mut esc = false;
    for (i, c) in line[open + 1..]
        .char_indices()
        .map(|(i, c)| (open + 1 + i, c))
    {
        if quoted {
            match c {
                _ if esc => esc = false,
                '\\' => esc = true,
                '"' => quoted = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            ',' | ')' => {
                let raw = &line[start..i];
                let skip = raw.len() - raw.trim_start().len();
                if !raw.trim().is_empty() {
                    args.push((
                        line[..start + skip].encode_utf16().count(),
                        raw.trim().to_string(),
                    ));
                }
                start = i + 1;
                if c == ')' {
                    closed = true;
                    break;
                }
            }
            _ => {}
        }
    }
    if !closed {
        let raw = &line[start..];
        let skip = raw.len() - raw.trim_start().len();
        args.push((
            line[..start + skip].encode_utf16().count(),
            raw.trim().to_string(),
        ));
    }
    Some((name, args, closed))
}

pub fn inlay_hints(doc: &Doc, params: &Value) -> Value {
    let idx = doc.index();
    let lo = params["range"]["start"]["line"].as_u64().unwrap_or(0) as usize;
    let hi = params["range"]["end"]["line"].as_u64().unwrap_or(u64::MAX) as usize;
    let mut out = Vec::new();
    for (n, line) in doc
        .text
        .lines()
        .enumerate()
        .skip(lo)
        .take(hi.saturating_sub(lo) + 1)
    {
        let Some((name, args, _)) = call(line) else {
            continue;
        };
        let Some(d) = idx.def(&name) else { continue };
        for ((at, arg), p) in args.iter().zip(&d.params) {
            if arg.trim_matches('"') == p {
                continue;
            }
            out.push(json!({
                "position": { "line": n, "character": at },
                "label": format!("{p}:"),
                "kind": 2,
                "paddingRight": true,
            }));
        }
    }
    Value::Array(out)
}

pub fn signature_help(doc: &Doc, p: &Value) -> Value {
    let (line, ch) = pos(p);
    let text = line_of(doc.text, line);
    let cut = text
        .char_indices()
        .scan(0, |u, (i, c)| {
            let at = *u;
            *u += c.len_utf16();
            Some((at, i))
        })
        .find(|&(u, _)| u >= ch)
        .map_or(text.len(), |(_, i)| i);
    let Some((name, args, closed)) = call(&text[..cut]) else {
        return Value::Null;
    };
    if closed {
        return Value::Null;
    }
    let idx = doc.index();
    let Some(d) = idx.def(&name) else {
        return Value::Null;
    };
    let label = signature(d);
    let mut at = name.len() + 1;
    let params: Vec<Value> = d
        .params
        .iter()
        .map(|p| {
            let r = json!([at, at + p.len()]);
            at += p.len() + 2;
            json!({ "label": r })
        })
        .collect();
    let mut sig = json!({ "label": label, "parameters": params });
    if !d.doc.is_empty() {
        sig["documentation"] = md(d.doc.clone());
    }
    json!({
        "signatures": [sig],
        "activeSignature": 0,
        "activeParameter": args.len().saturating_sub(1),
    })
}

fn quoted_name(msg: &str) -> Option<&str> {
    msg.split('`').nth(1)
}

/// diag_data records what a code action needs to fix a front-end diagnostic.
pub fn diag_data(d: &crate::diag::Diag) -> Option<Value> {
    if let Some(h) = &d.help
        && let Some(s) = h
            .strip_prefix("did you mean `")
            .and_then(|s| s.strip_suffix("`?"))
    {
        return Some(json!({ "replace": s }));
    }
    let name = quoted_name(&d.msg)?;
    match d.code {
        "E0001" if d.msg.starts_with("unknown node") => {
            Some(json!({ "declare": "node", "name": name }))
        }
        "E0004" => Some(json!({ "declare": "style", "name": name })),
        "W0203" => Some(json!({ "remove": name })),
        _ => None,
    }
}

/// top_level finds the unindented line that opens the block holding `line`, so a declaration lands outside templates and loops.
fn top_level(text: &str, line: usize) -> usize {
    let lines: Vec<&str> = text.lines().collect();
    (0..=line.min(lines.len().saturating_sub(1)))
        .rev()
        .find(|&i| {
            let l = lines[i];
            !l.starts_with([' ', '\t'])
                && !l.trim().is_empty()
                && !l.starts_with('#')
                && !l.starts_with("end")
                || i == 0
        })
        .filter(|&i| {
            let w = lines[i].split_whitespace().next().unwrap_or("");
            matches!(w, "def" | "for" | "if")
        })
        .map(|mut i| {
            while i > 0 && lines[i - 1].starts_with('#') {
                i -= 1;
            }
            i
        })
        .unwrap_or(line)
}

pub fn code_actions(doc: &Doc, params: &Value) -> Value {
    let mut out = Vec::new();
    let uri = params["textDocument"]["uri"].clone();
    let diags = params["context"]["diagnostics"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for d in &diags {
        let data = &d["data"];
        let range = &d["range"];
        let line = range["start"]["line"].as_u64().unwrap_or(0) as usize;
        let top = top_level(doc.text, line);
        let line_start = json!({ "line": line, "character": 0 });
        let top_start = json!({ "line": top, "character": 0 });
        let (title, edit) = if let Some(s) = data["replace"].as_str() {
            (
                format!("Replace with `{s}`"),
                json!({ "range": range, "newText": s }),
            )
        } else if let (Some(k), Some(n)) = (data["declare"].as_str(), data["name"].as_str()) {
            (
                format!("Declare {k} `{n}`"),
                json!({ "range": { "start": top_start, "end": top_start }, "newText": format!("{k} {n}\n") }),
            )
        } else if let Some(n) = data["remove"].as_str() {
            let next = json!({ "line": line + 1, "character": 0 });
            (
                format!("Remove unused style `{n}`"),
                json!({ "range": { "start": line_start, "end": next }, "newText": "" }),
            )
        } else {
            continue;
        };
        out.push(json!({
            "title": title,
            "kind": "quickfix",
            "diagnostics": [d],
            "isPreferred": data["replace"].is_string(),
            "edit": { "changes": { uri.as_str().unwrap_or(""): [edit] } },
        }));
    }
    Value::Array(out)
}

fn item(label: &str, kind: u32, detail: &str, doc: &str) -> Value {
    let mut v = json!({ "label": label, "kind": kind });
    if !detail.is_empty() {
        v["detail"] = json!(detail);
    }
    if !doc.is_empty() {
        v["documentation"] = md(doc.to_string());
    }
    v
}

fn node_items(idx: &Index) -> Vec<Value> {
    idx.node_order
        .iter()
        .map(|n| {
            let i = &idx.nodes[n];
            item(
                n,
                6,
                i.title.as_deref().unwrap_or(""),
                &fence(&i.text.join("\n")),
            )
        })
        .collect()
}

fn attr_items(stmt: &str) -> Vec<Value> {
    docs::attrs(stmt)
        .into_iter()
        .map(|a| {
            let mut v = item(
                a.key,
                10,
                &format!("{stmt} attribute"),
                &format!("{}{}", a.doc, values_line(a)),
            );
            v["insertText"] = json!(format!("{}=", a.key));
            v
        })
        .collect()
}

fn role_items(role: &str, kind: u32) -> Vec<Value> {
    docs::words(role)
        .into_iter()
        .map(|(n, d)| item(n, kind, role, d))
        .collect()
}

const EDGE_OPS: &[&str] = &["->", "--", "..>", ".."];

/// statement names the statement kind of a line, by its first word.
fn statement(first: &str) -> &'static str {
    match first {
        "node" => "node",
        "style" => "style",
        "key" => "key",
        "group" => "group",
        "layout" => "layout",
        "page" => "page",
        "font" => "font",
        "theme" => "theme",
        "require" => "require",
        "prefer" => "prefer",
        "include" => "include",
        "title" => "title",
        "root" => "root",
        w if docs::MACRO_WORDS.contains(&w) => "macro",
        _ => "edge",
    }
}

fn include_items(path: &str, typed: &str) -> Vec<Value> {
    let mut out: Vec<Value> = crate::stdlib::MODULES
        .iter()
        .map(|(n, _)| {
            let t = format!("std/{n}.fitd");
            item(&t, 9, "bundled module", &module_doc(&t).unwrap_or_default())
        })
        .collect();
    let base = std::path::Path::new(path)
        .parent()
        .unwrap_or(std::path::Path::new(""));
    let (sub, _) = typed.rsplit_once('/').unwrap_or(("", typed));
    if let Ok(rd) = std::fs::read_dir(base.join(sub)) {
        let mut found: Vec<(String, bool)> = rd
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let dir = e.file_type().ok()?.is_dir();
                (!name.starts_with('.') && (dir || name.ends_with(".fitd"))).then_some((name, dir))
            })
            .collect();
        found.sort();
        for (name, dir) in found {
            let full = if sub.is_empty() {
                name
            } else {
                format!("{sub}/{name}")
            };
            if base.join(&full) == std::path::Path::new(path) {
                continue;
            }
            let label = if dir { format!("{full}/") } else { full };
            out.push(item(&label, if dir { 19 } else { 17 }, "", ""));
        }
    }
    out
}

pub fn completion(doc: &Doc, p: &Value, snippets: bool) -> Value {
    let off = offset(doc.text, p);
    let line_start = doc.text[..off].rfind('\n').map_or(0, |i| i + 1);
    let before = &doc.text[line_start..off];
    let t = before.trim_start();
    let typed_len = before.len() - before.trim_end_matches(is_word).len();
    let head = &before[..before.len() - typed_len];
    let words: Vec<&str> = head.split_whitespace().collect();
    let first = words.first().copied().unwrap_or("");
    let stmt = statement(first);
    let quotes = head.matches('"').count() - head.matches("\\\"").count();
    if t.starts_with('#') || t.starts_with('|') || t.starts_with('~') {
        return json!([]);
    }
    if stmt == "include" && quotes % 2 == 1 {
        let q = before.rfind('"').unwrap_or(0) + 1;
        let typed = &before[q..];
        let lo = position16(doc.text, line_start + q);
        let range = json!({ "start": lo, "end": p });
        let items = include_items(doc.path, typed)
            .into_iter()
            .map(|mut v| {
                v["textEdit"] = json!({ "range": range, "newText": v["label"] });
                v
            })
            .collect();
        return Value::Array(items);
    }
    if quotes % 2 == 1 {
        return json!([]);
    }
    let idx = doc.index();
    if first == "use" && words.len() == 1 {
        return Value::Array(
            idx.defs
                .iter()
                .map(|d| {
                    let mut v = item(&d.name, 3, &signature(d), &d.doc);
                    if snippets {
                        let ps: Vec<String> = d
                            .params
                            .iter()
                            .enumerate()
                            .map(|(i, p)| format!("${{{}:{p}}}", i + 1))
                            .collect();
                        v["insertText"] = json!(format!("{}({})", d.name, ps.join(", ")));
                        v["insertTextFormat"] = json!(2);
                    }
                    v
                })
                .collect(),
        );
    }
    if head.ends_with('.') && !head.ends_with("..") {
        return Value::Array(
            idx.style_order
                .iter()
                .map(|s| item(s, 7, "style", &fence(&idx.styles[s].text.join("\n"))))
                .collect(),
        );
    }
    if let Some(eq) = head.strip_suffix('=') {
        let key: String = eq
            .chars()
            .rev()
            .take_while(|&c| is_word(c))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if stmt == "layout" && key == "root" {
            return Value::Array(node_items(&idx));
        }
        return Value::Array(match docs::attr(stmt, &key) {
            Some(a) => a
                .values
                .iter()
                .map(|v| {
                    item(
                        v,
                        20,
                        &key,
                        docs::word(docs::value_role(&key), v).unwrap_or(""),
                    )
                })
                .collect(),
            None => Vec::new(),
        });
    }
    if words.is_empty() {
        let mut out: Vec<Value> = docs::KEYWORDS
            .iter()
            .map(|&(k, snip, sig, d)| {
                let mut v = item(k, 14, sig, d);
                if snippets {
                    v["insertText"] = json!(snip);
                    v["insertTextFormat"] = json!(2);
                }
                v
            })
            .collect();
        out.extend(node_items(&idx));
        return Value::Array(out);
    }
    let args = &words[1..];
    let items = match stmt {
        "layout" if args.is_empty() => role_items("layout", 20),
        "layout" => attr_items("layout"),
        "theme" if args.is_empty() => {
            let mut v = role_items("theme", 20);
            v.extend(attr_items("theme"));
            v
        }
        "theme" | "font" | "style" | "key" => attr_items(stmt),
        "page" if args.is_empty() => role_items("page", 20),
        "page" => {
            let mut v = role_items("flag", 20);
            v.extend(attr_items("page"));
            v
        }
        "node" if args.is_empty() => Vec::new(),
        "node" => attr_items("node"),
        "root" => node_items(&idx),
        "group" => {
            let mut v = node_items(&idx);
            if !args.is_empty() && !head.trim_end().ends_with(',') {
                v.extend(attr_items("group"));
            }
            v
        }
        "require" | "prefer" => constraint_items(&idx, stmt, args, head),
        "edge" => {
            let last = words.last().copied().unwrap_or("");
            if EDGE_OPS.contains(&last) || head.trim_end().ends_with(',') {
                node_items(&idx)
            } else {
                let mut v = node_items(&idx);
                v.extend(attr_items("edge"));
                v
            }
        }
        _ => Vec::new(),
    };
    Value::Array(items)
}

fn constraint_items(idx: &Index, stmt: &str, args: &[&str], head: &str) -> Vec<Value> {
    let relations = || {
        let mut v = role_items("relation", 14);
        v.retain(|i| i["label"] != "align");
        v
    };
    if args.is_empty() {
        let mut v = node_items(idx);
        v.push(item(
            "align",
            14,
            "relation",
            docs::word("relation", "align").unwrap_or(""),
        ));
        return v;
    }
    if args[0] == "align" || args[0].starts_with("align(") {
        if head.contains(')') {
            return attr_items(stmt);
        }
        if head.contains('(') {
            return node_items(idx);
        }
        return role_items("side", 20);
    }
    let last = *args.last().unwrap();
    if last == "at" {
        return role_items("place", 20);
    }
    if args.len() >= 2 && args[args.len() - 2] == "at" {
        return attr_items(stmt);
    }
    if docs::word("relation", last).is_some() {
        return node_items(idx);
    }
    let mut v = relations();
    if args.len() >= 3 {
        v.extend(attr_items(stmt));
    }
    v
}

fn position16(text: &str, off: usize) -> Value {
    super::position(text, off)
}
