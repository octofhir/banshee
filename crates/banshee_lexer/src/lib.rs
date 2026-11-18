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
    let mut tokens = Vec::new();
    let mut cursor = Cursor::new(source);

    while !cursor.is_eof() {
        let start = cursor.pos();
        let kind = scan_token(&mut cursor);
        let len = TextSize::new((cursor.pos() - start) as u32);
        tokens.push(Token::new(kind, len));
    }

    tokens
}

fn scan_token(c: &mut Cursor<'_>) -> SyntaxKind {
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
            return scan_string(c, SyntaxKind::STRING, false);
        }
        if c.third() == '"' {
            c.bump(); // U
            c.bump(); // &
            return scan_quoted_ident(c);
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
            // Not a dollar string, treat as named parameter
            return scan_param(c);
        }
        // $1, $2, etc. - positional parameter
        if c.second().is_ascii_digit() {
            return scan_param(c);
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
    scan_operator_or_punct(c)
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

fn scan_param(c: &mut Cursor<'_>) -> SyntaxKind {
    c.bump(); // $
    c.bump_while(|ch| ch.is_ascii_alphanumeric() || ch == '_');
    SyntaxKind::PARAM
}

fn scan_number(c: &mut Cursor<'_>) -> SyntaxKind {
    let mut has_dot = false;
    let mut has_exp = false;

    // Optional leading dot
    if c.first() == '.' {
        has_dot = true;
        c.bump();
    }

    // Integer part
    c.bump_while(|ch| ch.is_ascii_digit());

    // Decimal part
    if !has_dot && c.first() == '.' && c.second().is_ascii_digit() {
        has_dot = true;
        c.bump(); // .
        c.bump_while(|ch| ch.is_ascii_digit());
    }

    // Exponent part
    if c.first() == 'e' || c.first() == 'E' {
        let next = c.second();
        if next.is_ascii_digit() || next == '+' || next == '-' {
            has_exp = true;
            c.bump(); // e/E
            if c.first() == '+' || c.first() == '-' {
                c.bump();
            }
            c.bump_while(|ch| ch.is_ascii_digit());
        }
    }

    if has_dot || has_exp {
        SyntaxKind::FLOAT
    } else {
        SyntaxKind::INTEGER
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

fn scan_operator_or_punct(c: &mut Cursor<'_>) -> SyntaxKind {
    let first = c.first();
    let second = c.second();
    let third = c.third();

    // Three-character operators
    match (first, second, third) {
        ('-', '>', '>') => {
            c.advance_by(3);
            return SyntaxKind::ARROW_TEXT;
        }
        ('#', '>', '>') => {
            c.advance_by(3);
            return SyntaxKind::HASH_ARROW_TEXT;
        }
        ('!', '~', '*') => {
            c.advance_by(3);
            return SyntaxKind::BANG_TILDE_STAR;
        }
        _ => {}
    }

    // Two-character operators
    match (first, second) {
        ('-', '>') => {
            c.advance_by(2);
            return SyntaxKind::ARROW;
        }
        ('#', '>') => {
            c.advance_by(2);
            return SyntaxKind::HASH_ARROW;
        }
        ('#', '-') => {
            c.advance_by(2);
            return SyntaxKind::HASH_MINUS;
        }
        ('@', '>') => {
            c.advance_by(2);
            return SyntaxKind::AT_GT;
        }
        ('<', '@') => {
            c.advance_by(2);
            return SyntaxKind::LT_AT;
        }
        ('?', '|') => {
            c.advance_by(2);
            return SyntaxKind::QUESTION_PIPE;
        }
        ('?', '&') => {
            c.advance_by(2);
            return SyntaxKind::QUESTION_AMP;
        }
        ('@', '?') => {
            c.advance_by(2);
            return SyntaxKind::AT_QUESTION;
        }
        ('@', '@') => {
            c.advance_by(2);
            return SyntaxKind::AT_AT;
        }
        ('<', '>') => {
            c.advance_by(2);
            return SyntaxKind::NE;
        }
        ('!', '=') => {
            c.advance_by(2);
            return SyntaxKind::NE;
        }
        ('<', '=') => {
            c.advance_by(2);
            return SyntaxKind::LE;
        }
        ('>', '=') => {
            c.advance_by(2);
            return SyntaxKind::GE;
        }
        ('|', '|') => {
            c.advance_by(2);
            return SyntaxKind::PIPE_PIPE;
        }
        (':', ':') => {
            c.advance_by(2);
            return SyntaxKind::DOUBLE_COLON;
        }
        ('~', '*') => {
            c.advance_by(2);
            return SyntaxKind::TILDE_STAR;
        }
        ('!', '~') => {
            c.advance_by(2);
            return SyntaxKind::BANG_TILDE;
        }
        ('<', '<') => {
            c.advance_by(2);
            return SyntaxKind::SHL;
        }
        ('>', '>') => {
            c.advance_by(2);
            return SyntaxKind::SHR;
        }
        ('=', '>') => {
            c.advance_by(2);
            return SyntaxKind::FAT_ARROW;
        }
        _ => {}
    }

    // Single-character operators and punctuation
    c.bump();
    match first {
        '=' => SyntaxKind::EQ,
        '<' => SyntaxKind::LT,
        '>' => SyntaxKind::GT,
        '+' => SyntaxKind::PLUS,
        '-' => SyntaxKind::MINUS,
        '*' => SyntaxKind::STAR,
        '/' => SyntaxKind::SLASH,
        '%' => SyntaxKind::PERCENT,
        '^' => SyntaxKind::CARET,
        '~' => SyntaxKind::TILDE,
        '&' => SyntaxKind::AMP,
        '|' => SyntaxKind::PIPE,
        '#' => SyntaxKind::HASH,
        '?' => SyntaxKind::QUESTION,
        ';' => SyntaxKind::SEMICOLON,
        ',' => SyntaxKind::COMMA,
        '.' => SyntaxKind::DOT,
        ':' => SyntaxKind::COLON,
        '(' => SyntaxKind::L_PAREN,
        ')' => SyntaxKind::R_PAREN,
        '[' => SyntaxKind::L_BRACKET,
        ']' => SyntaxKind::R_BRACKET,
        '{' => SyntaxKind::L_BRACE,
        '}' => SyntaxKind::R_BRACE,
        _ => SyntaxKind::ERROR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use banshee_syntax::SyntaxKind::*;

    fn token_kinds(source: &str) -> Vec<SyntaxKind> {
        tokenize(source).into_iter().map(|t| t.kind).collect()
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
        assert_eq!(token_kinds("$name"), vec![PARAM]);
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
