//! Minimal language server (`fitd lsp`): front-end diagnostics on every change, full layout
//! diagnostics on save, go-to-definition and completion for node and style names.

use crate::ast::{Name, StmtKind};
use crate::diag::{Diag, Level, Source, Span};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

const KEYWORDS: &[&str] = &[
    "title", "root", "layout", "page", "font", "theme", "style", "node", "group", "require",
    "prefer", "include",
];

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
    // A malformed message is skipped, not taken for the end of input.
    Some(serde_json::from_slice(&body).unwrap_or(Value::Null))
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
    let (line, ch) = (
        pos["line"].as_u64().unwrap_or(0) as usize,
        pos["character"].as_u64().unwrap_or(0) as usize,
    );
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

const NOWHERE: Span = Span { lo: 0, hi: 0 };

/// spliced_range maps a span of the spliced text to its file and a range there. Spliced lines
/// are verbatim copies, so only line numbers change; a span that crosses files is cut to a point.
fn spliced_range(src: &Source, span: Span) -> (String, Value) {
    let at = |off: usize| {
        let (line, col) = src.line_col(off);
        let units: usize = src
            .line_text(line)
            .chars()
            .take(col - 1)
            .map(char::len_utf16)
            .sum();
        let (path, n) = src.file_line(line);
        (path, json!({ "line": n - 1, "character": units }))
    };
    let (path, start) = at(span.lo);
    let (end_path, end) = at(span.hi.max(span.lo));
    let end = if end_path == path { end } else { start.clone() };
    (path.to_string(), json!({ "start": start, "end": end }))
}

fn lsp_diag(range: Value, d: &Diag) -> Value {
    let mut msg = d.msg.clone();
    for n in &d.notes {
        msg += &format!("\nnote: {n}");
    }
    if let Some(h) = &d.help {
        msg += &format!("\nhelp: {h}");
    }
    json!({ "range": range, "severity": if d.level == Level::Error { 1 } else { 2 }, "code": d.code, "source": "fitd", "message": msg })
}

/// Diagnostics by file path, so those in included files go to their own documents. The
/// document's own path is always present, possibly empty.
type Diags = HashMap<String, Vec<Value>>;

/// front_end runs include, lex, parse, and sema, which include the static contradiction
/// check (E0008).
fn front_end(path: &str, text: &str) -> Diags {
    let crate::macros::Loaded {
        src,
        parse,
        mut diags,
    } = crate::macros::load(path, text, crate::macros::DEFAULT_BUDGET);
    let (stmts, parse_diags) = crate::parse::parse(&parse);
    diags.extend(parse_diags);
    diags.extend(crate::sema::analyze(&stmts).1);
    diags.retain(|d| !crate::include::unused_shared_style(&src, d));
    let mut out = Diags::from([(path.to_string(), Vec::new())]);
    for d in diags.iter().filter(|d| d.level != Level::Note) {
        let span = d
            .labels
            .iter()
            .find(|l| l.primary)
            .or(d.labels.first())
            .map_or(NOWHERE, |l| l.span);
        let (file, r) = spliced_range(&src, span);
        out.entry(file).or_default().push(lsp_diag(r, d));
    }
    out
}

/// short_line splits a `--short` line `file:line:col: message`; the file may hold colons.
fn short_line(line: &str) -> Option<(&str, usize, usize, &str)> {
    let mut from = 0;
    while let Some(i) = line[from..].find(": ").map(|i| from + i) {
        let mut it = line[..i].rsplitn(3, ':');
        if let (Some(c), Some(l), Some(f)) = (it.next(), it.next(), it.next())
            && let (Ok(l), Ok(c)) = (l.parse(), c.parse())
            && !f.is_empty()
        {
            return Some((f, l, c, &line[i + 2..]));
        }
        from = i + 2;
    }
    None
}

/// Layout children by document uri, so a newer save can kill the build it replaces.
type Running = Arc<Mutex<HashMap<String, std::process::Child>>>;

/// cancel kills the layout child running for `uri`, if any.
fn cancel(running: &Running, uri: &str) {
    if let Some(mut c) = running.lock().unwrap().remove(uri) {
        let _ = c.kill();
        let _ = c.wait();
    }
}

