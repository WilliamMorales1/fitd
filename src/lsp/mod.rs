//! Language server: diagnostics, navigation, hover, and editing aids over stdio.

mod docs;
mod features;
mod index;

use crate::diag::{Diag, Level, Source, Span};
use features::Doc;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

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
    Some(serde_json::from_slice(&body).unwrap_or(Value::Null))
}

pub(super) fn position(text: &str, off: usize) -> Value {
    let off = off.min(text.len());
    let line_start = text[..off].rfind('\n').map_or(0, |i| i + 1);
    let line = text[..line_start].matches('\n').count();
    json!({ "line": line, "character": text[line_start..off].encode_utf16().count() })
}

pub(super) fn offset(text: &str, pos: &Value) -> usize {
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
    let mut v = json!({ "range": range, "severity": if d.level == Level::Error { 1 } else { 2 }, "code": d.code, "source": "fitd", "message": msg });
    if d.code == "W0203" {
        v["tags"] = json!([1]);
    }
    if let Some(data) = features::diag_data(d) {
        v["data"] = data;
    }
    v
}

type Diags = HashMap<String, Vec<Value>>;

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
        let file_text = if file == path {
            Some(text.to_string())
        } else {
            crate::stdlib::at_path(&file)
                .map(str::to_string)
                .or_else(|| std::fs::read_to_string(&file).ok())
        };
        let r = anchor(file_text.as_deref(), r, d.msg.split('`').nth(1));
        let v = lsp_diag(r, d);
        // Each template expansion repeats its diagnostics at the same source spot.
        let list = out.entry(file).or_default();
        if !list.contains(&v) {
            list.push(v);
        }
    }
    out
}

/// anchor moves a range computed on template-expanded text onto the source line it came from.
fn anchor(file: Option<&str>, r: Value, name: Option<&str>) -> Value {
    let (Some(file), Some(name)) = (file, name) else {
        return r;
    };
    let n = r["start"]["line"].as_u64().unwrap_or(0) as usize;
    if r["end"]["line"] != r["start"]["line"] {
        return r;
    }
    let Some(line) = file.lines().nth(n) else {
        return r;
    };
    let (lo, hi) = (
        r["start"]["character"].as_u64().unwrap_or(0) as usize,
        r["end"]["character"].as_u64().unwrap_or(0) as usize,
    );
    if index::slice16(line, lo, hi) == Some(name) {
        return r;
    }
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let hit = line
        .match_indices(name)
        .find(|&(i, _)| !line[..i].ends_with(word) && !line[i + name.len()..].starts_with(word));
    let (lo, hi) = match hit {
        Some((i, _)) => {
            let lo = line[..i].encode_utf16().count();
            (lo, lo + name.encode_utf16().count())
        }
        None => {
            let lead = line.len() - line.trim_start().len();
            (lead, line.trim_end().encode_utf16().count())
        }
    };
    json!({ "start": { "line": n, "character": lo }, "end": { "line": n, "character": hi } })
}

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

type Running = Arc<Mutex<HashMap<String, std::process::Child>>>;

fn cancel(running: &Running, uri: &str) {
    if let Some(mut c) = running.lock().unwrap().remove(uri) {
        let _ = c.kill();
        let _ = c.wait();
    }
}

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

pub(super) fn uri_of(path: &str) -> String {
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
    let drive = path.len() > 2 && path.as_bytes()[2] == b':';
    Some(if drive { path[1..].to_string() } else { path })
}

type Published = Arc<Mutex<HashMap<String, HashSet<String>>>>;
type Cache = Arc<Mutex<HashMap<String, Vec<Value>>>>;

/// Shared is what diagnostics publishing needs from any thread.
#[derive(Clone)]
struct Shared {
    out: Out,
    published: Published,
    cache: Cache,
}

fn publish(sh: &Shared, uri: &str, diags: Vec<Value>) {
    sh.cache
        .lock()
        .unwrap()
        .insert(uri.to_string(), diags.clone());
    send(
        &sh.out,
        json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": { "uri": uri, "diagnostics": diags } }),
    );
}

