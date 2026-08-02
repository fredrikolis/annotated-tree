// Concern: splits a Bash command into words and operators, resolving quoting and recording each token's byte span | Non-concern: what any token means | IO: (command) -> tokens + unmodellable constructs

/// A token's role. Words carry their text with quoting resolved; operators carry it verbatim.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Word(String),
    Op(String),
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: Kind,
    /// Byte range in the ORIGINAL command. A rewrite splices over this, so quoting, globs and
    /// spacing survive untouched.
    pub start: usize,
    pub end: usize,
}

/// The tokens, plus the three constructs that make a command unmodellable.
///
/// Lexing rather than pattern-matching is not fastidiousness. String tests on shell syntax get
/// three things wrong, each silently skipping commands that were perfectly eligible:
/// `2>/dev/null` is a *stderr* redirect, a `|` inside `grep "a\|b"` is part of a quoted token, and
/// `||` is "or else" rather than a pipe.
pub struct Lexed {
    pub tokens: Vec<Token>,
    /// `$( )` or a backtick: a second command this does not model.
    pub substitution: bool,
    /// `( … )`: legal, but its stdout can be redirected as a unit.
    pub subshell: bool,
    /// Quoting never closed.
    pub unbalanced: bool,
    /// A `<<` heredoc. Its body lines are DATA, not commands — rewriting one writes the rewrite
    /// into whatever file the heredoc feeds.
    pub heredoc: bool,
}

impl Lexed {
    /// Whether anything in the command puts it beyond what this can reason about.
    pub fn unmodellable(&self) -> bool {
        self.substitution || self.subshell || self.unbalanced || self.heredoc
    }
}

pub fn lex(src: &str) -> Lexed {
    let b = src.as_bytes();
    let mut tokens = Vec::new();
    let mut substitution = false;
    let mut subshell = false;
    let mut unbalanced = false;
    let mut heredoc = false;
    let mut i = 0;

    while i < b.len() {
        // A newline separates commands, so it must be classified before whitespace eats it.
        if b[i].is_ascii_whitespace() && b[i] != b'\n' {
            i += 1;
            continue;
        }
        let start = i;

        // `#` opens a comment only at a token boundary; inside a word it is ordinary (`file#1`). Emitting no token splices the annotator BEFORE it, since appending after a `#` comments out the closing `)`.
        if b[i] == b'#' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // Redirection, with an optional leading file descriptor: `>`, `>>`, `2>`, `1>>`, `2>&1`. `<<` and `<<-` open a heredoc; everything after is data this cannot model.
        if src.get(i..i + 2) == Some("<<") {
            heredoc = true;
            tokens.push(op(src, i, i + 2));
            i += 2;
            continue;
        }
        let digits = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if i + digits < b.len() && matches!(b[i + digits], b'>' | b'<') {
            let mut end = i + digits + 1;
            if end < b.len() && matches!(b[end], b'>' | b'&') {
                end += 1;
            }
            tokens.push(op(src, start, end));
            i = end;
            continue;
        }
        if b[i] == b'&' && i + 1 < b.len() && b[i + 1] == b'>' {
            tokens.push(op(src, start, i + 2));
            i += 2;
            continue;
        }

        // Two-character operators first, so `||` never lexes as two `|`. `get`, not `&src[i..i + 2]`: a byte index inside a multi-byte character panics.
        if matches!(
            src.get(i..i + 2),
            Some("||") | Some("&&") | Some(";;") | Some("|&")
        ) {
            tokens.push(op(src, start, i + 2));
            i += 2;
            continue;
        }
        if matches!(b[i], b'|' | b';' | b'&' | b'(' | b')' | b'\n') {
            subshell |= b[i] == b'(' || b[i] == b')';
            tokens.push(op(src, start, i + 1));
            i += 1;
            continue;
        }

        // Accumulated as BYTES: `b[i] as char` decodes each byte as its own Latin-1 character, mangling every multi-byte one — harmless while the text is only compared, corrupting once it is forwarded.
        let mut text: Vec<u8> = Vec::new();
        while i < b.len() {
            let c = b[i];
            if c.is_ascii_whitespace()
                || matches!(c, b'|' | b';' | b'&' | b'(' | b')' | b'<' | b'>')
            {
                break;
            }
            match c {
                b'\\' => {
                    i += 1;
                    match b.get(i) {
                        Some(&next) => {
                            text.push(next);
                            i += 1;
                        }
                        None => unbalanced = true,
                    }
                }
                b'\'' => {
                    i += 1;
                    let from = i;
                    while i < b.len() && b[i] != b'\'' {
                        i += 1;
                    }
                    text.extend_from_slice(&b[from..i.min(b.len())]);
                    if i >= b.len() {
                        unbalanced = true;
                    } else {
                        i += 1;
                    }
                }
                b'"' => {
                    i += 1;
                    while i < b.len() && b[i] != b'"' {
                        if b[i] == b'\\' && i + 1 < b.len() {
                            text.push(b[i + 1]);
                            i += 2;
                            continue;
                        }
                        substitution |=
                            b[i] == b'`' || (b[i] == b'$' && b.get(i + 1) == Some(&b'('));
                        text.push(b[i]);
                        i += 1;
                    }
                    if i >= b.len() {
                        unbalanced = true;
                    } else {
                        i += 1;
                    }
                }
                b'`' => {
                    substitution = true;
                    text.push(b'`');
                    i += 1;
                }
                b'$' if b.get(i + 1) == Some(&b'(') => {
                    substitution = true;
                    text.extend_from_slice(b"$(");
                    i += 2;
                }
                _ => {
                    text.push(c);
                    i += 1;
                }
            }
        }
        if i == start {
            i += 1; // never stall on a byte no branch consumed
        }
        tokens.push(Token {
            kind: Kind::Word(String::from_utf8_lossy(&text).into_owned()),
            start,
            end: i,
        });
    }

    Lexed {
        tokens,
        substitution,
        subshell,
        unbalanced,
        heredoc,
    }
}

fn op(src: &str, start: usize, end: usize) -> Token {
    Token {
        kind: Kind::Op(src[start..end].to_string()),
        start,
        end,
    }
}
