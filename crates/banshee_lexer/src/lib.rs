//! SQL lexer for tokenizing PostgreSQL dialect input.
//!
//! This crate exposes a single entry point, `tokenize`, which returns a flat
//! stream of tokens with lengths suitable for CST parsing.
//!
//! # Usage
//!
//! ```ignore
//! let tokens = banshee_lexer::tokenize("SELECT 1");
//! assert!(!tokens.is_empty());
//! ```

mod cursor;

use banshee_syntax::{SyntaxKind, Token, keyword_from_str};
use cursor::Cursor;
use text_size::TextSize;

pub fn tokenize(source: &str) -> Vec<Token> {
    tokenize_sql(source, false)
}

/// Tokenizes SQL with question-mark parameters enabled.
pub fn tokenize_with_question_mark_parameters(source: &str) -> Vec<Token> {
    tokenize_sql(source, true)
}

fn tokenize_sql(source: &str, question_mark_parameters: bool) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut cursor = Cursor::new(source);

    while !cursor.is_eof() {
        let start = cursor.pos();
        let kind = scan_token(&mut cursor, question_mark_parameters);
        let len = TextSize::new((cursor.pos() - start) as u32);
        tokens.push(Token::new(kind, len));
    }

    tokens
}

fn scan_token(c: &mut Cursor<'_>, question_mark_parameters: bool) -> SyntaxKind {
    let first = c.first();

    // Whitespace (excluding newline)
    if is_horizontal_whitespace(first) {
        return scan_whitespace(c);
    }

    // Newline
    if matches!(first, '\n' | '\r') {
        c.bump();
        if first == '\r' && c.first() == '\n' {
            c.bump();
        }
        return SyntaxKind::NEWLINE;
    }

    // Line comment: --
    if first == '-' && c.second() == '-' {
        return scan_line_comment(c);
    }

    // Block comment: /* */
    if first == '/' && c.second() == '*' {
        return scan_block_comment(c);
    }

    // String literal: 'text'
    if first == '\'' {
        return scan_string(c, SyntaxKind::STRING, false);
    }

    // Quoted identifier: "name"
    if first == '"' {
        return scan_quoted_ident(c);
    }

    // Bit string: B'101' or X'FF'
    if (first == 'b' || first == 'B') && c.second() == '\'' {
        c.bump();
        return scan_string(c, SyntaxKind::BIT_STRING, false);
    }
    if (first == 'x' || first == 'X') && c.second() == '\'' {
        c.bump();
        return scan_string(c, SyntaxKind::HEX_STRING, false);
    }

    // Escape string: E'...\n...'
    if (first == 'e' || first == 'E') && c.second() == '\'' {
        c.bump();
        return scan_string(c, SyntaxKind::STRING, true);
    }

    // Unicode string/identifier: U&'...' / U&"..."
    if (first == 'u' || first == 'U') && c.second() == '&' {
        if c.third() == '\'' {
            c.bump(); // U
            c.bump(); // &
            let kind = scan_string(c, SyntaxKind::STRING, false);
            return scan_unicode_escape(c, kind);
        }
        if c.third() == '"' {
            c.bump(); // U
            c.bump(); // &
            let kind = scan_quoted_ident(c);
            return scan_unicode_escape(c, kind);
        }
    }

    // Dollar handling: dollar-quoted strings or parameters
    if first == '$' {
        // Check for dollar-quoted string: $$...$$ or $tag$...$tag$
        if c.second() == '$' {
            // $$...$$ form
            return scan_dollar_string(c);
        }
        // Check for $tag$ form - need to look ahead for closing $
        if is_ident_start(c.second()) {
            if is_dollar_string_start(c) {
                return scan_dollar_string(c);
            }
            c.bump();
            c.bump_while(is_ident_continue);
            return SyntaxKind::ERROR;
        }
        // $1, $2, etc. - positional parameter
        if c.second().is_ascii_digit() {
            return scan_positional_param(c);
        }
        c.bump();
        return SyntaxKind::DOLLAR;
    }

    // Numbers: digits or .digits
    if first.is_ascii_digit() {
        return scan_number(c);
    }
    if first == '.' && c.second().is_ascii_digit() {
        return scan_number(c);
    }

    // Identifiers and keywords
    if is_ident_start(first) {
        return scan_ident_or_keyword(c);
    }

    // Operators and punctuation
    scan_operator_or_punct(c, question_mark_parameters)
}

