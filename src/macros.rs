//! Macros: `for`, `if`, `def`, `use`, and `${expr}` expand to plain statements after
//! `include` and before parsing. Every input line appears once in the expanded text as its raw
//! copy, blanked for the parser when it is a directive, so diagnostics can point at it; each
//! generated line maps back to the template line it came from.

use crate::diag::{Diag, Seg, Source, Span};
use std::collections::HashMap;

/// DEFAULT_BUDGET caps expansion steps (directives run plus lines generated).
pub const DEFAULT_BUDGET: u64 = 100_000;
const MAX_DEPTH: usize = 200;

/// Loaded is a file after include splicing and macro expansion.
pub struct Loaded {
    /// src is what diagnostics render against: raw directive lines stay visible here.
    pub src: Source,
    /// parse is `src.text` with directive lines blanked, byte for byte, so spans agree.
    pub parse: String,
    pub diags: Vec<Diag>,
}

/// load splices includes into `text`, read from `path`, then expands its macros within
/// `budget` steps.
pub fn load(path: &str, text: &str, budget: u64) -> Loaded {
    let (a, inc) = crate::include::expand(path, text);
    expand(a, inc, budget)
}

/// expand_text expands macros in `text` with no include splicing.
#[cfg(test)]
pub fn expand_text(path: &str, text: &str, budget: u64) -> Loaded {
    expand(Source::new(path, text), Vec::new(), budget)
}

#[derive(Clone, Debug, PartialEq)]
enum Val {
    Int(i64),
    Str(String),
}

impl std::fmt::Display for Val {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Val::Int(n) => write!(f, "{n}"),
            Val::Str(s) => f.write_str(s),
        }
    }
}

