//! Minimal language server (`fitd lsp`): front-end diagnostics on every change, full layout
//! diagnostics on save, go-to-definition and completion for node and style names.

use crate::ast::{Name, StmtKind};
use crate::diag::{Diag, Level, Source, Span};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

const KEYWORDS: &[&str] = &["title", "root", "layout", "page", "font", "theme", "style", "node", "group", "require", "prefer", "include"];

/// Out is stdout, shared with the threads that run full layouts.
type Out = Arc<Mutex<std::io::Stdout>>;

fn send(out: &Out, msg: Value) {
    let body = msg.to_string();
    let mut o = out.lock().unwrap();
    let _ = write!(o, "Content-Length: {}\r\n\r\n{body}", body.len());
    let _ = o.flush();
}

fn read(input: &mut impl BufRead) -> Option<Value> {
    let mut len = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(n) = line.strip_prefix("Content-Length:") {
            len = n.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0; len?];
    input.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

/// position converts a byte offset to an LSP position, whose character counts UTF-16 units.
fn position(text: &str, off: usize) -> Value {
    let off = off.min(text.len());
    let line_start = text[..off].rfind('\n').map_or(0, |i| i + 1);
    let line = text[..line_start].matches('\n').count();
    json!({ "line": line, "character": text[line_start..off].encode_utf16().count() })
}

/// offset converts an LSP position back to a byte offset, clamped to its line.
fn offset(text: &str, pos: &Value) -> usize {
    let (line, ch) = (pos["line"].as_u64().unwrap_or(0) as usize, pos["character"].as_u64().unwrap_or(0) as usize);
    let start: usize = text.split_inclusive('\n').take(line).map(str::len).sum();
    let mut units = 0;
    for (i, c) in text[start..].char_indices() {
        if units >= ch || c == '\n' {
            return start + i;
        }
        units += c.len_utf16();
    }
    text.len()
}

fn range(text: &str, span: Span) -> Value {
    json!({ "start": position(text, span.lo), "end": position(text, span.hi.max(span.lo)) })
}

fn lsp_diag(text: &str, d: &Diag) -> Value {
    let span = d.labels.iter().find(|l| l.primary).or(d.labels.first()).map_or(Span::new(0, 0), |l| l.span);
    let mut msg = d.msg.clone();
    for n in &d.notes {
        msg += &format!("\nnote: {n}");
    }
    if let Some(h) = &d.help {
        msg += &format!("\nhelp: {h}");
    }
    json!({ "range": range(text, span), "severity": if d.level == Level::Error { 1 } else { 2 }, "code": d.code, "source": "fitd", "message": msg })
}

/// front_end runs include, lex, parse, and sema, which include the static contradiction
/// check (E0008). Diagnostics that point into included files are left to those files.
fn front_end(path: &str, text: &str) -> Vec<Value> {
    let (src, mut diags) = crate::include::expand(path, text);
    let (stmts, parse_diags) = crate::parse::parse(&src.text);
    diags.extend(parse_diags);
    diags.extend(crate::sema::analyze(&stmts).1);
    let here = |off: usize| Some(src.file_offset(off)).filter(|(p, _)| *p == path).map(|(_, o)| o);
    let mut out = Vec::new();
    for mut d in diags.into_iter().filter(|d| d.level != Level::Note) {
        let had = !d.labels.is_empty();
        d.labels.retain_mut(|l| match (here(l.span.lo), here(l.span.hi.max(l.span.lo))) {
            (Some(lo), Some(hi)) => {
                l.span = Span::new(lo, hi);
                true
            }
            _ => false,
        });
        if !had || !d.labels.is_empty() {
            out.push(lsp_diag(text, &d));
        }
    }
    out
}

/// layout builds the saved file in a child process and reads back its `--short` diagnostics,
/// so the server shares every check with the command line.
fn layout(path: &str, text: &str) -> Vec<Value> {
    let out = std::env::temp_dir().join(format!("fitd-lsp-{}.svg", std::process::id()));
    let Ok(exe) = std::env::current_exe() else { return Vec::new() };
    let Ok(run) = std::process::Command::new(exe).args([path, "-o", out.to_str().unwrap_or("fitd-lsp.svg"), "--short"]).output() else { return Vec::new() };
    let _ = std::fs::remove_file(&out);
    let src = Source::new(path, text);
    let mut diags = Vec::new();
    for line in String::from_utf8_lossy(&run.stderr).lines() {
        let (at, rest) = match line.strip_prefix(path).and_then(|l| l.strip_prefix(':')) {
            Some(l) => {
                let mut it = l.splitn(3, ':');
                let (ln, col) = (it.next().and_then(|x| x.parse::<usize>().ok()), it.next().and_then(|x| x.parse::<usize>().ok()));
                match (ln, col, it.next()) {
                    (Some(ln), Some(col), Some(rest)) => (Some((ln, col)), rest.trim_start()),
                    _ => continue,
                }
            }
            None if line.starts_with("error: ") && !line.contains("aborting due to") => (None, line),
            None => continue,
        };
        let severity = if rest.starts_with("error") { 1 } else { 2 };
        let code = rest.split_once('[').and_then(|(_, r)| r.split_once(']')).map_or("", |(c, _)| c);
        let msg = rest.split_once("]: ").map_or(rest, |(_, m)| m);
        // `--short` gives a line and column, so the mark runs from there to the end of the line.
        let r = match at {
            Some((ln, col)) if ln >= 1 && ln <= text.lines().count().max(1) => {
                let line_text = src.line_text(ln);
                let start_units: usize = line_text.chars().take(col.saturating_sub(1)).map(char::len_utf16).sum();
                json!({ "start": { "line": ln - 1, "character": start_units }, "end": { "line": ln - 1, "character": line_text.encode_utf16().count() } })
            }
            _ => json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } }),
        };
        diags.push(json!({ "range": r, "severity": severity, "code": code, "source": "fitd", "message": msg }));
    }
    diags
}