fn scan_unicode_escape(c: &mut Cursor<'_>, kind: SyntaxKind) -> SyntaxKind {
    if kind == SyntaxKind::ERROR {
        return kind;
    }
    let mut tail = c.clone();
    let whitespace = |ch| is_horizontal_whitespace(ch) || matches!(ch, '\r' | '\n');
    tail.bump_while(whitespace);
    if !tail
        .remaining()
        .as_bytes()
        .get(..7)
        .is_some_and(|word| word.eq_ignore_ascii_case(b"UESCAPE"))
    {
        return kind;
    }
    tail.advance_by(7);
    if is_ident_continue(tail.first()) {
        return kind;
    }
    tail.bump_while(whitespace);
    let escaped = matches!(tail.first(), 'e' | 'E') && tail.second() == '\'';
    if escaped {
        tail.bump();
    }
    let valid = if tail.first() == '\'' {
        let start = tail.pos();
        scan_string(&mut tail, SyntaxKind::STRING, escaped);
        matches!(
            tail.slice_from(start).as_bytes(),
            [b'\'', escape, b'\''] if escape.is_ascii()
                && !escape.is_ascii_hexdigit()
                && (!escaped || *escape != b'\\')
                && !b"+'\" \t\r\n\x0b\x0c".contains(escape)
        )
    } else {
        false
    };
    // The escape marker and the quoted text form one literal or identifier.
    *c = tail;
    if valid { kind } else { SyntaxKind::ERROR }
}

fn scan_whitespace(c: &mut Cursor<'_>) -> SyntaxKind {
    c.bump_while(is_horizontal_whitespace);
    SyntaxKind::WHITESPACE
}

fn is_horizontal_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\x0b' | '\x0c')
}

fn scan_line_comment(c: &mut Cursor<'_>) -> SyntaxKind {
    c.bump(); // -
    c.bump(); // -
    c.bump_while(|ch| !matches!(ch, '\n' | '\r'));
    SyntaxKind::LINE_COMMENT
}

fn scan_block_comment(c: &mut Cursor<'_>) -> SyntaxKind {
    c.bump(); // /
    c.bump(); // *
    let mut depth = 1;

    while !c.is_eof() && depth > 0 {
        if c.first() == '/' && c.second() == '*' {
            c.bump();
            c.bump();
            depth += 1;
        } else if c.first() == '*' && c.second() == '/' {
            c.bump();
            c.bump();
            depth -= 1;
        } else {
            c.bump();
        }
    }
    if depth == 0 {
        SyntaxKind::BLOCK_COMMENT
    } else {
        SyntaxKind::ERROR
    }
}

fn scan_string(c: &mut Cursor<'_>, kind: SyntaxKind, escapes: bool) -> SyntaxKind {
    c.bump(); // opening '
    loop {
        match c.first() {
            '\\' if escapes => {
                c.bump();
                c.bump();
            }
            '\'' => {
                c.bump();
                if kind == SyntaxKind::STRING && c.first() == '\'' {
                    c.bump();
                } else if let Some(chars) = string_continuation(c.remaining()) {
                    c.advance_by(chars);
                } else {
                    return kind;
                }
            }
            '\0' => return SyntaxKind::ERROR,
            _ => {
                c.bump();
            }
        }
    }
}

// Adjacent quoted strings separated by whitespace containing a newline are
// one token. Keep their separator and the original string mode.
fn string_continuation(source: &str) -> Option<usize> {
    let mut lookahead = Cursor::new(source);
    let mut newline = false;
    loop {
        lookahead.bump_while(is_horizontal_whitespace);
        match lookahead.first() {
            '\n' | '\r' => {
                newline = true;
                lookahead.bump();
            }
            '\'' if newline => {
                lookahead.bump();
                return Some(source[..lookahead.pos()].chars().count());
            }
            _ => return None,
        }
    }
}

