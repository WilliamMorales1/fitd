use crate::diag::{Diag, Span};

#[derive(Clone, PartialEq, Debug)]
pub enum Tok {
    Ident,
    Str(String),
    Num,
    Color,
    Body(String),
    /// Expr is the raw condition of `if` or the `NAMES in ITEMS` of `for`, up to its `{`.
    Expr(String),
    /// Tilde starts a figure row; a `[` right after it switches the lexer to cells.
    Tilde,
    /// Cell is one raw comma-separated item of a `~ [...]` row, such as `%100"d 64"*4`.
    Cell(String),
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Arrow,
    DashDash,
    DotArrow,
    DotDot,
    Comma,
    LParen,
    RParen,
    Eq,
    Dot,
    Newline,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
    /// text replaces the source slice for tokens that macro expansion produced.
    pub text: Option<String>,
}

impl Token {
    pub fn new(tok: Tok, span: Span) -> Token {
        Token {
            tok,
            span,
            text: None,
        }
    }

    /// src gives the token's text: its override, or else its slice of `text`.
    pub fn src<'a>(&'a self, text: &'a str) -> &'a str {
        match &self.text {
            Some(t) => t,
            None => &text[self.span.lo..self.span.hi],
        }
    }
}

pub fn describe(t: &Tok) -> &'static str {
    match t {
        Tok::Ident => "a name",
        Tok::Str(_) => "a string",
        Tok::Num => "a number",
        Tok::Color => "a color",
        Tok::Body(_) => "a body line",
        Tok::Expr(_) => "an expression",
        Tok::Tilde => "`~`",
        Tok::Cell(_) => "a cell",
        Tok::LBrace => "`{`",
        Tok::RBrace => "`}`",
        Tok::LBracket => "`[`",
        Tok::RBracket => "`]`",
        Tok::Arrow => "`->`",
        Tok::DashDash => "`--`",
        Tok::DotArrow => "`..>`",
        Tok::DotDot => "`..`",
        Tok::Comma => "`,`",
        Tok::LParen => "`(`",
        Tok::RParen => "`)`",
        Tok::Eq => "`=`",
        Tok::Dot => "`.`",
        Tok::Newline => "end of line",
        Tok::Eof => "end of file",
    }
}

fn ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// interp_end gives the char index after the `}` closing a `${` that starts at `i`, skipping quoted text and nested braces.
pub fn interp_end(cs: &[char], i: usize) -> Option<usize> {
    if cs.get(i) != Some(&'$') || cs.get(i + 1) != Some(&'{') {
        return None;
    }
    let (mut depth, mut quoted, mut j) = (1, false, i + 2);
    while let Some(&c) = cs.get(j) {
        match c {
            '\\' if quoted => j += 1,
            '"' => quoted = !quoted,
            '\n' => return None,
            '{' if !quoted => depth += 1,
            '}' if !quoted => {
                depth -= 1;
                if depth == 0 {
                    return Some(j + 1);
                }
            }
            _ => {}
        }
        j += 1;
    }
    None
}

