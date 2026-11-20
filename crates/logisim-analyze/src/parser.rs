//! Reads expressions typed by people, in any of the supported notations at once.
//!
//! Operators: `~ ! - ' ¬` (NOT, `'` after the operand), `* · ⋅ ∧ & &&` (AND, or just writing
//! operands next to each other), `+ ∨ | ||` (OR), `^ ⊕ ⊻ ≠ != ≢` (XOR), `= == ≡ ⇔ ⊙` (XNOR),
//! and the words NOT, AND, OR, XOR, EQUALS. `0` and `1` are constants.
//!
//! Binding strength is the same one the printer uses, so printed text reads back unchanged.
//! With the ASCII symbols that means `*` binds tighter than `^`, which binds tighter than `+`
//! (the original treats `*` and `+` as equals and reads left to right; this port follows the
//! usual school-book convention instead because it is what people expect to type).

use crate::expr::{prec, Expr};
use std::fmt;

/// A syntax error, with where it happened (character offsets into the typed text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub offset: usize,
    pub length: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at character {})", self.message, self.offset + 1)
    }
}

impl std::error::Error for ParseError {}

/// The result of reading one line: the expression, and the output it was assigned to, if the
/// line was written `Out = expression`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub target: Option<String>,
    pub expr: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    And,
    Or,
    Xor,
    Xnor,
    Not,
    NotPostfix,
    LParen,
    RParen,
    Ident,
    Const,
    Eq,
    ErrBadChar,
    ErrBrace,
    ErrSubscript,
    ErrIdent,
}

#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    offset: usize,
    length: usize,
    prec: i32,
    text: String,
}

impl Token {
    fn error(&self, message: String) -> ParseError {
        ParseError {
            message,
            offset: self.offset,
            length: self.length,
        }
    }
}

struct Level {
    level: i32,
    current: Option<Expr>,
    cause: Token,
}