/// Declarations of node ids and style names, for definitions and completion.
fn declared(text: &str) -> (Vec<Name>, Vec<Name>) {
    let (stmts, _) = crate::parse::parse(text);
    let (mut nodes, mut styles) = (Vec::new(), Vec::new());
    for st in stmts {
        match st.kind {
            StmtKind::Node { id, .. } => nodes.push(id),
            StmtKind::Style { name, .. } => styles.push(name),
            _ => {}
        }
    }
    (nodes, styles)
}

/// word_at finds the name under the cursor and whether a `.` marks it as a style.
fn word_at(text: &str, off: usize) -> Option<(String, bool)> {
    let is = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let lo = text[..off].char_indices().rev().take_while(|&(_, c)| is(c)).last().map_or(off, |(i, _)| i);
    let hi = off + text[off..].char_indices().find(|&(_, c)| !is(c)).map_or(text.len() - off, |(i, _)| i);
    (lo < hi).then(|| (text[lo..hi].to_string(), text[..lo].ends_with('.')))
}

fn definition(text: &str, uri: &Value, pos: &Value) -> Value {
    let Some((word, style)) = word_at(text, offset(text, pos)) else { return Value::Null };
    let (nodes, styles) = declared(text);
    let pool = if style { styles } else { nodes };
    pool.iter().find(|n| n.text == word).map_or(Value::Null, |n| json!({ "uri": uri, "range": range(text, n.span) }))
}

fn completion(text: &str, pos: &Value) -> Value {
    let off = offset(text, pos);
    let line_start = text[..off].rfind('\n').map_or(0, |i| i + 1);
    let before = &text[line_start..off];
    let typed = before.rsplit(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-')).next().unwrap_or("");
    let (nodes, styles) = declared(text);
    let items: Vec<Value> = if before[..before.len() - typed.len()].ends_with('.') {
        styles.iter().map(|s| json!({ "label": s.text, "kind": 7 })).collect()
    } else if before.trim() == typed {
        KEYWORDS.iter().map(|k| json!({ "label": k, "kind": 14 })).collect()
    } else {
        // A node may be referenced before its `node` line, so every declaration is offered.
        let mut seen = std::collections::HashSet::new();
        nodes.iter().filter(|n| seen.insert(n.text.clone())).map(|n| json!({ "label": n.text, "kind": 6 })).collect()
    };
    Value::Array(items)
}

fn path_of(uri: &str) -> Option<String> {
    let p = uri.strip_prefix("file://")?;
    let mut out = Vec::new();
    let b = p.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(x) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).ok()?, 16)
        {
            out.push(x);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    let path = String::from_utf8(out).ok()?;
    // `file:///C:/x` is `C:/x` on Windows.
    let drive = path.len() > 2 && path.as_bytes()[2] == b':';
    Some(if drive { path[1..].to_string() } else { path })
}