pub fn lex(text: &str) -> (Vec<Token>, Vec<Diag>) {
    let b: Vec<(usize, char)> = text.char_indices().collect();
    let cs: Vec<char> = b.iter().map(|&(_, c)| c).collect();
    let at = |i: usize| b.get(i).map(|&(_, c)| c);
    let off = |i: usize| b.get(i).map(|&(o, _)| o).unwrap_or(text.len());
    let (mut toks, mut errs): (Vec<Token>, Vec<Diag>) = (Vec::new(), Vec::new());
    let mut line_start = true;
    // depth counts open `(` and `[`, inside which newlines do not end a statement.
    let mut depth = 0usize;
    let mut i = 0;
    while let Some(c) = at(i) {
        let start = i;
        let push = |toks: &mut Vec<Token>, tok, end: usize| {
            toks.push(Token::new(tok, Span::new(off(start), off(end))))
        };
        if c == '\n' {
            if depth == 0 {
                push(&mut toks, Tok::Newline, i + 1);
            }
            line_start = true;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let first = std::mem::replace(&mut line_start, false);
        if c == '#' && i > 0 && at(i - 1) == Some('=') {
            i += 1;
            while at(i).is_some_and(|c| c.is_ascii_hexdigit()) {
                i += 1;
            }
            push(&mut toks, Tok::Color, i);
        } else if c == '/' && at(i + 1) == Some('/') {
            while at(i).is_some_and(|c| c != '\n') {
                i += 1;
            }
        } else if c == '|' && first {
            i += 1;
            if at(i) == Some(' ') {
                i += 1;
            }
            let lo = i;
            while at(i).is_some_and(|c| c != '\n') {
                i += 1;
            }
            let line = text[off(lo)..off(i)].trim_end().to_string();
            push(&mut toks, Tok::Body(line), i);
        } else if c == '~' && first {
            i += 1;
            push(&mut toks, Tok::Tilde, i);
            while at(i).is_some_and(|c| c == ' ' || c == '\t') {
                i += 1;
            }
            if at(i) == Some('[') {
                i = cells(text, &b, i, &mut toks, &mut errs);
            }
        } else if c == '"' {
            i += 1;
            let mut s = String::new();
            loop {
                match at(i) {
                    None | Some('\n') => {
                        errs.push(
                            Diag::error("E0002", "unterminated string")
                                .primary(Span::new(off(start), off(i)), "string starts here"),
                        );
                        break;
                    }
                    Some('"') => {
                        i += 1;
                        break;
                    }
                    Some('$') if interp_end(&cs, i).is_some() => {
                        let end = interp_end(&cs, i).unwrap();
                        s.extend(&cs[i..end]);
                        i = end;
                    }
                    Some('\\') => {
                        match at(i + 1) {
                            Some('n') => s.push('\n'),
                            Some(o) if o != '\n' => s.push(o),
                            _ => {}
                        }
                        i += 2;
                    }
                    Some(o) => {
                        s.push(o);
                        i += 1;
                    }
                }
            }
            push(&mut toks, Tok::Str(s), i);
        } else if c.is_ascii_digit() {
            while at(i).is_some_and(|c| c.is_alphanumeric())
                || (matches!(at(i), Some('.' | ':'))
                    && at(i + 1).is_some_and(|c| c.is_ascii_digit()))
            {
                i += 1;
            }
            push(&mut toks, Tok::Num, i);
        } else if ident_char(c) || interp_end(&cs, i).is_some() {
            loop {
                if let Some(end) = interp_end(&cs, i) {
                    i = end;
                } else if at(i).is_some_and(ident_char)
                    || (at(i) == Some('-')
                        && (at(i + 1).is_some_and(ident_char) || interp_end(&cs, i + 1).is_some()))
                {
                    i += 1;
                } else {
                    break;
                }
            }
            let word = &text[off(start)..off(i)];
            let lead = toks
                .last()
                .is_none_or(|t| matches!(t.tok, Tok::Newline | Tok::LBrace | Tok::RBrace));
            let after_else = toks
                .last()
                .is_some_and(|t| t.tok == Tok::Ident && &text[t.span.lo..t.span.hi] == "else");
            push(&mut toks, Tok::Ident, i);
            if (lead && (word == "if" || word == "for")) || (after_else && word == "if") {
                i = expr(text, &b, i, &mut toks);
            }
        } else {
            let two = (c, at(i + 1).unwrap_or(' '));
            let (tok, len) = match two {
                ('-', '>') => (Some(Tok::Arrow), 2),
                ('-', '-') => (Some(Tok::DashDash), 2),
                ('.', '.') if at(i + 2) == Some('>') => (Some(Tok::DotArrow), 3),
                ('.', '.') => (Some(Tok::DotDot), 2),
                (',', _) => (Some(Tok::Comma), 1),
                ('(', _) => (Some(Tok::LParen), 1),
                (')', _) => (Some(Tok::RParen), 1),
                ('[', _) => (Some(Tok::LBracket), 1),
                (']', _) => (Some(Tok::RBracket), 1),
                ('{', _) => (Some(Tok::LBrace), 1),
                ('}', _) => (Some(Tok::RBrace), 1),
                ('=', _) => (Some(Tok::Eq), 1),
                ('.', _) => (Some(Tok::Dot), 1),
                _ => (None, 1),
            };
            i += len;
            match tok {
                Some(Tok::LParen | Tok::LBracket) => depth += 1,
                Some(Tok::RParen | Tok::RBracket) => depth = depth.saturating_sub(1),
                _ => {}
            }
            match tok {
                Some(t) => push(&mut toks, t, i),
                None => errs.push(
                    Diag::error("E0002", format!("unexpected character `{c}`"))
                        .primary(Span::new(off(start), off(i)), "not valid here"),
                ),
            }
        }
    }
    let end = Span::new(text.len(), text.len());
    toks.push(Token::new(Tok::Newline, end));
    toks.push(Token::new(Tok::Eof, end));
    (toks, errs)
}

/// expr lexes the raw text after `if` or `for` up to the `{` that opens its block, and gives the index of that `{`.
fn expr(text: &str, b: &[(usize, char)], mut i: usize, toks: &mut Vec<Token>) -> usize {
    let cs: Vec<char> = b.iter().map(|&(_, c)| c).collect();
    let off = |i: usize| b.get(i).map(|&(o, _)| o).unwrap_or(text.len());
    while cs.get(i).is_some_and(|&c| c == ' ' || c == '\t') {
        i += 1;
    }
    let lo = i;
    let (mut depth, mut quoted) = (0usize, false);
    while let Some(&c) = cs.get(i) {
        match c {
            '\n' => break,
            '\\' if quoted => i += 1,
            '"' => quoted = !quoted,
            '/' if !quoted && cs.get(i + 1) == Some(&'/') => break,
            '(' | '[' if !quoted => depth += 1,
            ')' | ']' if !quoted => depth = depth.saturating_sub(1),
            '{' if !quoted && depth == 0 => break,
            _ => {}
        }
        i += 1;
    }
    let raw = text[off(lo)..off(i)].trim_end();
    if !raw.is_empty() {
        toks.push(Token::new(
            Tok::Expr(raw.to_string()),
            Span::new(off(lo), off(lo) + raw.len()),
        ));
    }
    i
}

/// cells lexes a `[...]` figure row starting at char `i`, one `Cell` per comma-separated item, and gives the index after `]`.
fn cells(
    text: &str,
    b: &[(usize, char)],
    mut i: usize,
    toks: &mut Vec<Token>,
    errs: &mut Vec<Diag>,
) -> usize {
    let at = |i: usize| b.get(i).map(|&(_, c)| c);
    let off = |i: usize| b.get(i).map(|&(o, _)| o).unwrap_or(text.len());
    let open = i;
    let cs: Vec<char> = b.iter().map(|&(_, c)| c).collect();
    let tok = |toks: &mut Vec<Token>, tok, lo: usize, hi: usize| {
        toks.push(Token::new(tok, Span::new(off(lo), off(hi))))
    };
    tok(toks, Tok::LBracket, i, i + 1);
    i += 1;
    loop {
        match at(i) {
            None => {
                errs.push(
                    Diag::error("E0002", "figure row has no closing `]`")
                        .primary(Span::new(off(open), off(open + 1)), "row starts here"),
                );
                return i;
            }
            Some(c) if c.is_whitespace() => i += 1,
            Some('/') if at(i + 1) == Some('/') => {
                while at(i).is_some_and(|c| c != '\n') {
                    i += 1;
                }
            }
            Some(']') => {
                tok(toks, Tok::RBracket, i, i + 1);
                return i + 1;
            }
            Some(',') => {
                tok(toks, Tok::Comma, i, i + 1);
                i += 1;
            }
            Some(_) => {
                let lo = i;
                // Parens count too, so a `for x in split(s, ",") { .. }` cell stays whole.
                let (mut quoted, mut depth, mut parens) = (false, 0usize, 0usize);
                while let Some(c) = at(i) {
                    match c {
                        '$' if !quoted && interp_end(&cs, i).is_some() => {
                            i = interp_end(&cs, i).unwrap();
                            continue;
                        }
                        '"' => quoted = !quoted,
                        '\\' if quoted => i += 1,
                        '{' if !quoted => depth += 1,
                        '}' if !quoted && depth > 0 => depth -= 1,
                        '(' if !quoted => parens += 1,
                        ')' if !quoted && parens > 0 => parens -= 1,
                        ',' | ']' if !quoted && depth == 0 && parens == 0 => break,
                        '\n' if depth == 0 => break,
                        _ => {}
                    }
                    i += 1;
                }
                let raw = text[off(lo)..off(i)].trim_end().to_string();
                let hi = lo + raw.chars().count();
                tok(toks, Tok::Cell(raw), lo, hi);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(s: &str) -> Vec<Tok> {
        lex(s).0.into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn hyphenated_names_and_arrows() {
        use Tok::*;
        assert_eq!(
            kinds("a left-of b\n"),
            vec![Ident, Ident, Ident, Newline, Newline, Eof]
        );
        assert_eq!(
            kinds("a->b--c..>d..e"),
            vec![
                Ident, Arrow, Ident, DashDash, Ident, DotArrow, Ident, DotDot, Ident, Newline, Eof
            ]
        );
    }

    #[test]
    fn numbers_colors_comments_bodies() {
        use Tok::*;
        assert_eq!(
            kinds("font 7pt..48pt // c"),
            vec![Ident, Num, DotDot, Num, Newline, Eof]
        );
        assert_eq!(
            kinds("x color=#1565C0"),
            vec![Ident, Ident, Eq, Color, Newline, Eof]
        );
        assert_eq!(
            kinds("~ [%100, \"d, 64\"*2,\n .] // c"),
            vec![
                Tilde,
                LBracket,
                Cell("%100".into()),
                Comma,
                Cell("\"d, 64\"*2".into()),
                Comma,
                Cell(".".into()),
                RBracket,
                Newline,
                Eof
            ]
        );
        assert_eq!(
            kinds("a [k=1,\n j=2] {\n}"),
            vec![
                Ident, LBracket, Ident, Eq, Num, Comma, Ident, Eq, Num, RBracket, LBrace, Newline,
                RBrace, Newline, Eof
            ]
        );
        assert_eq!(
            kinds("| a = b // not a comment"),
            vec![Body("a = b // not a comment".into()), Newline, Eof]
        );
        assert_eq!(kinds("page 2296x1524pt"), vec![Ident, Num, Newline, Eof]);
    }
}