fn scan_quoted_ident(c: &mut Cursor<'_>) -> SyntaxKind {
    c.bump(); // opening "
    loop {
        match c.first() {
            '"' => {
                c.bump();
                // Check for escaped quote ""
                if c.first() == '"' {
                    c.bump();
                    continue;
                }
                break;
            }
            '\0' => return SyntaxKind::ERROR,
            _ => {
                c.bump();
            }
        }
    }
    SyntaxKind::QUOTED_IDENT
}

fn is_dollar_string_start(c: &Cursor<'_>) -> bool {
    // Check if $tag$ pattern exists (need closing $ after tag)
    let remaining = c.remaining();
    if !remaining.starts_with('$') {
        return false;
    }
    let after_dollar = &remaining[1..];
    // Find where the tag ends
    let tag_end = after_dollar
        .find(|ch: char| !is_dollar_tag_char(ch))
        .unwrap_or(after_dollar.len());
    if tag_end == 0 {
        return false;
    }
    // Check if there's a $ after the tag
    after_dollar.get(tag_end..tag_end + 1) == Some("$")
}

fn scan_dollar_string(c: &mut Cursor<'_>) -> SyntaxKind {
    let start = c.pos();
    c.bump(); // $

    // Scan optional tag (may be empty for $$...$$).
    c.bump_while(is_dollar_tag_char);
    c.bump(); // closing $ of tag

    let tag = c.slice_from(start);

    // Scan until matching closing tag
    loop {
        if c.is_eof() {
            return SyntaxKind::ERROR;
        }
        if c.first() == '$' && c.starts_with(tag) {
            c.advance_by(tag.chars().count());
            break;
        }
        c.bump();
    }

    SyntaxKind::DOLLAR_STRING
}

fn scan_positional_param(c: &mut Cursor<'_>) -> SyntaxKind {
    c.bump(); // $
    let start = c.pos();
    c.bump_while(|ch| ch.is_ascii_digit());
    let in_range = c.slice_from(start).parse::<i32>().is_ok();
    finish_number(c, SyntaxKind::PARAM, in_range)
}

fn scan_number(c: &mut Cursor<'_>) -> SyntaxKind {
    if c.first() == '0' {
        let radix = match c.second() {
            'x' | 'X' => 16,
            'o' | 'O' => 8,
            'b' | 'B' => 2,
            _ => 10,
        };
        if radix != 10 {
            c.advance_by(2);
            if c.first() == '_' {
                c.bump();
            }
            let valid = scan_digits(c, radix);
            return finish_number(c, SyntaxKind::INTEGER, valid);
        }
    }
    scan_digits(c, 10);
    // PostgreSQL leaves both dots in `1..10` for the surrounding grammar.
    let has_dot = c.first() == '.' && c.second() != '.';
    if has_dot {
        c.bump();
        scan_digits(c, 10);
    }
    let has_exponent = matches!(c.first(), 'e' | 'E');
    let valid = if has_exponent {
        c.bump();
        if matches!(c.first(), '+' | '-') {
            c.bump();
        }
        scan_digits(c, 10)
    } else {
        true
    };
    let kind = if has_dot || has_exponent {
        SyntaxKind::FLOAT
    } else {
        SyntaxKind::INTEGER
    };
    finish_number(c, kind, valid)
}

fn scan_digits(c: &mut Cursor<'_>, radix: u32) -> bool {
    if !c.first().is_digit(radix) {
        return false;
    }
    c.bump_while(|ch| ch.is_digit(radix));
    while c.first() == '_' && c.second().is_digit(radix) {
        c.advance_by(2);
        c.bump_while(|ch| ch.is_digit(radix));
    }
    true
}

fn finish_number(c: &mut Cursor<'_>, kind: SyntaxKind, valid: bool) -> SyntaxKind {
    if valid && !is_ident_continue(c.first()) {
        kind
    } else {
        c.bump_while(is_ident_continue);
        SyntaxKind::ERROR
    }
}

fn scan_ident_or_keyword(c: &mut Cursor<'_>) -> SyntaxKind {
    let start = c.pos();
    c.bump();
    c.bump_while(is_ident_continue);
    let text = c.slice_from(start);

    keyword_from_str(text).unwrap_or(SyntaxKind::IDENT)
}