/// layout builds the saved file in a child process registered under `uri` and reads back its
/// `--short` diagnostics, so the server shares every check with the command line. None when
/// the build was cancelled or could not start.
fn layout(path: &str, text: &str, uri: &str, running: &Running) -> Option<Diags> {
    use std::io::Read;
    static RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let out = std::env::temp_dir().join(format!("fitd-lsp-{}-{n}.svg", std::process::id()));
    let exe = std::env::current_exe().ok()?;
    let mut child = std::process::Command::new(exe)
        .args([path, "-o", out.to_str()?, "--short"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    let (id, mut err) = (child.id(), child.stderr.take()?);
    cancel(running, uri);
    running.lock().unwrap().insert(uri.to_string(), child);
    let mut log = Vec::new();
    let _ = err.read_to_end(&mut log);
    // Still registered means nobody killed it; reap it here.
    let mine = {
        let mut r = running.lock().unwrap();
        if r.get(uri).is_some_and(|c| c.id() == id) {
            r.remove(uri)
        } else {
            None
        }
    };
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(format!("{}.fitd-tmp{id}", out.display()));
    let status = mine?.wait().ok()?;
    // Killed by a signal: no exit code, and the log is cut short.
    status.code()?;
    let mut texts = HashMap::from([(path.to_string(), Source::new(path, text))]);
    let mut diags = Diags::from([(path.to_string(), Vec::new())]);
    for line in String::from_utf8_lossy(&log).lines() {
        let (file, at, rest) = match short_line(line) {
            Some((f, ln, col, rest)) => (f.to_string(), Some((ln, col)), rest),
            None if line.starts_with("error: ") && !line.contains("aborting due to") => {
                (path.to_string(), None, line)
            }
            None => continue,
        };
        let severity = if rest.starts_with("error") { 1 } else { 2 };
        let code = rest
            .split_once('[')
            .and_then(|(_, r)| r.split_once(']'))
            .map_or("", |(c, _)| c);
        let msg = rest.split_once("]: ").map_or(rest, |(_, m)| m);
        let src = texts.entry(file.clone()).or_insert_with(|| {
            Source::new(&file, &std::fs::read_to_string(&file).unwrap_or_default())
        });
        // `--short` gives a line and column, so the mark runs from there to the end of the line.
        let r = match at {
            Some((ln, col)) if ln >= 1 && ln <= src.text.lines().count().max(1) => {
                let line_text = src.line_text(ln);
                let start_units: usize = line_text
                    .chars()
                    .take(col.saturating_sub(1))
                    .map(char::len_utf16)
                    .sum();
                json!({ "start": { "line": ln - 1, "character": start_units }, "end": { "line": ln - 1, "character": line_text.encode_utf16().count() } })
            }
            _ => range("", NOWHERE),
        };
        diags.entry(file).or_default().push(json!({ "range": r, "severity": severity, "code": code, "source": "fitd", "message": msg }));
    }
    Some(diags)
}

/// Decl is a declared name with its file and its range there.
struct Decl {
    name: String,
    path: String,
    range: Value,
}

/// declared lists node ids and style names, for definitions and completion, including those
/// declared in included files.
fn declared(path: &str, text: &str) -> (Vec<Decl>, Vec<Decl>) {
    let crate::macros::Loaded { src, parse, .. } =
        crate::macros::load(path, text, crate::macros::DEFAULT_BUDGET);
    let (stmts, _) = crate::parse::parse(&parse);
    let decl = |n: Name| {
        let (path, range) = spliced_range(&src, n.span);
        Decl {
            name: n.text,
            path,
            range,
        }
    };
    let (mut nodes, mut styles) = (Vec::new(), Vec::new());
    for st in stmts {
        match st.kind {
            StmtKind::Node { id, .. } => nodes.push(decl(id)),
            StmtKind::Style { name, .. } => styles.push(decl(name)),
            _ => {}
        }
    }
    (nodes, styles)
}

/// word_at finds the name under the cursor and whether a `.` marks it as a style.
fn word_at(text: &str, off: usize) -> Option<(String, bool)> {
    let is = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let lo = text[..off]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| is(c))
        .last()
        .map_or(off, |(i, _)| i);
    let hi = off
        + text[off..]
            .char_indices()
            .find(|&(_, c)| !is(c))
            .map_or(text.len() - off, |(i, _)| i);
    (lo < hi).then(|| (text[lo..hi].to_string(), text[..lo].ends_with('.')))
}

fn definition(path: &str, text: &str, uri: &Value, pos: &Value) -> Value {
    let Some((word, style)) = word_at(text, offset(text, pos)) else {
        return Value::Null;
    };
    let (nodes, styles) = declared(path, text);
    let pool = if style { styles } else { nodes };
    // The open file's own declaration wins over one in an include.
    let found = pool
        .iter()
        .find(|d| d.name == word && d.path == path)
        .or_else(|| pool.iter().find(|d| d.name == word));
    found.map_or(Value::Null, |d| json!({ "uri": if d.path == path { uri.clone() } else { json!(uri_of(&d.path)) }, "range": d.range }))
}

fn completion(path: &str, text: &str, pos: &Value) -> Value {
    let off = offset(text, pos);
    let line_start = text[..off].rfind('\n').map_or(0, |i| i + 1);
    let before = &text[line_start..off];
    let typed = before
        .rsplit(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .next()
        .unwrap_or("");
    let (nodes, styles) = declared(path, text);
    let mut seen = HashSet::new();
    let items: Vec<Value> = if before[..before.len() - typed.len()].ends_with('.') {
        styles
            .iter()
            .filter(|s| seen.insert(s.name.clone()))
            .map(|s| json!({ "label": s.name, "kind": 7 }))
            .collect()
    } else if before.trim() == typed {
        KEYWORDS
            .iter()
            .map(|k| json!({ "label": k, "kind": 14 }))
            .collect()
    } else {
        // A node may be referenced before its `node` line, so every declaration is offered.
        nodes
            .iter()
            .filter(|n| seen.insert(n.name.clone()))
            .map(|n| json!({ "label": n.name, "kind": 6 }))
            .collect()
    };
    Value::Array(items)
}

/// uri_of is the `file://` uri of a path, with `.` and `..` resolved and unsafe bytes escaped.
fn uri_of(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for c in path.split(['/', '\\']) {
        match c {
            "." => {}
            ".." if parts.last().is_some_and(|p| !p.is_empty() && *p != "..") => {
                parts.pop();
            }
            _ => parts.push(c),
        }
    }
    let p = parts.join("/");
    let mut s = String::from(if p.starts_with('/') {
        "file://"
    } else {
        "file:///"
    });
    for b in p.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/:".contains(&b) {
            s.push(b as char);
        } else {
            s += &format!("%{b:02X}");
        }
    }
    s
}