fn publish_all(sh: &Shared, uri: &str, path: &str, mut diags: Diags) {
    publish(sh, uri, diags.remove(path).unwrap_or_default());
    let mut others = HashSet::new();
    for (p, v) in diags {
        let u = uri_of(&p);
        publish(sh, &u, v);
        others.insert(u);
    }
    let mut published = sh.published.lock().unwrap();
    for stale in published
        .get(uri)
        .into_iter()
        .flatten()
        .filter(|u| !others.contains(*u))
    {
        publish(sh, stale, Vec::new());
    }
    published.insert(uri.to_string(), others);
}

/// apply folds one didChange content change into the buffer.
fn apply(text: &mut String, change: &Value) {
    let new = change["text"].as_str().unwrap_or("");
    if change["range"].is_object() {
        let lo = offset(text, &change["range"]["start"]);
        let hi = offset(text, &change["range"]["end"]).max(lo);
        text.replace_range(lo..hi, new);
    } else {
        *text = new.to_string();
    }
}

fn capabilities() -> Value {
    json!({
        "positionEncoding": "utf-16",
        "textDocumentSync": { "openClose": true, "change": 2, "save": { "includeText": false } },
        "hoverProvider": true,
        "definitionProvider": true,
        "referencesProvider": true,
        "documentHighlightProvider": true,
        "documentSymbolProvider": true,
        "renameProvider": { "prepareProvider": true },
        "completionProvider": { "triggerCharacters": [".", "=", "\"", "/", " "] },
        "signatureHelpProvider": { "triggerCharacters": ["(", ","], "retriggerCharacters": [" "] },
        "inlayHintProvider": true,
        "codeActionProvider": { "codeActionKinds": ["quickfix"] },
        "foldingRangeProvider": true,
        "colorProvider": true,
    })
}