fn ok_char(c: char) -> bool {
    c.is_whitespace()
        || c.is_alphabetic()
        || c == '_'
        || c == '$'
        || "()01~-^+*!&|=\\':[]".contains(c)
        || "≠≢⋀⋁∧∨⊕⋅¬∙⇔≡↔˜·∥⊻⊤⊥⊙".contains(c)
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}
fn is_ident_part(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn tok(kind: Kind, start: usize, text: &str, prec: i32) -> Token {
    Token {
        kind,
        offset: start,
        length: text.chars().count(),
        prec,
        text: text.to_string(),
    }
}

fn tokenize(input: &str) -> Vec<Token> {
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let peek = |i: usize| chars.get(i).copied().unwrap_or(' ');
    let slice = |a: usize, b: usize| chars[a..b.min(len)].iter().collect::<String>();
    let mut out = vec![];
    let mut pos = 0;
    while pos < len {
        let c = chars[pos];
        if c.is_whitespace() {
            pos += 1;
            continue;
        }
        let start = pos;
        pos += 1;
        if is_ident_start(c) {
            while pos < len && is_ident_part(chars[pos]) {
                pos += 1;
            }
            let name = slice(start, pos);
            let mut subscript: Option<String> = None;
            if peek(pos) == ':' && peek(pos + 1).is_ascii_digit() {
                pos += 1;
                let s = pos;
                while peek(pos).is_ascii_digit() {
                    pos += 1;
                }
                subscript = Some(slice(s, pos));
            } else if peek(pos) == '[' {
                let brace = pos;
                pos += 1;
                while pos < len && chars[pos].is_whitespace() {
                    pos += 1;
                }
                let s = pos;
                while peek(pos).is_ascii_digit() && pos < len {
                    pos += 1;
                }
                let digits = slice(s, pos);
                while pos < len && chars[pos].is_whitespace() {
                    pos += 1;
                }
                if pos >= len || chars[pos] != ']' {
                    let rest = slice(brace, len);
                    let mut t = tok(Kind::ErrBrace, start, &rest, 0);
                    t.length = len - start;
                    out.push(t);
                    pos = len;
                    continue;
                }
                pos += 1;
                subscript = Some(digits);
            }
            match subscript {
                Some(s) if s.is_empty() => {
                    let text = slice(start, pos);
                    out.push(tok(Kind::ErrSubscript, start, &text, 0));
                }
                Some(s) => {
                    let n: u64 = s.parse().unwrap_or(u64::MAX);
                    let full = format!("{name}[{n}]");
                    let mut t = tok(Kind::Ident, start, &full, i32::MAX);
                    t.length = pos - start;
                    out.push(t);
                }
                None => out.push(tok(Kind::Ident, start, &name, i32::MAX)),
            }
            continue;
        }
        let t = match c {
            '(' => tok(Kind::LParen, start, "(", i32::MAX),
            ')' => tok(Kind::RParen, start, ")", i32::MAX),
            '1' | '⊤' => tok(Kind::Const, start, "1", i32::MAX),
            '0' | '⊥' => tok(Kind::Const, start, "0", i32::MAX),
            '~' | '-' | '¬' | '˜' => tok(Kind::Not, start, "~", prec::NOT),
            '!' => {
                if peek(pos) == '=' {
                    pos += 1;
                    let text = slice(start, pos);
                    tok(Kind::Xor, start, &text, prec::LOGIC)
                } else {
                    tok(Kind::Not, start, "~", prec::NOT)
                }
            }
            '\'' => tok(Kind::NotPostfix, start, "'", prec::NOT),
            '^' | '⊕' => tok(Kind::Xor, start, "^", prec::OPLUS),
            '⊻' | '≢' | '≠' => tok(Kind::Xor, start, "^", prec::LOGIC),
            '+' => tok(Kind::Or, start, "+", prec::PLUS),
            '⋁' | '∨' => tok(Kind::Or, start, "+", prec::LOGIC),
            '∥' => tok(Kind::Or, start, "+", prec::OR),
            '*' => tok(Kind::And, start, "*", prec::TIMES),
            '⋀' | '∧' => tok(Kind::And, start, "*", prec::LOGIC),
            '⋅' | '∙' | '·' => tok(Kind::And, start, "*", prec::TIMES),
            '⊙' => tok(Kind::Xnor, start, "^", prec::OTIMES),
            '⇔' | '≡' | '↔' => tok(Kind::Xnor, start, "=", prec::LOGIC),
            '&' => {
                if peek(pos) == '&' {
                    pos += 1;
                    tok(Kind::And, start, "&&", prec::AND)
                } else {
                    tok(Kind::And, start, "&", prec::BITAND)
                }
            }
            '|' => {
                if peek(pos) == '|' {
                    pos += 1;
                    tok(Kind::Or, start, "||", prec::OR)
                } else {
                    tok(Kind::Or, start, "|", prec::BITOR)
                }
            }
            '=' => {
                if peek(pos) == '=' {
                    pos += 1;
                }
                let text = slice(start, pos);
                tok(Kind::Xnor, start, &text, prec::LOGIC)
            }
            ':' => {
                if peek(pos) == '=' {
                    pos += 1;
                }
                let text = slice(start, pos);
                tok(Kind::Eq, start, &text, prec::EQ)
            }
            '[' | ']' => tok(Kind::ErrIdent, start, &c.to_string(), 0),
            _ => {
                while pos < len && !ok_char(chars[pos]) {
                    pos += 1;
                }
                let text = slice(start, pos);
                tok(Kind::ErrBadChar, start, &text, 0)
            }
        };
        out.push(t);
    }
    out
}

fn peek_level(stack: &[Level]) -> i32 {
    stack.last().map_or(-3, |c| c.level)
}

fn pop_to(
    stack: &mut Vec<Level>,
    level: i32,
    mut current: Option<Expr>,
) -> Result<Option<Expr>, ParseError> {
    while peek_level(stack) >= level && !stack.is_empty() {
        let top = stack.pop().unwrap();
        let Some(cur) = current else {
            return Err(top
                .cause
                .error(format!("'{}' needs something on its right", top.cause.text)));
        };
        current = Some(match top.cause.kind {
            Kind::And => combine(top.current, cur, Expr::and),
            Kind::Or => combine(top.current, cur, Expr::or),
            Kind::Xor => combine(top.current, cur, Expr::xor),
            Kind::Xnor => combine(top.current, cur, Expr::xnor),
            Kind::Not => Expr::not(cur),
            _ => cur,
        });
    }
    Ok(current)
}

fn combine(left: Option<Expr>, right: Expr, f: fn(Expr, Expr) -> Expr) -> Expr {
    match left {
        Some(l) => f(l, right),
        None => right,
    }
}

fn parse_tokens(tokens: &[Token]) -> Result<Expr, ParseError> {
    let mut stack: Vec<Level> = vec![];
    let mut current: Option<Expr> = None;
    let implicit_and = |at: &Token| Token {
        kind: Kind::And,
        offset: at.offset,
        length: 0,
        prec: prec::IMPLICIT_AND,
        text: "implicit AND".to_string(),
    };
    let mut i = 0;
    while i < tokens.len() {
        let t = &tokens[i];
        match t.kind {
            Kind::Ident | Kind::Const => {
                let mut here = if t.kind == Kind::Ident {
                    Expr::var(&t.text)
                } else {
                    Expr::Const(t.text == "1")
                };
                while i + 1 < tokens.len() && tokens[i + 1].kind == Kind::NotPostfix {
                    here = Expr::not(here);
                    i += 1;
                }
                while peek_level(&stack) == prec::NOT && !stack.is_empty() {
                    here = Expr::not(here);
                    stack.pop();
                }
                current = Some(combine(current.take(), here, Expr::and));
                if peek_level(&stack) == prec::IMPLICIT_AND && !stack.is_empty() {
                    let top = stack.pop().unwrap();
                    current = Some(combine(top.current, current.take().unwrap(), Expr::and));
                }
            }
            Kind::Not => {
                if current.is_some() {
                    stack.push(Level {
                        level: prec::IMPLICIT_AND,
                        current: current.take(),
                        cause: implicit_and(t),
                    });
                }
                stack.push(Level {
                    level: prec::NOT,
                    current: None,
                    cause: t.clone(),
                });
                current = None;
            }
            Kind::NotPostfix => {
                return Err(t.error("a ' (NOT) must come right after something to negate".into()))
            }
            Kind::LParen => {
                if current.is_some() {
                    stack.push(Level {
                        level: prec::IMPLICIT_AND,
                        current: current.take(),
                        cause: implicit_and(t),
                    });
                }
                stack.push(Level {
                    level: -2,
                    current: None,
                    cause: t.clone(),
                });
                current = None;
            }
            Kind::RParen => {
                current = pop_to(&mut stack, -1, current.take())?;
                if stack.is_empty() {
                    return Err(t.error("this ) has no matching (".into()));
                }
                stack.pop();
                let mut c = current.take();
                if c.is_none() {
                    return Err(t.error("there is nothing between ( and )".into()));
                }
                while i + 1 < tokens.len() && tokens[i + 1].kind == Kind::NotPostfix {
                    c = c.map(Expr::not);
                    i += 1;
                }
                current = pop_to(&mut stack, prec::IMPLICIT_AND, c)?;
            }
            Kind::And | Kind::Or | Kind::Xor | Kind::Xnor | Kind::Eq => {
                let Some(cur) = current.take() else {
                    return Err(t.error(format!("'{}' needs something on its left", t.text)));
                };
                let left = pop_to(&mut stack, t.prec, Some(cur))?;
                stack.push(Level {
                    level: t.prec,
                    current: left,
                    cause: t.clone(),
                });
            }
            Kind::ErrBadChar | Kind::ErrBrace | Kind::ErrSubscript | Kind::ErrIdent => {
                unreachable!("error tokens are reported before parsing")
            }
        }
        i += 1;
    }
    current = pop_to(&mut stack, -1, current)?;
    if let Some(top) = stack.pop() {
        return Err(top.cause.error("this ( is never closed".into()));
    }
    current.ok_or_else(|| ParseError {
        message: "the expression is empty".into(),
        offset: 0,
        length: 0,
    })
}

fn check_tokens(
    tokens: &mut [Token],
    inputs: &[String],
    outputs: &[String],
    allow_assignment: bool,
) -> Result<Option<String>, ParseError> {
    let mut target = None;
    for i in 0..tokens.len() {
        let t = tokens[i].clone();
        match t.kind {
            Kind::ErrBadChar => {
                return Err(t.error(format!("'{}' is not allowed in an expression", t.text)))
            }
            Kind::ErrBrace => {
                return Err(t.error(format!("missing ] to close the bit number in '{}'", t.text)))
            }
            Kind::ErrSubscript => {
                return Err(t.error(format!("'{}' needs a bit number, as in A[0]", t.text)))
            }
            Kind::ErrIdent => {
                return Err(t.error(format!("'{}' only belongs after a variable name", t.text)))
            }
            Kind::Eq if i != 1 || !allow_assignment => {
                return Err(t.error(format!(
                    "'{}' can only be used once, after the output name",
                    t.text
                )))
            }
            Kind::Ident => {
                if inputs.contains(&t.text) {
                    continue;
                }
                let (kind, p) = match t.text.to_uppercase().as_str() {
                    "NOT" => (Kind::Not, prec::NOT),
                    "AND" => (Kind::And, prec::PYTHON_AND),
                    "XOR" => (Kind::Xor, prec::PYTHON_XOR),
                    "OR" => (Kind::Or, prec::PYTHON_OR),
                    "EQUALS" => (Kind::Xnor, prec::LOGIC),
                    _ => {
                        if i == 0
                            && allow_assignment
                            && outputs.contains(&t.text)
                            && tokens.len() >= 2
                            && matches!(tokens[1].kind, Kind::Xnor | Kind::Eq)
                        {
                            tokens[1].kind = Kind::Eq;
                            tokens[1].prec = prec::EQ;
                            target = Some(t.text.clone());
                            continue;
                        }
                        let hint = if outputs.contains(&t.text) {
                            ": outputs can only be used on the left of ="
                        } else {
                            ""
                        };
                        return Err(t.error(format!("'{}' is not an input variable{hint}", t.text)));
                    }
                };
                tokens[i].kind = kind;
                tokens[i].prec = p;
            }
            _ => {}
        }
    }
    Ok(target)
}

/// Read an expression that may only use the given input variables. Empty text gives `None`.
pub fn parse(text: &str, inputs: &[String]) -> Result<Option<Expr>, ParseError> {
    Ok(parse_line(text, inputs, &[], false)?.map(|p| p.expr))
}

/// Like [`parse`], but also accepts `Out = expression` (or `Out := expression`) for the given
/// output names.
pub fn parse_assignment(
    text: &str,
    inputs: &[String],
    outputs: &[String],
) -> Result<Option<Parsed>, ParseError> {
    parse_line(text, inputs, outputs, true)
}

fn parse_line(
    text: &str,
    inputs: &[String],
    outputs: &[String],
    allow_assignment: bool,
) -> Result<Option<Parsed>, ParseError> {
    let mut tokens = tokenize(text);
    if tokens.is_empty() {
        return Ok(None);
    }
    let target = check_tokens(&mut tokens, inputs, outputs, allow_assignment)?;
    if target.is_some() {
        let eq = tokens[1].clone();
        let rest = &tokens[2..];
        if rest.is_empty() {
            return Err(eq.error(format!("'{}' needs something on its right", eq.text)));
        }
        let expr = parse_tokens(rest)?;
        return Ok(Some(Parsed { target, expr }));
    }
    Ok(Some(Parsed {
        target: None,
        expr: parse_tokens(&tokens)?,
    }))
}

/// Rename a variable in expression text, leaving everything else (spacing, symbols) alone.
pub fn replace_variable(text: &str, old: &str, new: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if is_ident_start(chars[i]) {
            let s = i;
            while i < chars.len() && is_ident_part(chars[i]) {
                i += 1;
            }
            let mut end = i;
            // keep a bit subscript with its name: A[3] or A:3
            if i < chars.len() && chars[i] == '[' {
                if let Some(close) = chars[i..].iter().position(|c| *c == ']') {
                    end = i + close + 1;
                }
            } else if i + 1 < chars.len() && chars[i] == ':' && chars[i + 1].is_ascii_digit() {
                end = i + 1;
                while end < chars.len() && chars[end].is_ascii_digit() {
                    end += 1;
                }
            }
            let name: String = chars[s..i].iter().collect();
            let rest: String = chars[i..end].iter().collect();
            if name == old {
                out.push_str(new);
            } else {
                out.push_str(&name);
            }
            out.push_str(&rest);
            i = end;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Notation;

    fn ins() -> Vec<String> {
        ["a", "b", "c", "d"].map(String::from).to_vec()
    }

    fn p(s: &str) -> Expr {
        parse(s, &ins())
            .unwrap_or_else(|e| panic!("{s}: {e}"))
            .unwrap()
    }

    fn table(e: &Expr) -> Vec<bool> {
        (0..16u32)
            .map(|row| e.eval(&|n| row >> (3 - (n.as_bytes()[0] - b'a')) & 1 == 1))
            .collect()
    }

    fn same(a: &str, b: &str) {
        assert_eq!(table(&p(a)), table(&p(b)), "{a}  vs  {b}");
    }

    #[test]
    fn precedence_of_ascii_operators() {
        assert_eq!(
            p("a + b * c"),
            Expr::or(Expr::var("a"), Expr::and(Expr::var("b"), Expr::var("c")))
        );
        same("a + b * c", "a + (b * c)");
        same("a ^ b * c", "a ^ (b * c)");
        same("a + b ^ c", "a + (b ^ c)");
        same("~a + b", "(~a) + b");
        same("a b + c", "(a b) + c");
        same("a + b c", "a + (b c)");
    }

    #[test]
    fn identifiers_are_whole_words() {
        let e = parse("ab", &ins()).unwrap_err();
        assert!(e.message.contains("'ab' is not an input variable"), "{e}");
        let e = parse("a1", &ins()).unwrap_err();
        assert!(e.message.contains("'a1'"), "{e}");
        same("a 1", "a");
    }

    #[test]
    fn every_spelling_of_each_operator() {
        for s in [
            "a*b", "a·b", "a⋅b", "a∧b", "a&b", "a&&b", "a b", "a AND b", "a and b",
        ] {
            same(s, "a*b");
        }
        for s in ["a+b", "a∨b", "a|b", "a||b", "a OR b", "a or b"] {
            same(s, "a+b");
        }
        for s in ["a^b", "a⊕b", "a⊻b", "a≠b", "a!=b", "a≢b", "a XOR b"] {
            same(s, "a^b");
        }
        for s in ["a=b", "a==b", "a≡b", "a⇔b", "a↔b", "a⊙b", "a EQUALS b"] {
            same(s, "~(a^b)");
        }
        for s in ["~a", "!a", "-a", "¬a", "˜a", "a'", "NOT a", "not a"] {
            same(s, "~a");
        }
    }

    #[test]
    fn postfix_not_and_implicit_and() {
        same("a'b", "(~a)*b");
        same("(a+b)'", "~(a+b)");
        same("(a+b)'c", "~(a+b)*c");
        same("a(b+c)", "a*(b+c)");
        same("(a+b)(c+d)", "(a+b)*(c+d)");
        same("a~b", "a*(~b)");
        same("~~a", "a");
        same("a'' ", "a");
        same("~(a+b)c", "(~(a+b))*c");
    }

    #[test]
    fn constants_and_unicode_constants() {
        assert_eq!(p("1"), Expr::Const(true));
        assert_eq!(p("⊥"), Expr::Const(false));
        same("a 0", "0");
    }

    #[test]
    fn python_words_bind_loosest() {
        same("a and b or c", "(a and b) or c");
        same("a or b and c", "a or (b and c)");
        same("a xor b and c", "a xor (b and c)");
        same("a or b xor c", "a or (b xor c)");
    }

    #[test]
    fn printed_text_reads_back_the_same_function() {
        let samples = [
            "a b + ~c d",
            "(a + b)(~a + c)",
            "a ^ b ^ c",
            "~(a + b) * (c ^ d)",
            "a = b + c",
            "(a ^ b) = (c ^ d)",
            "~a ~b ~c + a b c",
            "a + b c' + (a ^ d)'",
            "a + (b + c)",
            "a * (b * c)",
            "(a = b) = c",
            "~~a ^ ~(b c)",
        ];
        for s in samples {
            let e = p(s);
            for n in Notation::ALL {
                if n == Notation::Latex {
                    continue;
                }
                let text = e.to_text(n);
                let back = parse(&text, &ins())
                    .unwrap_or_else(|err| panic!("{s} printed as {text:?} ({n:?}): {err}"))
                    .unwrap();
                assert_eq!(table(&e), table(&back), "{s} printed as {text:?} ({n:?})");
            }
        }
    }

    #[test]
    fn unknown_names_and_bad_characters_are_reported_with_position() {
        let e = parse("a + z", &ins()).unwrap_err();
        assert_eq!((e.offset, e.length), (4, 1));
        assert!(e.message.contains("'z' is not an input variable"));
        let e = parse("a + #", &ins()).unwrap_err();
        assert_eq!(e.offset, 4);
        assert!(e.message.contains("not allowed"));
        let e = parse("a +", &ins()).unwrap_err();
        assert!(e.message.contains("right"), "{e}");
        assert_eq!(e.offset, 2);
        let e = parse("* a", &ins()).unwrap_err();
        assert!(e.message.contains("left"), "{e}");
    }

    #[test]
    fn parentheses_errors() {
        let e = parse("(a + b", &ins()).unwrap_err();
        assert_eq!(e.offset, 0);
        assert!(e.message.contains("never closed"));
        let e = parse("a + b)", &ins()).unwrap_err();
        assert_eq!(e.offset, 5);
        assert!(e.message.contains("no matching"));
        let e = parse("()", &ins()).unwrap_err();
        assert!(e.message.contains("nothing between"), "{e}");
        let e = parse("'a", &ins()).unwrap_err();
        assert!(e.message.contains('\''));
    }

    #[test]
    fn empty_text_is_not_an_error() {
        assert_eq!(parse("", &ins()), Ok(None));
        assert_eq!(parse("   ", &ins()), Ok(None));
    }

    #[test]
    fn bit_subscripts() {
        let inputs: Vec<String> = ["A[0]", "A[1]", "b"].map(String::from).to_vec();
        let e = parse("A[1] * A:0 + A[ 0 ]b", &inputs).unwrap().unwrap();
        assert_eq!(e.variables().len(), 3);
        assert!(parse("A[", &inputs)
            .unwrap_err()
            .message
            .contains("missing ]"));
        assert!(parse("A[]", &inputs)
            .unwrap_err()
            .message
            .contains("bit number"));
        assert!(parse("A[2]", &inputs)
            .unwrap_err()
            .message
            .contains("not an input"));
    }

    #[test]
    fn assignment_to_an_output() {
        let outs = vec!["Y".to_string()];
        let r = parse_assignment("Y = a b", &ins(), &outs).unwrap().unwrap();
        assert_eq!(r.target.as_deref(), Some("Y"));
        assert_eq!(table(&r.expr), table(&p("a b")));
        let r = parse_assignment("Y := ~a", &ins(), &outs).unwrap().unwrap();
        assert_eq!(r.target.as_deref(), Some("Y"));
        let r = parse_assignment("a b", &ins(), &outs).unwrap().unwrap();
        assert_eq!(r.target, None);
        let e = parse_assignment("Y =", &ins(), &outs).unwrap_err();
        assert!(e.message.contains("right"), "{e}");
        let e = parse_assignment("a = b = c", &ins(), &outs).unwrap();
        assert!(e.is_some()); // plain XNOR chain
        let e = parse_assignment("a + Y", &ins(), &outs).unwrap_err();
        assert!(e.message.contains("left of ="), "{e}");
        let e = parse("Y = a", &ins()).unwrap_err();
        assert!(e.message.contains("not an input"), "{e}");
    }

    #[test]
    fn second_equals_after_assignment_is_an_error() {
        let outs = vec!["Y".to_string()];
        let e = parse_assignment("Y := a := b", &ins(), &outs).unwrap_err();
        assert!(e.message.contains("once"), "{e}");
    }

    #[test]
    fn replace_variable_keeps_other_text() {
        assert_eq!(replace_variable("a + b*a'", "a", "x"), "x + b*x'");
        assert_eq!(replace_variable("ab + a", "a", "x"), "ab + x");
        assert_eq!(replace_variable("A[1] + A[0]", "A", "B"), "B[1] + B[0]");
        assert_eq!(replace_variable("A:1 | b", "A", "Z"), "Z:1 | b");
    }
}