impl Val {
    fn truthy(&self) -> bool {
        match self {
            Val::Int(n) => *n != 0,
            Val::Str(s) => !s.is_empty(),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Val::Int(_) => "a number",
            Val::Str(_) => "a string",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum T {
    Int(i64),
    Str(String),
    Id(String),
    Op(&'static str),
}

const OPS: [&str; 16] = ["..", "==", "!=", "<=", ">=", "+", "-", "*", "/", "%", "<", ">", "(", ")", ",", "="];

fn tokens(s: &str) -> Result<Vec<T>, String> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    'outer: while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '#' {
            break;
        } else if c.is_ascii_digit() {
            let lo = i;
            while i < cs.len() && cs[i].is_ascii_digit() {
                i += 1;
            }
            let w: String = cs[lo..i].iter().collect();
            out.push(T::Int(w.parse().map_err(|_| format!("number `{w}` is too large"))?));
        } else if c.is_alphabetic() || c == '_' {
            let lo = i;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            out.push(T::Id(cs[lo..i].iter().collect()));
        } else if c == '"' {
            let mut s = String::new();
            i += 1;
            loop {
                match cs.get(i) {
                    None => return Err("unterminated string".into()),
                    Some('"') => break,
                    Some('\\') if i + 1 < cs.len() => {
                        s.push(cs[i + 1]);
                        i += 2;
                    }
                    Some(&o) => {
                        s.push(o);
                        i += 1;
                    }
                }
            }
            i += 1;
            out.push(T::Str(s));
        } else {
            for op in OPS {
                if op.chars().enumerate().all(|(k, oc)| cs.get(i + k) == Some(&oc)) {
                    out.push(T::Op(op));
                    i += op.len();
                    continue 'outer;
                }
            }
            return Err(format!("unexpected `{c}` in an expression"));
        }
    }
    Ok(out)
}

#[derive(Clone, Debug)]
enum Expr {
    Lit(Val),
    Var(String),
    Bin(&'static str, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

struct P {
    t: Vec<T>,
    i: usize,
}

impl P {
    fn peek(&self) -> Option<&T> {
        self.t.get(self.i)
    }

    fn op(&mut self, o: &str) -> bool {
        if self.peek() == Some(&T::Op(match OPS.iter().find(|x| **x == o) {
            Some(x) => x,
            None => return false,
        })) {
            self.i += 1;
            return true;
        }
        false
    }

    fn word(&mut self, w: &str) -> bool {
        if matches!(self.peek(), Some(T::Id(x)) if x == w) {
            self.i += 1;
            return true;
        }
        false
    }

    fn name(&mut self, what: &str) -> Result<String, String> {
        match self.peek().cloned() {
            Some(T::Id(x)) => {
                self.i += 1;
                Ok(x)
            }
            _ => Err(format!("expected {what}")),
        }
    }

    fn expect(&mut self, o: &str) -> Result<(), String> {
        if self.op(o) { Ok(()) } else { Err(format!("expected `{o}`")) }
    }

    fn done(&self) -> Result<(), String> {
        match self.peek() {
            None => Ok(()),
            Some(t) => Err(format!("unexpected {} after expression", show(t))),
        }
    }

    fn expr(&mut self) -> Result<Expr, String> {
        let mut a = self.and()?;
        while self.word("or") {
            a = Expr::Bin("or", Box::new(a), Box::new(self.and()?));
        }
        Ok(a)
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut a = self.compare()?;
        while self.word("and") {
            a = Expr::Bin("and", Box::new(a), Box::new(self.compare()?));
        }
        Ok(a)
    }

    fn compare(&mut self) -> Result<Expr, String> {
        let a = self.sum()?;
        for o in ["==", "!=", "<=", ">=", "<", ">"] {
            if self.op(o) {
                let op = OPS.iter().find(|x| **x == o).unwrap();
                return Ok(Expr::Bin(op, Box::new(a), Box::new(self.sum()?)));
            }
        }
        Ok(a)
    }

    fn sum(&mut self) -> Result<Expr, String> {
        let mut a = self.product()?;
        loop {
            let op = if self.op("+") { "+" } else if self.op("-") { "-" } else { return Ok(a) };
            a = Expr::Bin(op, Box::new(a), Box::new(self.product()?));
        }
    }

    fn product(&mut self) -> Result<Expr, String> {
        let mut a = self.unary()?;
        loop {
            let op = if self.op("*") { "*" } else if self.op("/") { "/" } else if self.op("%") { "%" } else { return Ok(a) };
            a = Expr::Bin(op, Box::new(a), Box::new(self.unary()?));
        }
    }

    fn unary(&mut self) -> Result<Expr, String> {
        match self.peek().cloned() {
            Some(T::Int(n)) => {
                self.i += 1;
                Ok(Expr::Lit(Val::Int(n)))
            }
            Some(T::Str(s)) => {
                self.i += 1;
                Ok(Expr::Lit(Val::Str(s)))
            }
            Some(T::Id(x)) if !matches!(x.as_str(), "and" | "or") => {
                self.i += 1;
                if self.op("(") {
                    return Ok(Expr::Call(x, self.args()?));
                }
                Ok(Expr::Var(x))
            }
            Some(T::Op("(")) => {
                self.i += 1;
                let e = self.expr()?;
                self.expect(")")?;
                Ok(e)
            }
            Some(t) => Err(format!("expected a value, found {}", show(&t))),
            None => Err("expected a value, found end of line".into()),
        }
    }

    /// args reads a comma list up to `)`, after the `(` is consumed.
    fn args(&mut self) -> Result<Vec<Expr>, String> {
        let mut out = Vec::new();
        if self.op(")") {
            return Ok(out);
        }
        loop {
            out.push(self.expr()?);
            if self.op(")") {
                return Ok(out);
            }
            self.expect(",")?;
        }
    }
}

fn show(t: &T) -> String {
    match t {
        T::Int(n) => format!("`{n}`"),
        T::Str(s) => format!("\"{s}\""),
        T::Id(x) => format!("`{x}`"),
        T::Op(o) => format!("`{o}`"),
    }
}

fn parse_expr(s: &str) -> Result<Expr, String> {
    let mut p = P { t: tokens(s)?, i: 0 };
    let e = p.expr()?;
    p.done()?;
    Ok(e)
}

#[derive(Clone, Debug)]
enum Piece {
    Lit(String),
    Expr(Expr),
}

/// pieces splits a line at each `${expr}`; `$${` is a literal `${`. None means no interpolation.
fn pieces(line: &str) -> Result<Option<Vec<Piece>>, String> {
    if !line.contains("${") {
        return Ok(None);
    }
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut rest = line;
    while let Some(k) = rest.find("${") {
        if rest[..k].ends_with('$') {
            lit += &rest[..k - 1];
            lit += "${";
            rest = &rest[k + 2..];
            continue;
        }
        lit += &rest[..k];
        let body = &rest[k + 2..];
        let mut in_str = false;
        let mut end = None;
        let mut prev = ' ';
        for (j, c) in body.char_indices() {
            match c {
                '"' if prev != '\\' => in_str = !in_str,
                '}' if !in_str => {
                    end = Some(j);
                    break;
                }
                _ => {}
            }
            prev = c;
        }
        let end = end.ok_or("`${` without a closing `}`")?;
        out.push(Piece::Lit(std::mem::take(&mut lit)));
        out.push(Piece::Expr(parse_expr(&body[..end])?));
        rest = &body[end + 1..];
    }
    lit += rest;
    out.push(Piece::Lit(lit));
    Ok(Some(out))
}

/// comment_start finds where a statement line's comment begins, as the lexer does: `#` outside
/// strings and `${...}`, and not right after `=`, where it starts a color.
fn comment_start(line: &str) -> usize {
    let (mut in_str, mut depth, mut prev) = (false, 0, ' ');
    let mut it = line.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        match c {
            '\\' if in_str => {
                it.next();
            }
            '"' => in_str = !in_str,
            '$' if !in_str && it.peek().is_some_and(|&(_, n)| n == '{') => {
                it.next();
                depth += 1;
            }
            '}' if depth > 0 && !in_str => depth -= 1,
            '#' if !in_str && depth == 0 && prev != '=' => return i,
            _ => {}
        }
        prev = c;
    }
    line.len()
}

#[derive(Clone, Debug)]
enum Iter {
    Range(Expr, Expr),
    List(Vec<Val>),
}

#[derive(Clone, Debug)]
enum Kind {
    /// A line copied as is: a statement, comment, or blank.
    Plain,
    Text(Vec<Piece>),
    /// `for x in ...` or `for i, x in ...`; the last name takes the items, the first counts from 1.
    For(Vec<String>, Iter, Vec<Item>),
    /// Arms in order; a `None` condition is `else`.
    If(Vec<(Option<Expr>, Vec<Item>)>),
    Use(String, Vec<Expr>),
    /// A `def` block, or a line that failed to parse; neither emits anything.
    Skip,
}

#[derive(Clone, Debug)]
struct Item {
    /// lo and hi are the first and last input lines (0-based) the item spans.
    lo: usize,
    hi: usize,
    kind: Kind,
}

struct Def {
    params: Vec<String>,
    body: Vec<Item>,
    line: usize,
}

enum Line {
    Item(Kind),
    For(Vec<String>, Iter),
    If(Expr),
    ElseIf(Expr),
    Else,
    End,
    Def(String, Vec<String>),
}

fn is_kw_line(t: &str) -> Option<(&str, &str)> {
    let w = t.split(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or("");
    if !matches!(w, "for" | "if" | "else" | "end" | "def" | "use") {
        return None;
    }
    let rest = &t[w.len()..];
    let r = rest.trim_start();
    // `end -> x` is an edge from a node named `end`; `end-x` is a hyphenated name.
    if rest.starts_with('-') || r.starts_with("->") || r.starts_with("--") || r.starts_with("..") {
        return None;
    }
    Some((w, r))
}

fn classify(line: &str) -> Result<Line, String> {
    let t = line.trim();
    if t.starts_with('#') || t.starts_with('|') && !t.contains("${") {
        return Ok(Line::Item(Kind::Plain));
    }
    let Some((w, rest)) = is_kw_line(t) else {
        // A trailing comment is copied as is, so `${` in it is just text.
        let cut = if t.starts_with('|') { line.len() } else { comment_start(line) };
        return Ok(Line::Item(match pieces(&line[..cut])? {
            Some(mut p) => {
                p.push(Piece::Lit(line[cut..].to_string()));
                Kind::Text(p)
            }
            None => Kind::Plain,
        }));
    };
    if w == "for" {
        return for_line(rest);
    }
    let mut p = P { t: tokens(rest)?, i: 0 };
    Ok(match w {
        "end" => {
            p.done()?;
            Line::End
        }
        "else" => {
            if p.word("if") {
                let e = p.expr()?;
                p.done()?;
                Line::ElseIf(e)
            } else {
                p.done()?;
                Line::Else
            }
        }
        "if" => {
            let e = p.expr()?;
            p.done()?;
            Line::If(e)
        }
        "def" => {
            let name = p.name("a template name: `def NAME(a, b)`")?;
            p.expect("(")?;
            let mut params = Vec::new();
            if !p.op(")") {
                loop {
                    params.push(p.name("a parameter name")?);
                    if p.op(")") {
                        break;
                    }
                    p.expect(",")?;
                }
            }
            p.done()?;
            Line::Def(name, params)
        }
        _ => {
            let name = p.name("a template name: `use NAME(args)`")?;
            p.expect("(")?;
            let args = p.args()?;
            p.done()?;
            Line::Item(Kind::Use(name, args))
        }
    })
}

const FOR_SHAPE: &str = "`for NAME in LO..HI`, `for NAME in a, b, c`, or `for I, NAME in a, b, c`";

fn for_line(rest: &str) -> Result<Line, String> {
    let padded = format!(" {rest} ");
    let Some(k) = padded.find(" in ") else { return Err(format!("expected `in`: {FOR_SHAPE}")) };
    let vars: Vec<String> = padded[..k].split(',').map(|v| v.trim().to_string()).collect();
    let ok = |v: &String| !v.is_empty() && v.chars().all(|c| c.is_alphanumeric() || c == '_') && !v.starts_with(|c: char| c.is_ascii_digit());
    if vars.len() > 2 || !vars.iter().all(ok) {
        return Err(format!("expected one or two variable names: {FOR_SHAPE}"));
    }
    let var = vars;
    let items = padded[k + 4..].trim();
    if let Ok(t) = tokens(items)
        && t.contains(&T::Op(".."))
    {
        let mut p = P { t, i: 0 };
        let lo = p.sum()?;
        p.expect("..")?;
        let hi = p.sum()?;
        p.done()?;
        return Ok(Line::For(var, Iter::Range(lo, hi)));
    }
    Ok(Line::For(var, Iter::List(list(items)?)))
}

/// list reads `for` items: words (hyphens allowed), numbers, or quoted strings.
fn list(s: &str) -> Result<Vec<Val>, String> {
    let mut out = Vec::new();
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() || c == ',' {
            i += 1;
        } else if c == '#' {
            break;
        } else if c == '"' {
            let mut s = String::new();
            i += 1;
            loop {
                match cs.get(i) {
                    None => return Err("unterminated string".into()),
                    Some('"') => break,
                    Some('\\') if i + 1 < cs.len() => {
                        s.push(cs[i + 1]);
                        i += 2;
                    }
                    Some(&o) => {
                        s.push(o);
                        i += 1;
                    }
                }
            }
            i += 1;
            out.push(Val::Str(s));
        } else {
            let lo = i;
            while i < cs.len() && !(cs[i].is_whitespace() || cs[i] == ',' || cs[i] == '#') {
                i += 1;
            }
            let w: String = cs[lo..i].iter().collect();
            out.push(match w.parse() {
                Ok(n) => Val::Int(n),
                Err(_) => Val::Str(w),
            });
        }
    }
    if out.is_empty() {
        return Err("`for ... in` needs a range `LO..HI` or a list of items".into());
    }
    Ok(out)
}

/// Builder turns input lines into items; errors carry the input line they belong to.
struct Builder<'a> {
    lines: &'a [&'a str],
    i: usize,
    defs: HashMap<String, Def>,
    errs: Vec<(usize, Diag)>,
}