/// serve runs the server on stdin and stdout until the client sends `exit`.
pub fn serve() -> std::process::ExitCode {
    let out: Out = Arc::new(Mutex::new(std::io::stdout()));
    let mut input = std::io::BufReader::new(std::io::stdin());
    let mut docs: HashMap<String, String> = HashMap::new();
    let publish = |out: &Out, uri: &str, diags: Vec<Value>| send(out, json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": { "uri": uri, "diagnostics": diags } }));
    // Uri to the change count when its last full layout started, so a stale result is dropped.
    let versions: Arc<Mutex<HashMap<String, u64>>> = Arc::default();
    let mut shutdown = false;
    while let Some(msg) = read(&mut input) {
        let method = msg["method"].as_str().unwrap_or("");
        let params = &msg["params"];
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
        let reply = |result: Value| send(&out, json!({ "jsonrpc": "2.0", "id": msg["id"], "result": result }));
        let bump = |uri: &str| *versions.lock().unwrap().entry(uri.to_string()).or_insert(0) += 1;
        match method {
            "initialize" => reply(json!({
                "capabilities": {
                    "textDocumentSync": { "openClose": true, "change": 1, "save": { "includeText": false } },
                    "definitionProvider": true,
                    "completionProvider": { "triggerCharacters": ["."] }
                },
                "serverInfo": { "name": "fitd", "version": env!("CARGO_PKG_VERSION") }
            })),
            "shutdown" => {
                shutdown = true;
                reply(Value::Null);
            }
            "exit" => break,
            "textDocument/didOpen" | "textDocument/didChange" => {
                let text = if method == "textDocument/didOpen" { &params["textDocument"]["text"] } else { &params["contentChanges"][0]["text"] };
                let text = text.as_str().unwrap_or("").to_string();
                bump(&uri);
                publish(&out, &uri, front_end(&path_of(&uri).unwrap_or_default(), &text));
                docs.insert(uri, text);
            }
            "textDocument/didClose" => {
                docs.remove(&uri);
                bump(&uri);
                publish(&out, &uri, Vec::new());
            }
            "textDocument/didSave" => {
                let (Some(path), Some(text)) = (path_of(&uri), docs.get(&uri).cloned()) else { continue };
                bump(&uri);
                let at = versions.lock().unwrap()[&uri];
                let (out, versions) = (out.clone(), versions.clone());
                std::thread::spawn(move || {
                    let diags = layout(&path, &text);
                    if versions.lock().unwrap().get(&uri) == Some(&at) {
                        publish(&out, &uri, diags);
                    }
                });
            }
            "textDocument/definition" => reply(docs.get(&uri).map_or(Value::Null, |t| definition(t, &params["textDocument"]["uri"], &params["position"]))),
            "textDocument/completion" => reply(docs.get(&uri).map_or(Value::Array(Vec::new()), |t| completion(t, &params["position"]))),
            _ if msg.get("id").is_some() => send(&out, json!({ "jsonrpc": "2.0", "id": msg["id"], "error": { "code": -32601, "message": format!("unsupported method {method}") } })),
            _ => {}
        }
    }
    if shutdown { std::process::ExitCode::SUCCESS } else { std::process::ExitCode::FAILURE }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_count_utf16_units() {
        let text = "node a \"⟨𝔾⟩\"\nnode b\n";
        let off = text.find('⟩').unwrap();
        let p = position(text, off);
        assert_eq!(p, json!({ "line": 0, "character": 11 }));
        assert_eq!(offset(text, &p), off);
        assert_eq!(offset(text, &json!({ "line": 1, "character": 5 })), text.find("b\n").unwrap());
    }

    #[test]
    fn definitions_and_completions_follow_names() {
        let text = "style hot color=#C62828\nnode a .hot\nnode b\na -> b\nrequire a left-of \n";
        let uri = json!("file:///x.fitd");
        let at = |s: &str, k: usize| position(text, text.match_indices(s).nth(k).unwrap().0);
        assert_eq!(definition(text, &uri, &at("b", 1))["range"]["start"], json!({ "line": 2, "character": 5 }));
        assert_eq!(definition(text, &uri, &at("hot", 1))["range"]["start"], json!({ "line": 0, "character": 6 }));
        let labels = |v: Value| v.as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(labels(completion(text, &position(text, text.find("hot\nnode b").unwrap()))), ["hot"]);
        assert_eq!(labels(completion(text, &position(text, text.rfind('\n').unwrap()))), ["a", "b"]);
        assert!(labels(completion(text, &json!({ "line": 5, "character": 0 }))).contains(&"require".to_string()));
    }

    #[test]
    fn file_uris_decode() {
        assert_eq!(path_of("file:///home/a%20b/m.fitd").as_deref(), Some("/home/a b/m.fitd"));
        assert_eq!(path_of("file:///c%3A/Users/m.fitd").as_deref(), Some("c:/Users/m.fitd"));
        assert_eq!(path_of("file:///C:/m.fitd").as_deref(), Some("C:/m.fitd"));
        assert_eq!(path_of("untitled:1"), None);
    }
}