fn is_ident_start(ch: char) -> bool {
    // PostgreSQL accepts every high-bit byte in an unquoted identifier.
    ch.is_ascii_alphabetic() || ch == '_' || !ch.is_ascii()
}

fn is_ident_continue(ch: char) -> bool {
    is_dollar_tag_char(ch) || ch == '$'
}

fn is_dollar_tag_char(ch: char) -> bool {
    is_ident_start(ch) || ch.is_ascii_digit()
}

fn scan_operator_or_punct(c: &mut Cursor<'_>, question_mark_parameters: bool) -> SyntaxKind {
    use SyntaxKind::*;
    if c.starts_with("::") {
        c.advance_by(2);
        return DOUBLE_COLON;
    }
    if c.first().is_ascii() && is_operator_byte(c.first() as u8) {
        return scan_operator(c, question_mark_parameters);
    }
    match c.bump() {
        Some(';') => SEMICOLON,
        Some(',') => COMMA,
        Some('.') => DOT,
        Some(':') => COLON,
        Some('(') => L_PAREN,
        Some(')') => R_PAREN,
        Some('[') => L_BRACKET,
        Some(']') => R_BRACKET,
        Some('{') => L_BRACE,
        Some('}') => R_BRACE,
        _ => ERROR,
    }
}

fn is_operator_byte(byte: u8) -> bool {
    b"+-*/<>=~!@#%^&|`?".contains(&byte)
}

fn scan_operator(c: &mut Cursor<'_>, question_mark_parameters: bool) -> SyntaxKind {
    let mut end = 0;
    let remaining = c.remaining().as_bytes();
    while remaining
        .get(end)
        .is_some_and(|&byte| is_operator_byte(byte))
    {
        if matches!(remaining.get(end..end + 2), Some(b"--" | b"/*")) {
            break;
        }
        end += 1;
    }
    let operator = &c.remaining()[..end];
    // A trailing sign belongs to the next token unless a nonstandard
    // operator character occurs earlier in the run.
    let mut operator = if operator.bytes().any(|byte| b"~!@#^&|`?%".contains(&byte)) {
        operator
    } else {
        let end = operator.trim_end_matches(['+', '-']).len().max(1);
        &operator[..end]
    };
    if question_mark_parameters
        && operator_kind(operator).is_none()
        && let Some(index) = operator.find('?')
    {
        let (prefix, after) = operator.split_at(index);
        if !prefix.is_empty() && operator_kind(prefix).is_some() {
            operator = prefix;
        } else if prefix.is_empty() && operator_kind(&after[1..]).is_some() {
            operator = &operator[..1];
        }
    }
    c.advance_by(operator.len());
    if operator.len() >= 64 {
        SyntaxKind::ERROR
    } else {
        operator_kind(operator).unwrap_or(SyntaxKind::GENERIC_OP)
    }
}