enum Stop {
    Eof,
    End,
    Else(Option<Expr>),
}

impl Builder<'_> {
    fn err(&mut self, line: usize, msg: impl Into<String>) {
        self.errs.push((line, Diag::error("E0002", msg)));
    }

    fn block(&mut self, top: bool) -> (Vec<Item>, Stop, usize) {
        let mut items = Vec::new();
        while self.i < self.lines.len() {
            let at = self.i;
            self.i += 1;
            let line = match classify(self.lines[at]) {
                Ok(l) => l,
                Err(m) => {
                    self.err(at, m);
                    items.push(Item { lo: at, hi: at, kind: Kind::Skip });
                    continue;
                }
            };
            let kind = match line {
                Line::Item(k) => k,
                Line::End | Line::Else | Line::ElseIf(_) if top => {
                    self.err(at, "`end` or `else` with no open `for`, `if`, or `def`");
                    Kind::Skip
                }
                Line::End => return (items, Stop::End, at),
                Line::Else => return (items, Stop::Else(None), at),
                Line::ElseIf(e) => return (items, Stop::Else(Some(e)), at),
                Line::For(var, it) => {
                    let (body, stop, _) = self.block(false);
                    self.close(at, &stop, "for");
                    Kind::For(var, it, body)
                }
                Line::If(cond) => {
                    let mut arms = Vec::new();
                    let mut cond = Some(cond);
                    loop {
                        let (body, stop, end) = self.block(false);
                        let had_else = cond.is_none();
                        arms.push((cond.take(), body));
                        match stop {
                            Stop::Else(next) if !had_else => cond = next,
                            Stop::Else(_) => self.err(end, "`else` after `else`"),
                            Stop::End => break,
                            Stop::Eof => {
                                self.close(at, &Stop::Eof, "if");
                                break;
                            }
                        }
                    }
                    Kind::If(arms)
                }
                Line::Def(name, params) => {
                    let (body, stop, _) = self.block(false);
                    self.close(at, &stop, "def");
                    if !top {
                        self.err(at, "`def` is allowed only at the top level");
                    } else if let Some(old) = self.defs.get(&name) {
                        let line = old.line;
                        self.errs.push((at, Diag::error("E0012", format!("template `{name}` is defined twice")).note(format!("first defined at {}", at_marker(line)))));
                    } else {
                        self.defs.insert(name, Def { params, body, line: at });
                    }
                    Kind::Skip
                }
            };
            items.push(Item { lo: at, hi: self.i - 1, kind });
        }
        (items, Stop::Eof, self.lines.len())
    }

    fn close(&mut self, at: usize, stop: &Stop, what: &str) {
        match stop {
            Stop::End => {}
            Stop::Eof => self.err(at, format!("`{what}` has no matching `end`")),
            Stop::Else(_) => self.err(at, format!("`else` inside `{what}` belongs to no `if`")),
        }
    }
}