pub fn serve() -> std::process::ExitCode {
    let sh = Shared {
        out: Arc::new(Mutex::new(std::io::stdout())),
        published: Arc::default(),
        cache: Arc::default(),
    };
    let mut input = std::io::BufReader::new(std::io::stdin());
    let mut docs: HashMap<String, String> = HashMap::new();
    let versions: Arc<Mutex<HashMap<String, u64>>> = Arc::default();
    let running: Running = Arc::default();
    let mut snippets = false;
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
                &sh.out,
                json!({ "jsonrpc": "2.0", "id": msg["id"], "result": result }),
            )
        };
        let fail = |code: i64, message: String| {
            send(
                &sh.out,
                json!({ "jsonrpc": "2.0", "id": msg["id"], "error": { "code": code, "message": message } }),
            )
        };
        let bump = |uri: &str| *versions.lock().unwrap().entry(uri.to_string()).or_insert(0) += 1;
        let path = path_of(&uri).unwrap_or_default();
        let open = |p: &str| {
            docs.iter()
                .find(|(u, _)| path_of(u).as_deref() == Some(p))
                .map(|(_, t)| t.clone())
        };
        let with = |f: &dyn Fn(&Doc) -> Value, none: Value| match docs.get(&uri) {
            Some(text) => f(&Doc {
                uri: &uri,
                path: &path,
                text,
                open: &open,
            }),
            None => none,
        };
        let p = &params["position"];
        match method {
            "initialize" => {
                snippets = params["capabilities"]["textDocument"]["completion"]["completionItem"]
                    ["snippetSupport"]
                    .as_bool()
                    .unwrap_or(false);
                reply(json!({
                    "capabilities": capabilities(),
                    "serverInfo": { "name": "fitd", "version": env!("CARGO_PKG_VERSION") }
                }))
            }
            "shutdown" => {
                shutdown = true;
                reply(Value::Null);
            }
            "exit" => break,
            "textDocument/didOpen" | "textDocument/didChange" => {
                let text = if method == "textDocument/didOpen" {
                    params["textDocument"]["text"]
                        .as_str()
                        .unwrap_or("")
                        .to_string()
                } else {
                    let mut t = docs.remove(&uri).unwrap_or_default();
                    for c in params["contentChanges"].as_array().into_iter().flatten() {
                        apply(&mut t, c);
                    }
                    t
                };
                bump(&uri);
                publish_all(&sh, &uri, &path, front_end(&path, &text));
                docs.insert(uri, text);
            }
            "textDocument/didClose" => {
                docs.remove(&uri);
                bump(&uri);
                cancel(&running, &uri);
                publish_all(&sh, &uri, "", Diags::new());
            }
            "textDocument/didSave" => {
                let (Some(path), Some(text)) = (path_of(&uri), docs.get(&uri).cloned()) else {
                    continue;
                };
                bump(&uri);
                let at = versions.lock().unwrap()[&uri];
                let (sh, versions, running) = (sh.clone(), versions.clone(), running.clone());
                std::thread::spawn(move || {
                    let Some(diags) = layout(&path, &text, &uri, &running) else {
                        return;
                    };
                    if versions.lock().unwrap().get(&uri) == Some(&at) {
                        publish_all(&sh, &uri, &path, diags);
                    }
                });
            }
            "textDocument/hover" => {
                let diags = sh
                    .cache
                    .lock()
                    .unwrap()
                    .get(&uri)
                    .cloned()
                    .unwrap_or_default();
                reply(with(&|d| features::hover(d, p, &diags), Value::Null))
            }
            "textDocument/definition" => reply(with(&|d| features::definition(d, p), Value::Null)),
            "textDocument/references" => {
                let decl = params["context"]["includeDeclaration"]
                    .as_bool()
                    .unwrap_or(true);
                reply(with(&|d| features::references(d, p, decl), Value::Null))
            }
            "textDocument/documentHighlight" => {
                reply(with(&|d| features::highlights(d, p), Value::Null))
            }
            "textDocument/documentSymbol" => reply(with(&features::symbols, json!([]))),
            "textDocument/prepareRename" => {
                reply(with(&|d| features::prepare_rename(d, p), Value::Null))
            }
            "textDocument/rename" => {
                let new = params["newName"].as_str().unwrap_or("");
                let r = match docs.get(&uri) {
                    Some(text) => features::rename(
                        &Doc {
                            uri: &uri,
                            path: &path,
                            text,
                            open: &open,
                        },
                        p,
                        new,
                    ),
                    None => Ok(Value::Null),
                };
                match r {
                    Ok(v) => reply(v),
                    Err(e) => fail(-32602, e),
                }
            }
            "textDocument/completion" => {
                reply(with(&|d| features::completion(d, p, snippets), json!([])))
            }
            "textDocument/signatureHelp" => {
                reply(with(&|d| features::signature_help(d, p), Value::Null))
            }
            "textDocument/inlayHint" => {
                reply(with(&|d| features::inlay_hints(d, params), json!([])))
            }
            "textDocument/codeAction" => {
                reply(with(&|d| features::code_actions(d, params), json!([])))
            }
            "textDocument/foldingRange" => reply(with(&|d| features::folds(d.text), json!([]))),
            "textDocument/documentColor" => reply(with(&|d| features::colors(d.text), json!([]))),
            "textDocument/colorPresentation" => reply(features::color_presentation(params)),
            _ if msg.get("id").is_some() => fail(-32601, format!("unsupported method {method}")),
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

    fn doc<'a>(
        uri: &'a str,
        path: &'a str,
        text: &'a str,
        open: &'a dyn Fn(&str) -> Option<String>,
    ) -> Doc<'a> {
        Doc {
            uri,
            path,
            text,
            open,
        }
    }

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
        let text = "style hot color=#C62828\nhot\n  node a\nnode b\na -> b\nrequire a left-of \n";
        let none = |_: &str| None;
        let d = doc("file:///x.fitd", "/x.fitd", text, &none);
        let at = |s: &str, k: usize| position(text, text.match_indices(s).nth(k).unwrap().0);
        assert_eq!(
            features::definition(&d, &at("b", 1))["range"]["start"],
            json!({ "line": 3, "character": 5 })
        );
        assert_eq!(
            features::definition(&d, &at("hot", 1))["range"]["start"],
            json!({ "line": 0, "character": 6 })
        );
        let labels = |v: Value| {
            v.as_array()
                .unwrap()
                .iter()
                .map(|i| i["label"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        assert!(
            labels(features::completion(
                &d,
                &json!({ "line": 1, "character": 0 }),
                false
            ))
            .contains(&"hot".to_string())
        );
        assert_eq!(
            labels(features::completion(
                &d,
                &position(text, text.rfind('\n').unwrap()),
                false
            )),
            ["a", "b"]
        );
        assert!(
            labels(features::completion(
                &d,
                &json!({ "line": 7, "character": 0 }),
                false
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
            "style hot color=#EF5350\nhot\n  far \"Far\"\nfar -> ghost\n",
        )
        .unwrap();
        let main = dir.join("main.fitd");
        let path = main.to_str().unwrap();
        let text = "node a \"A\"\ninclude \"parts/more.fitd\"\na -> far\n";
        let none = |_: &str| None;
        let d = doc("file:///main", path, text, &none);
        let def = features::definition(&d, &position(text, text.rfind("far").unwrap()));
        assert_eq!(
            def["uri"],
            json!(uri_of(dir.join("parts/more.fitd").to_str().unwrap()))
        );
        assert_eq!(def["range"]["start"], json!({ "line": 2, "character": 2 }));
        let labels = |v: Value| {
            v.as_array()
                .unwrap()
                .iter()
                .map(|i| i["label"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            labels(features::completion(
                &d,
                &position(text, text.len() - 1),
                false
            )),
            ["a", "far"]
        );
        let diags = front_end(path, text);
        assert!(diags[path].is_empty(), "{diags:?}");
        let inc = &diags[dir.join("parts/more.fitd").to_str().unwrap()];
        assert!(
            inc.len() == 1
                && inc[0]["code"] == "E0001"
                && inc[0]["range"]["start"] == json!({ "line": 3, "character": 7 }),
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

    #[test]
    fn edits_apply_in_ranges() {
        let mut t = String::from("node a\nnode b\n");
        apply(
            &mut t,
            &json!({ "range": { "start": { "line": 1, "character": 5 }, "end": { "line": 1, "character": 6 } }, "text": "c \"C\"" }),
        );
        assert_eq!(t, "node a\nnode c \"C\"\n");
        apply(&mut t, &json!({ "text": "x" }));
        assert_eq!(t, "x");
    }

    #[test]
    fn modern_requests_answer() {
        let text = "style hot color=#C62828\n# Pair two nodes.\ndef pair(x, y)\n  node ${x}\n  node ${y}\nend\nhot\n  node a \"A\"\nnode b\na -> b\npair c d\n";
        let none = |_: &str| None;
        let d = doc("file:///x.fitd", "/x.fitd", text, &none);
        let at = |s: &str| position(text, text.rfind(s).unwrap());
        let h = features::hover(&d, &at("hot"), &[]);
        assert!(
            h["contents"]["value"]
                .as_str()
                .unwrap()
                .contains("style hot color=#C62828")
        );
        let h = features::hover(&d, &at("pair"), &[]);
        assert!(
            h["contents"]["value"]
                .as_str()
                .unwrap()
                .contains("Pair two nodes.")
        );
        assert_eq!(
            features::references(&d, &at("a ->"), true)
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let r = features::rename(&d, &at("b\n"), "bee").unwrap();
        assert_eq!(r["changes"]["file:///x.fitd"].as_array().unwrap().len(), 2);
        assert!(features::rename(&d, &at("b\n"), "1x").is_err());
        let sig = features::signature_help(&d, &json!({ "line": 10, "character": 6 }));
        assert_eq!(sig["signatures"][0]["label"], "pair x y");
        assert_eq!(sig["activeParameter"], 0);
        let hints = features::inlay_hints(
            &d,
            &json!({ "range": { "start": { "line": 0 }, "end": { "line": 11 } } }),
        );
        assert_eq!(hints[1]["label"], "y:");
        assert_eq!(
            features::folds(text)[0],
            json!({ "startLine": 2, "endLine": 5 })
        );
        assert_eq!(features::colors(text)[0]["range"]["start"]["character"], 16);
        let syms = features::symbols(&d);
        let names: Vec<&str> = syms
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert!(
            names.contains(&"pair") && names.contains(&"a") && names.contains(&"hot"),
            "{names:?}"
        );
    }

    #[test]
    fn module_classes_answer_by_qualified_name() {
        let text = "style nn.block color=#CC79A7\nnn.block\n  a\n";
        let none = |_: &str| None;
        let d = doc("file:///x.fitd", "/x.fitd", text, &none);
        let at = position(text, text.find("nn.block").unwrap() + 3);
        assert_eq!(
            features::definition(&d, &at)["range"]["start"],
            json!({ "line": 0, "character": 6 })
        );
    }

    #[test]
    fn style_blocks_answer_like_classes() {
        let text = "style step color=#0072B2\nstyle nn.block color=#CC79A7\nstep\n  a \"A\"\n  | body\n  nn.block\n    e\n  b \n\n";
        let none = |_: &str| None;
        let d = doc("file:///x.fitd", "/x.fitd", text, &none);
        let at = |s: &str| position(text, text.find(s).unwrap() + 1);
        assert_eq!(
            features::definition(&d, &at("step\n"))["range"]["start"],
            json!({ "line": 0, "character": 6 })
        );
        assert_eq!(
            features::definition(&d, &at("nn.block\n"))["range"]["start"],
            json!({ "line": 1, "character": 6 })
        );
        let h = features::hover(&d, &at("step\n"), &[]);
        assert!(
            h["contents"]["value"]
                .as_str()
                .unwrap()
                .contains("style step"),
            "{h}"
        );
        let labels = |line: u64, ch: u64| -> Vec<String> {
            features::completion(&d, &json!({ "line": line, "character": ch }), false)
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v["label"].as_str().unwrap().to_string())
                .collect()
        };
        let start = labels(8, 0);
        assert!(
            start.iter().any(|l| l == "step") && start.iter().any(|l| l == "nn.block"),
            "{start:?}"
        );
        assert!(labels(7, 4).iter().any(|l| l == "color"));
        let folds = features::folds(text);
        for (lo, hi) in [(3, 4), (5, 6), (2, 7)] {
            assert!(
                folds
                    .as_array()
                    .unwrap()
                    .contains(&json!({ "startLine": lo, "endLine": hi })),
                "{folds}"
            );
        }
    }

    #[test]
    fn module_templates_answer_by_qualified_name() {
        let text = "include \"nn\"\nnn.layer e Enc d 2\nnn.\n";
        let none = |_: &str| None;
        let d = doc("file:///m.fitd", "/m.fitd", text, &none);
        let h = features::hover(&d, &json!({ "line": 1, "character": 4 }), &[]);
        assert!(
            h["contents"]["value"]
                .as_str()
                .unwrap()
                .contains("nn.layer id"),
            "{h}"
        );
        let sig = features::signature_help(&d, &json!({ "line": 1, "character": 12 }));
        assert_eq!(sig["signatures"][0]["label"], "nn.layer id name dims times");
        assert_eq!(sig["activeParameter"], 1);
        let items = features::completion(&d, &json!({ "line": 2, "character": 3 }), false);
        let labels: Vec<&str> = items
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["label"].as_str().unwrap())
            .collect();
        assert!(
            labels.contains(&"input") && labels.contains(&"layer"),
            "{labels:?}"
        );
        let rows = "include \"nn\"\nnn.layer\n  e Enc d 2\nnn.step s Pool\n";
        let d = doc("file:///r.fitd", "/r.fitd", rows, &none);
        for line in [1, 3] {
            let h = features::hover(&d, &json!({ "line": line, "character": 4 }), &[]);
            assert!(
                h["contents"]["value"].as_str().unwrap().contains("nn."),
                "{h}"
            );
        }
    }
}
