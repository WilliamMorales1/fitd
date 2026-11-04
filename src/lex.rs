
use crate::diag::{Diag, Span};

#[derive(Clone, PartialEq, Debug)]
pub enum Tok {
    Ident,
    Str(String),
    Num,
    Color,
    Body(String),
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
}

pub fn describe(t: &Tok) -> &'static str {
    match t {
        Tok::Ident => "a name",
        Tok::Str(_) => "a string",
        Tok::Num => "a number",
        Tok::Color => "a color",
        Tok::Body(_) => "a body line",
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

pub fn lex(text: &str) -> (Vec<Token>, Vec<Diag>) {
    let b: Vec<(usize, char)> = text.char_indices().collect();
    let at = |i: usize| b.get(i).map(|&(_, c)| c);
    let off = |i: usize| b.get(i).map(|&(o, _)| o).unwrap_or(text.len());
    let (mut toks, mut errs) = (Vec::new(), Vec::new());
    let mut line_start = true;
    let mut i = 0;
    while let Some(c) = at(i) {
        let start = i;
        let push = |toks: &mut Vec<Token>, tok, end: usize| {
            toks.push(Token {
                tok,
                span: Span::new(off(start), off(end)),
            })
        };
        if c == '\n' {
            push(&mut toks, Tok::Newline, i + 1);
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
        } else if c == '#' {
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
            push(
                &mut toks,
                Tok::Body(text[off(lo)..off(i)].trim_end().to_string()),
                i,
            );
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
                || (at(i) == Some('.') && at(i + 1).is_some_and(|c| c.is_ascii_digit()))
            {
                i += 1;
            }
            push(&mut toks, Tok::Num, i);
        } else if ident_char(c) {
            while at(i).is_some_and(ident_char)
                || (at(i) == Some('-') && at(i + 1).is_some_and(ident_char))
            {
                i += 1;
            }
            push(&mut toks, Tok::Ident, i);
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
                ('=', _) => (Some(Tok::Eq), 1),
                ('.', _) => (Some(Tok::Dot), 1),
                _ => (None, 1),
            };
            i += len;
            match tok {
                Some(t) => push(&mut toks, t, i),
                None => errs.push(
                    Diag::error("E0002", format!("unexpected character `{c}`"))
                        .primary(Span::new(off(start), off(i)), "not valid here"),
                ),
            }
        }
    }
    toks.push(Token {
        tok: Tok::Newline,
        span: Span::new(text.len(), text.len()),
    });
    toks.push(Token {
        tok: Tok::Eof,
        span: Span::new(text.len(), text.len()),
    });
    (toks, errs)
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
            kinds("font 7pt..48pt # c"),
            vec![Ident, Num, DotDot, Num, Newline, Eof]
        );
        assert_eq!(
            kinds("x color=#1565C0"),
            vec![Ident, Ident, Eq, Color, Newline, Eof]
        );
        assert_eq!(
            kinds("| a = b # not a comment"),
            vec![Body("a = b # not a comment".into()), Newline, Eof]
        );
        assert_eq!(kinds("page 2296x1524pt"), vec![Ident, Num, Newline, Eof]);
    }
}