/// Other documents each open document last published diagnostics to, so an include whose
/// problems are fixed gets them cleared.
type Published = Arc<Mutex<HashMap<String, HashSet<String>>>>;

fn publish(out: &Out, uri: &str, diags: Vec<Value>) {
    send(
        out,
        json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": { "uri": uri, "diagnostics": diags } }),
    );
}

/// publish_all sends the diagnostics of the document at `uri` and of the files it includes,
/// and clears files it published to before that now have none.
fn publish_all(out: &Out, published: &Published, uri: &str, path: &str, mut diags: Diags) {
    publish(out, uri, diags.remove(path).unwrap_or_default());
    let mut others = HashSet::new();
    for (p, v) in diags {
        let u = uri_of(&p);
        publish(out, &u, v);
        others.insert(u);
    }
    let mut published = published.lock().unwrap();
    for stale in published
        .get(uri)
        .into_iter()
        .flatten()
        .filter(|u| !others.contains(*u))
    {
        publish(out, stale, Vec::new());
    }
    published.insert(uri.to_string(), others);
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
    let published: Published = Arc::default();
    // Uri to the change count when its last full layout started, so a stale result is dropped.
    let versions: Arc<Mutex<HashMap<String, u64>>> = Arc::default();
    let running: Running = Arc::default();
    let mut shutdown = false;
    while let Some(msg) = read(&mut input) {
        let method = msg["method"].as_str().unwrap_or("");
        let params = &msg["params"];
        let uri = params["textDocument"]["uri"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let reply = |result: Value| {
            send(
                &out,
                json!({ "jsonrpc": "2.0", "id": msg["id"], "result": result }),
            )
        };
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
                let text = if method == "textDocument/didOpen" {
                    &params["textDocument"]["text"]
                } else {
                    &params["contentChanges"][0]["text"]
                };
                let text = text.as_str().unwrap_or("").to_string();
                bump(&uri);
                let path = path_of(&uri).unwrap_or_default();
                publish_all(&out, &published, &uri, &path, front_end(&path, &text));
                docs.insert(uri, text);
            }
            "textDocument/didClose" => {
                docs.remove(&uri);
                bump(&uri);
                cancel(&running, &uri);
                publish_all(&out, &published, &uri, "", Diags::new());
            }
            "textDocument/didSave" => {
                let (Some(path), Some(text)) = (path_of(&uri), docs.get(&uri).cloned()) else {
                    continue;
                };
                bump(&uri);
                let at = versions.lock().unwrap()[&uri];
                let (out, versions, running, published) = (
                    out.clone(),
                    versions.clone(),
                    running.clone(),
                    published.clone(),
                );
                std::thread::spawn(move || {
                    let Some(diags) = layout(&path, &text, &uri, &running) else {
                        return;
                    };
                    if versions.lock().unwrap().get(&uri) == Some(&at) {
                        publish_all(&out, &published, &uri, &path, diags);
                    }
                });
            }
            "textDocument/definition" => reply(docs.get(&uri).map_or(Value::Null, |t| {
                definition(
                    &path_of(&uri).unwrap_or_default(),
                    t,
                    &params["textDocument"]["uri"],
                    &params["position"],
                )
            })),
            "textDocument/completion" => {
                reply(docs.get(&uri).map_or(Value::Array(Vec::new()), |t| {
                    completion(&path_of(&uri).unwrap_or_default(), t, &params["position"])
                }))
            }
            _ if msg.get("id").is_some() => send(
                &out,
                json!({ "jsonrpc": "2.0", "id": msg["id"], "error": { "code": -32601, "message": format!("unsupported method {method}") } }),
            ),
            _ => {}
        }
    }
    for (_, mut c) in running.lock().unwrap().drain() {
        let _ = c.kill();
        let _ = c.wait();
    }
    if shutdown {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
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
        assert_eq!(
            offset(text, &json!({ "line": 1, "character": 5 })),
            text.find("b\n").unwrap()
        );
    }

    #[test]
    fn definitions_and_completions_follow_names() {
        let text = "style hot color=#C62828\nnode a .hot\nnode b\na -> b\nrequire a left-of \n";
        let uri = json!("file:///x.fitd");
        let at = |s: &str, k: usize| position(text, text.match_indices(s).nth(k).unwrap().0);
        assert_eq!(
            definition("/x.fitd", text, &uri, &at("b", 1))["range"]["start"],
            json!({ "line": 2, "character": 5 })
        );
        assert_eq!(
            definition("/x.fitd", text, &uri, &at("hot", 1))["range"]["start"],
            json!({ "line": 0, "character": 6 })
        );
        let labels = |v: Value| {
            v.as_array()
                .unwrap()
                .iter()
                .map(|i| i["label"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            labels(completion(
                "/x.fitd",
                text,
                &position(text, text.find("hot\nnode b").unwrap())
            )),
            ["hot"]
        );
        assert_eq!(
            labels(completion(
                "/x.fitd",
                text,
                &position(text, text.rfind('\n').unwrap())
            )),
            ["a", "b"]
        );
        assert!(
            labels(completion(
                "/x.fitd",
                text,
                &json!({ "line": 5, "character": 0 })
            ))
            .contains(&"require".to_string())
        );
    }

    #[test]
    fn includes_lend_their_names_and_diagnostics() {
        let dir = std::env::temp_dir().join(format!("fitd-lsp-inc-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("parts")).unwrap();
        std::fs::write(
            dir.join("parts/more.fitd"),
            "style hot color=#EF5350\nnode far \"Far\" .hot\nfar -> ghost\n",
        )
        .unwrap();
        let main = dir.join("main.fitd");
        let path = main.to_str().unwrap();
        let text = "node a \"A\"\ninclude \"parts/more.fitd\"\na -> far\n";
        let uri = json!("file:///main");
        let def = definition(
            path,
            text,
            &uri,
            &position(text, text.rfind("far").unwrap()),
        );
        assert_eq!(
            def["uri"],
            json!(uri_of(dir.join("parts/more.fitd").to_str().unwrap()))
        );
        assert_eq!(def["range"]["start"], json!({ "line": 1, "character": 5 }));
        let labels = |v: Value| {
            v.as_array()
                .unwrap()
                .iter()
                .map(|i| i["label"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            labels(completion(path, text, &position(text, text.len() - 1))),
            ["a", "far"]
        );
        let diags = front_end(path, text);
        assert!(diags[path].is_empty(), "{diags:?}");
        let inc = &diags[dir.join("parts/more.fitd").to_str().unwrap()];
        assert!(
            inc.len() == 1
                && inc[0]["code"] == "E0001"
                && inc[0]["range"]["start"] == json!({ "line": 2, "character": 7 }),
            "{inc:?}"
        );
    }

    #[test]
    fn short_lines_split_and_uris_encode() {
        assert_eq!(
            short_line("C:/m/a.fitd:3:7: error[E0001]: x"),
            Some(("C:/m/a.fitd", 3, 7, "error[E0001]: x"))
        );
        assert_eq!(short_line("error: no room"), None);
        assert_eq!(
            uri_of("/home/a b/./parts/../m.fitd"),
            "file:///home/a%20b/m.fitd"
        );
        assert_eq!(uri_of("c:\\Users\\m.fitd"), "file:///c:/Users/m.fitd");
    }

    #[test]
    fn file_uris_decode() {
        assert_eq!(
            path_of("file:///home/a%20b/m.fitd").as_deref(),
            Some("/home/a b/m.fitd")
        );
        assert_eq!(
            path_of("file:///c%3A/Users/m.fitd").as_deref(),
            Some("c:/Users/m.fitd")
        );
        assert_eq!(path_of("file:///C:/m.fitd").as_deref(), Some("C:/m.fitd"));
        assert_eq!(path_of("untitled:1"), None);
    }
}