fn operator_kind(text: &str) -> Option<SyntaxKind> {
    use SyntaxKind::*;
    Some(match text {
        "->>" => ARROW_TEXT,
        "#>>" => HASH_ARROW_TEXT,
        "!~*" => BANG_TILDE_STAR,
        "->" => ARROW,
        "#>" => HASH_ARROW,
        "#-" => HASH_MINUS,
        "@>" => AT_GT,
        "<@" => LT_AT,
        "?|" => QUESTION_PIPE,
        "?&" => QUESTION_AMP,
        "@?" => AT_QUESTION,
        "@@" => AT_AT,
        "<>" | "!=" => NE,
        "<=" => LE,
        ">=" => GE,
        "||" => PIPE_PIPE,
        "~*" => TILDE_STAR,
        "!~" => BANG_TILDE,
        "<<" => SHL,
        ">>" => SHR,
        "=>" => FAT_ARROW,
        "=" => EQ,
        "<" => LT,
        ">" => GT,
        "+" => PLUS,
        "-" => MINUS,
        "*" => STAR,
        "/" => SLASH,
        "%" => PERCENT,
        "^" => CARET,
        "~" => TILDE,
        "&" => AMP,
        "|" => PIPE,
        "#" => HASH,
        "?" => QUESTION,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use banshee_syntax::SyntaxKind::*;

    fn token_kinds(source: &str) -> Vec<SyntaxKind> {
        tokenize(source).into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn postgres_numbers_and_parameter_boundaries() {
        for number in ["1_000", "0x_FF", "0o17", "0b10_10"] {
            assert_eq!(token_kinds(number), [INTEGER], "{number}");
        }
        for number in ["1.", ".5", "1.2_3e+4_5"] {
            assert_eq!(token_kinds(number), [FLOAT], "{number}");
        }
        for invalid in ["1e+", "123abc", "$1oops", "$1_2", "0b102"] {
            assert_eq!(token_kinds(invalid), [ERROR], "{invalid}");
        }
        assert_eq!(token_kinds("1..10"), [INTEGER, DOT, FLOAT]);
    }

    #[test]
    fn test_basic_tokens() {
        assert_eq!(
            token_kinds("SELECT * FROM users"),
            vec![
                SELECT_KW, WHITESPACE, STAR, WHITESPACE, FROM_KW, WHITESPACE, IDENT
            ]
        );
    }

    #[test]
    fn test_jsonb_operators() {
        assert_eq!(token_kinds("->"), vec![ARROW]);
        assert_eq!(token_kinds("->>"), vec![ARROW_TEXT]);
        assert_eq!(token_kinds("#>"), vec![HASH_ARROW]);
        assert_eq!(token_kinds("#>>"), vec![HASH_ARROW_TEXT]);
        assert_eq!(token_kinds("@>"), vec![AT_GT]);
        assert_eq!(token_kinds("<@"), vec![LT_AT]);
        assert_eq!(token_kinds("?"), vec![QUESTION]);
        assert_eq!(token_kinds("?|"), vec![QUESTION_PIPE]);
        assert_eq!(token_kinds("?&"), vec![QUESTION_AMP]);
        assert_eq!(token_kinds("#-"), vec![HASH_MINUS]);
        assert_eq!(token_kinds("@?"), vec![AT_QUESTION]);
        assert_eq!(token_kinds("@@"), vec![AT_AT]);
    }

    #[test]
    fn test_string_literals() {
        assert_eq!(token_kinds("'hello'"), vec![STRING]);
        assert_eq!(token_kinds("'it''s'"), vec![STRING]); // escaped quote
        assert_eq!(token_kinds("$$body$$"), vec![DOLLAR_STRING]);
        assert_eq!(token_kinds("$tag$content$tag$"), vec![DOLLAR_STRING]);
        assert_eq!(token_kinds("B'101'"), vec![BIT_STRING]);
        assert_eq!(token_kinds("X'FF'"), vec![HEX_STRING]);
    }

    #[test]
    fn test_dollar_strings_nested() {
        assert_eq!(token_kinds("$$line1\nline2$$"), vec![DOLLAR_STRING]);
        assert_eq!(
            token_kinds("$tag$body $$ inner $other$ ok$tag$"),
            vec![DOLLAR_STRING]
        );
    }

    #[test]
    fn test_copy_tokens() {
        assert_eq!(
            token_kinds("COPY users FROM '/tmp/file'"),
            vec![
                COPY_KW, WHITESPACE, IDENT, WHITESPACE, FROM_KW, WHITESPACE, STRING
            ]
        );
        assert_eq!(
            token_kinds("\\copy users from '/tmp/file'"),
            vec![
                ERROR, COPY_KW, WHITESPACE, IDENT, WHITESPACE, FROM_KW, WHITESPACE, STRING
            ]
        );
    }

    #[test]
    fn test_numbers() {
        assert_eq!(token_kinds("42"), vec![INTEGER]);
        assert_eq!(token_kinds("3.14"), vec![FLOAT]);
        assert_eq!(token_kinds(".5"), vec![FLOAT]);
        assert_eq!(token_kinds("1e10"), vec![FLOAT]);
        assert_eq!(token_kinds("1.5e-3"), vec![FLOAT]);
    }

    #[test]
    fn test_identifiers() {
        assert_eq!(token_kinds("foo"), vec![IDENT]);
        assert_eq!(token_kinds("_bar"), vec![IDENT]);
        assert_eq!(token_kinds("\"Column Name\""), vec![QUOTED_IDENT]);
    }

    #[test]
    fn test_parameters() {
        assert_eq!(token_kinds("$1"), vec![PARAM]);
        assert_eq!(token_kinds("$name"), vec![ERROR]);
    }

    #[test]
    fn test_comments() {
        assert_eq!(token_kinds("-- comment"), vec![LINE_COMMENT]);
        assert_eq!(token_kinds("/* block */"), vec![BLOCK_COMMENT]);
        assert_eq!(
            token_kinds("/* nested /* comment */ */"),
            vec![BLOCK_COMMENT]
        );
    }

    #[test]
    fn test_complex_query() {
        let sql = "SELECT data->'name'->>'first' FROM users WHERE data @> '{\"active\": true}'";
        let kinds = token_kinds(sql);
        assert!(kinds.contains(&ARROW));
        assert!(kinds.contains(&ARROW_TEXT));
        assert!(kinds.contains(&AT_GT));
    }

    #[test]
    fn test_cast_operator() {
        assert_eq!(token_kinds("foo::int"), vec![IDENT, DOUBLE_COLON, INT_KW]);
    }

    #[test]
    fn test_bitwise_and_shift_operators() {
        assert_eq!(token_kinds("&"), vec![AMP]);
        assert_eq!(token_kinds("|"), vec![PIPE]);
        assert_eq!(token_kinds("#"), vec![HASH]);
        assert_eq!(token_kinds("<<"), vec![SHL]);
        assert_eq!(token_kinds(">>"), vec![SHR]);
        assert_eq!(token_kinds("=>"), vec![FAT_ARROW]);
        // Existing multi-char operators must still win over the new single ones.
        assert_eq!(token_kinds("#>"), vec![HASH_ARROW]);
        assert_eq!(token_kinds("||"), vec![PIPE_PIPE]);
        assert_eq!(token_kinds(">="), vec![GE]);
    }

    #[test]
    fn test_escape_and_unicode_strings() {
        assert_eq!(token_kinds("E'a\\nb'"), vec![STRING]);
        assert_eq!(token_kinds("E'it\\'s'"), vec![STRING]); // escaped quote via backslash
        assert_eq!(token_kinds("U&'d\\0061t'"), vec![STRING]);
        assert_eq!(token_kinds("U&\"d\\0061t\""), vec![QUOTED_IDENT]);
    }

    #[test]
    fn test_unicode_identifiers() {
        assert_eq!(token_kinds("имя"), vec![IDENT]);
        assert_eq!(token_kinds("café"), vec![IDENT]);
        assert_eq!(token_kinds("日本語"), vec![IDENT]);
    }

    fn format_tokens(source: &str) -> String {
        let tokens = tokenize(source);
        let mut result = String::new();
        let mut pos = 0u32;
        for token in tokens {
            let end = pos + u32::from(token.len);
            let text = &source[pos as usize..end as usize];
            result.push_str(&format!("{:?}@{}..{} {:?}\n", token.kind, pos, end, text));
            pos = end;
        }
        result
    }

    #[test]
    fn snapshot_basic_select() {
        let sql = "SELECT id, name FROM users WHERE active = TRUE";
        insta::assert_snapshot!(format_tokens(sql));
    }

    #[test]
    fn snapshot_jsonb_query() {
        let sql = "SELECT data->'name'->>'first' FROM users WHERE data @> '{\"active\": true}'";
        insta::assert_snapshot!(format_tokens(sql));
    }

    #[test]
    fn snapshot_all_jsonb_operators() {
        let sql = "-> ->> #> #>> @> <@ ? ?| ?& #- @? @@";
        insta::assert_snapshot!(format_tokens(sql));
    }

    #[test]
    fn snapshot_string_literals() {
        let sql = "'hello' 'it''s' $$body$$ $tag$content$tag$ B'101' X'FF'";
        insta::assert_snapshot!(format_tokens(sql));
    }

    #[test]
    fn snapshot_complex_window_function() {
        let sql = "SELECT id, SUM(amount) OVER (PARTITION BY category ORDER BY date ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM sales";
        insta::assert_snapshot!(format_tokens(sql));
    }
}