/// Expander runs items, writing the expanded text.
struct Expander<'a> {
    lines: &'a [&'a str],
    defs: &'a HashMap<String, Def>,
    text: String,
    parse: String,
    /// origin is the input line (0-based) of each output line.
    origin: Vec<usize>,
    /// raw is where each input line's raw copy starts in the output.
    raw: Vec<usize>,
    frames: Vec<HashMap<String, Val>>,
    calls: Vec<(String, usize)>,
    steps: u64,
    budget: u64,
    out_of_budget: bool,
    errs: Vec<(usize, Diag)>,
}

type Run<T> = Result<T, ()>;

impl Expander<'_> {
    fn emit(&mut self, line: usize, s: &str, hidden: bool) {
        self.text += s;
        self.text.push('\n');
        if hidden {
            self.parse.extend(std::iter::repeat_n(' ', s.len()));
        } else {
            self.parse += s;
        }
        self.parse.push('\n');
        self.origin.push(line);
    }

    fn raw(&mut self, item: &Item) {
        for k in item.lo..=item.hi {
            self.raw[k] = self.text.len();
            let line = self.lines[k];
            self.emit(k, line, !matches!(item.kind, Kind::Plain));
        }
    }

    fn fail(&mut self, line: usize, msg: impl Into<String>) {
        let mut d = Diag::error("E0012", msg);
        for (name, at) in self.calls.iter().rev().take(4) {
            d = d.note(format!("in `use {name}` at {}", at_marker(*at)));
        }
        self.errs.push((line, d));
    }

    fn tick(&mut self, line: usize) -> Run<()> {
        self.steps += 1;
        if self.steps > self.budget {
            if !self.out_of_budget {
                self.out_of_budget = true;
                let d = Diag::error("E0013", format!("macro expansion ran past {} steps", self.budget));
                self.errs.push((line, d.help("check for runaway recursion or a huge `for` range, or raise `--expand-budget`")));
            }
            return Err(());
        }
        Ok(())
    }

    fn get(&self, name: &str) -> Option<&Val> {
        self.frames.last().unwrap().get(name).or_else(|| self.frames[0].get(name))
    }

    fn eval(&mut self, line: usize, e: &Expr) -> Run<Val> {
        let int = |me: &mut Self, v: Val, what: &str| match v {
            Val::Int(n) => Ok(n),
            o => {
                me.fail(line, format!("{what} needs a number, found {} \"{o}\"", o.kind()));
                Err(())
            }
        };
        Ok(match e {
            Expr::Lit(v) => v.clone(),
            Expr::Var(x) => match self.get(x) {
                Some(v) => v.clone(),
                None => {
                    let mut known: Vec<&String> = self.frames.last().unwrap().keys().chain(self.frames[0].keys()).collect();
                    known.sort();
                    known.dedup();
                    let d = if known.is_empty() { String::new() } else { format!(" (known: {})", known.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", ")) };
                    self.fail(line, format!("unknown variable `{x}`{d}"));
                    return Err(());
                }
            },
            Expr::Bin("and", a, b) => {
                let x = self.eval(line, a)?;
                if x.truthy() { self.eval(line, b)? } else { x }
            }
            Expr::Bin("or", a, b) => {
                let x = self.eval(line, a)?;
                if x.truthy() { x } else { self.eval(line, b)? }
            }
            Expr::Bin(op, a, b) => {
                let (x, y) = (self.eval(line, a)?, self.eval(line, b)?);
                match *op {
                    "==" => Val::Int((x == y) as i64),
                    "!=" => Val::Int((x != y) as i64),
                    "+" if matches!((&x, &y), (Val::Str(_), _) | (_, Val::Str(_))) => Val::Str(format!("{x}{y}")),
                    _ => {
                        let (m, n) = (int(self, x, &format!("`{op}`"))?, int(self, y, &format!("`{op}`"))?);
                        let r = match *op {
                            "+" => m.checked_add(n),
                            "-" => m.checked_sub(n),
                            "*" => m.checked_mul(n),
                            "/" | "%" if n == 0 => {
                                self.fail(line, "division by zero");
                                return Err(());
                            }
                            "/" => m.checked_div_euclid(n),
                            "%" => m.checked_rem_euclid(n),
                            _ => Some(cmp(op, m.cmp(&n)) as i64),
                        };
                        match r {
                            Some(v) => Val::Int(v),
                            None => {
                                self.fail(line, format!("`{op}` overflows"));
                                return Err(());
                            }
                        }
                    }
                }
            }
            Expr::Call(f, args) => {
                let mut vs = Vec::new();
                for a in args {
                    vs.push(self.eval(line, a)?);
                }
                self.call(line, f, vs)?
            }
        })
    }

    fn call(&mut self, line: usize, f: &str, vs: Vec<Val>) -> Run<Val> {
        let arity = match f {
            "names" => 3,
            _ => {
                let hint = if self.defs.contains_key(f) { format!("; `{f}` is a template, so write `use {f}(...)` on its own line") } else { String::new() };
                self.fail(line, format!("unknown function `{f}`{hint}"));
                return Err(());
            }
        };
        if vs.len() != arity {
            self.fail(line, format!("`{f}` takes {arity} arguments, found {}", vs.len()));
            return Err(());
        }
        let num = |me: &mut Self, v: &Val| match v {
            Val::Int(n) => Ok(*n),
            o => {
                me.fail(line, format!("`{f}` needs a number, found {} \"{o}\"", o.kind()));
                Err(())
            }
        };
        let (lo, hi) = (num(self, &vs[1])?, num(self, &vs[2])?);
        let mut s = String::new();
        for k in lo..=hi {
            self.tick(line)?;
            if k > lo {
                s += ", ";
            }
            s += &format!("{}{k}", vs[0]);
        }
        Ok(Val::Str(s))
    }

    fn run(&mut self, items: &[Item], top: bool) -> Run<()> {
        for (k, item) in items.iter().enumerate() {
            if top {
                self.raw(item);
            }
            if self.item(item, top).is_err() && self.out_of_budget {
                // Later lines still need raw copies, so diagnostics can point at them.
                if top {
                    items[k + 1..].iter().for_each(|it| self.raw(it));
                }
                return Err(());
            }
        }
        Ok(())
    }

    fn item(&mut self, item: &Item, top: bool) -> Run<()> {
        let at = item.lo;
        self.tick(at)?;
        match &item.kind {
            Kind::Plain if !top => {
                let line = self.lines[at];
                self.emit(at, line, false);
            }
            Kind::Plain | Kind::Skip => {}
            Kind::Text(ps) => {
                let mut s = String::new();
                for p in ps {
                    match p {
                        Piece::Lit(l) => s += l,
                        Piece::Expr(e) => s += &self.eval(at, e)?.to_string(),
                    }
                }
                if s.contains('\n') {
                    self.fail(at, "an interpolated value holds a line break");
                    return Err(());
                }
                self.emit(at, &s, false);
            }
            Kind::For(var, it, body) => {
                let vals: Box<dyn Iterator<Item = Val>> = match it {
                    Iter::Range(lo, hi) => {
                        let lo = self.eval(at, lo)?;
                        let hi = self.eval(at, hi)?;
                        match (lo, hi) {
                            (Val::Int(a), Val::Int(b)) => Box::new((a..=b).map(Val::Int)),
                            (a, b) => {
                                self.fail(at, format!("a range needs numbers, found {} and {}", a.kind(), b.kind()));
                                return Err(());
                            }
                        }
                    }
                    Iter::List(vs) => Box::new(vs.clone().into_iter()),
                };
                for (k, v) in vals.enumerate() {
                    self.tick(at)?;
                    let frame = self.frames.last_mut().unwrap();
                    if let [i, _] = &var[..] {
                        frame.insert(i.clone(), Val::Int(k as i64 + 1));
                    }
                    frame.insert(var.last().unwrap().clone(), v);
                    self.run(body, false)?;
                }
            }
            Kind::If(arms) => {
                for (cond, body) in arms {
                    let go = match cond {
                        Some(c) => self.eval(at, c)?.truthy(),
                        None => true,
                    };
                    if go {
                        return self.run(body, false);
                    }
                }
            }
            Kind::Use(name, args) => {
                let defs = self.defs;
                let Some(def) = defs.get(name) else {
                    let mut known: Vec<&String> = defs.keys().collect();
                    known.sort();
                    let hint = if known.is_empty() { String::new() } else { format!(" (templates: {})", known.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", ")) };
                    self.fail(at, format!("unknown template `{name}`{hint}"));
                    return Err(());
                };
                if args.len() != def.params.len() {
                    self.fail(at, format!("`{name}` takes {} argument{} ({}), found {}", def.params.len(), if def.params.len() == 1 { "" } else { "s" }, def.params.join(", "), args.len()));
                    return Err(());
                }
                if self.calls.len() >= MAX_DEPTH {
                    self.fail(at, format!("templates nest more than {MAX_DEPTH} deep"));
                    self.out_of_budget = true;
                    return Err(());
                }
                let mut frame = HashMap::new();
                for (p, a) in def.params.iter().zip(args) {
                    let v = self.eval(at, a)?;
                    frame.insert(p.clone(), v);
                }
                self.frames.push(frame);
                self.calls.push((name.clone(), at));
                let r = self.run(&def.body, false);
                self.calls.pop();
                self.frames.pop();
                r?;
            }
        }
        Ok(())
    }
}

/// at_marker stands for an input line in a note until `expand` knows its file and line.
fn at_marker(line: usize) -> String {
    format!("\u{1}{line}\u{1}")
}

fn cmp(op: &str, o: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::*;
    match op {
        "<" => o == Less,
        "<=" => o != Greater,
        ">" => o == Greater,
        _ => o != Less,
    }
}

/// expand runs the macros of `a`, the include-spliced source. `inc` holds the include
/// diagnostics, whose spans point into `a`.
fn expand(a: Source, inc: Vec<Diag>, budget: u64) -> Loaded {
    let lines: Vec<&str> = a.text.split('\n').collect();
    // The last piece after a final newline is empty, not a line.
    let lines = if a.text.ends_with('\n') { &lines[..lines.len() - 1] } else { &lines[..] };
    if !lines.iter().any(|l| l.contains("${") || is_kw_line(l.trim()).is_some()) {
        let parse = a.text.clone();
        return Loaded { src: a, parse, diags: inc };
    }
    let mut b = Builder { lines, i: 0, defs: HashMap::new(), errs: Vec::new() };
    let (items, _, _) = b.block(true);
    let mut x = Expander {
        lines,
        defs: &b.defs,
        text: String::new(),
        parse: String::new(),
        origin: Vec::new(),
        raw: vec![0; lines.len()],
        frames: vec![HashMap::new()],
        calls: Vec::new(),
        steps: 0,
        budget,
        out_of_budget: false,
        errs: Vec::new(),
    };
    let _ = x.run(&items, true);
    let mut segs: Vec<Seg> = Vec::new();
    for (i, &o) in x.origin.iter().enumerate() {
        let (path, fl) = a.file_line(o + 1);
        let line = i + 1;
        if let Some(s) = segs.last()
            && s.path == path
            && s.file_line + (line - s.line) == fl
        {
            continue;
        }
        segs.push(Seg { line, path: path.to_string(), file_line: fl });
    }
    if segs.is_empty() {
        segs.push(Seg { line: 1, path: a.path.clone(), file_line: 1 });
    }
    let src = Source::spliced(&a.path, &x.text, segs);
    let at_line = |line: usize| {
        let lo = x.raw[line];
        let t = lines[line];
        let lead = t.len() - t.trim_start().len();
        Span::new(lo + lead, lo + t.trim_end().len().max(lead))
    };
    let mut diags = Vec::new();
    let line_starts: Vec<usize> = std::iter::once(0).chain(a.text.match_indices('\n').map(|(k, _)| k + 1)).collect();
    for mut d in inc {
        for l in &mut d.labels {
            let line = line_starts.partition_point(|&s| s <= l.span.lo) - 1;
            if line < lines.len() {
                let base = x.raw[line] as isize - line_starts[line] as isize;
                l.span = Span::new((l.span.lo as isize + base) as usize, (l.span.hi as isize + base) as usize);
            }
        }
        diags.push(d);
    }
    let mut seen = std::collections::HashSet::new();
    for (line, mut d) in b.errs.into_iter().chain(x.errs) {
        // A template line fails the same way at every call; report it once.
        if !seen.insert((line, d.msg.clone())) {
            continue;
        }
        for n in &mut d.notes {
            let parts: Vec<String> = n.split('\u{1}').map(String::from).collect();
            *n = parts.iter().enumerate().map(|(k, p)| match (k % 2, p.parse::<usize>()) {
                (1, Ok(l)) => {
                    let (path, fl) = a.file_line(l + 1);
                    format!("{path}:{fl}")
                }
                _ => p.clone(),
            }).collect();
        }
        let label = match d.code {
            "E0013" => "expansion stopped here",
            "E0002" => "not a valid macro line",
            _ => "while expanding this line",
        };
        diags.push(d.primary(at_line(line), label));
    }
    Loaded { src, parse: x.parse, diags }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> Loaded {
        expand(Source::new("t.fitd", text), Vec::new(), DEFAULT_BUDGET)
    }

    fn visible(l: &Loaded) -> Vec<String> {
        l.parse.lines().map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
    }

    #[test]
    fn plain_files_pass_through() {
        let text = "node end\nend -> a\nnode a \"x^{2}\"\n| cost $5\n";
        let l = run(text);
        assert_eq!(l.parse, text);
        assert!(l.diags.is_empty());
    }

    #[test]
    fn loops_and_interpolation() {
        let l = run("for i in 1..3\n  node s${i} \"Step ${i * 10}\"\n  if i < 3\n    s${i} -> s${i + 1}\n  end\nend\ngroup ${names(\"s\", 1, 3)}\n");
        assert!(l.diags.is_empty(), "{:?}", l.diags.iter().map(|d| &d.msg).collect::<Vec<_>>());
        assert_eq!(visible(&l), ["node s1 \"Step 10\"", "s1 -> s2", "node s2 \"Step 20\"", "s2 -> s3", "node s3 \"Step 30\"", "group s1, s2, s3"]);
        // Generated lines map back to the template line they came from.
        let k = l.src.text.lines().position(|t| t.trim() == "s2 -> s3").unwrap();
        assert_eq!(l.src.file_line(k + 1), ("t.fitd", 4));
    }

    #[test]
    fn comments_are_not_interpolated() {
        let l = run("node a \"$${x} ${1 + 1}\"\n| $${y}\n");
        assert_eq!(visible(&l), ["node a \"${x} 2\"", "| ${y}"]);
        let l = run("for i in 1..1\n  node a${i} \"#${i}\" color=#123  # ${nope}\nend\n");
        assert!(l.diags.is_empty(), "{:?}", l.diags.iter().map(|d| &d.msg).collect::<Vec<_>>());
        assert_eq!(visible(&l), ["node a1 \"#1\" color=#123  # ${nope}"]);
    }

    #[test]
    fn list_loops_and_else() {
        let l = run("for i, s in a, b\n  node ${s}${i}\nend\n");
        assert_eq!(visible(&l), ["node a1", "node b2"]);
        let l = run("for s in auth, front-end, \"Big DB\"\n  if s == \"auth\"\n    node a\n  else if s == \"front-end\"\n    node c\n  else\n    node b\n  end\nend\n");
        assert!(l.diags.is_empty());
        assert_eq!(visible(&l), ["node a", "node c", "node b"]);
    }

    #[test]
    fn recursive_templates() {
        let l = run("use fib(\"f\", 3)\ndef fib(id, n)\n  node ${id} \"${n}\"\n  if n >= 2\n    use fib(id + \"a\", n - 1)\n    use fib(id + \"b\", n - 2)\n    ${id} -> ${id}a, ${id}b\n  end\nend\n");
        assert!(l.diags.is_empty(), "{:?}", l.diags.iter().map(|d| &d.msg).collect::<Vec<_>>());
        let nodes = visible(&l).iter().filter(|s| s.starts_with("node")).count();
        assert_eq!(nodes, 5);
    }

    #[test]
    fn collatz_runs_and_budget_stops_runaway() {
        let collatz = "def c(k)\n  node n${k}\n  if k > 1 and k % 2 == 0\n    n${k} -> n${k / 2}\n    use c(k / 2)\n  else if k > 1\n    n${k} -> n${3 * k + 1}\n    use c(3 * k + 1)\n  end\nend\nuse c(27)\n";
        let l = run(collatz);
        assert!(l.diags.is_empty(), "{:?}", l.diags.iter().map(|d| &d.msg).collect::<Vec<_>>());
        assert_eq!(visible(&l).iter().filter(|s| s.starts_with("node")).count(), 112);
        let l = run("def f(n)\n  use f(n + 1)\nend\nuse f(0)\n");
        assert!(l.diags.iter().any(|d| d.msg.contains("nest more than")), "{:?}", l.diags.iter().map(|d| &d.msg).collect::<Vec<_>>());
        let l = run("for i in 1..1000000000\n  node a${i}\nend\n");
        assert!(l.diags.iter().any(|d| d.code == "E0013"));
    }

    #[test]
    fn errors_point_at_raw_lines() {
        let l = run("node a\nfor i in 1..2\n  node b${j}\nend\nif 1\nnode c\n");
        let msgs: Vec<_> = l.diags.iter().map(|d| (d.code, d.msg.clone(), l.src.file_line(l.src.line_col(d.pos()).0).1)).collect();
        assert!(msgs.iter().any(|(c, m, line)| *c == "E0012" && m.contains("unknown variable `j`") && *line == 3), "{msgs:?}");
        assert!(msgs.iter().any(|(c, m, line)| *c == "E0002" && m.contains("no matching `end`") && *line == 5), "{msgs:?}");
        assert_eq!(l.parse.len(), l.src.text.len());
    }
}
