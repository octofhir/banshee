//! Semantic highlighting (`textDocument/semanticTokens/full`).
//!
//! Classifies each non-trivia token by [`SyntaxKind`] and emits LSP semantic
//! tokens in the spec's delta encoding. Comments are emitted too (they carry
//! position info editors use for the comment scope).

use banshee_config::Templated;
use banshee_syntax::{SyntaxKind, SyntaxNode};
use lsp_types::{SemanticToken, SemanticTokenType, SemanticTokens};

use crate::convert::LineIndex;

/// Token-type legend; the index into this array is what tokens reference.
pub const LEGEND: &[SemanticTokenType] = &[
    SemanticTokenType::KEYWORD,   // 0
    SemanticTokenType::OPERATOR,  // 1
    SemanticTokenType::STRING,    // 2
    SemanticTokenType::NUMBER,    // 3
    SemanticTokenType::COMMENT,   // 4
    SemanticTokenType::VARIABLE,  // 5
    SemanticTokenType::PARAMETER, // 6
];

const KEYWORD: u32 = 0;
const OPERATOR: u32 = 1;
const STRING: u32 = 2;
const NUMBER: u32 = 3;
const COMMENT: u32 = 4;
const VARIABLE: u32 = 5;
const PARAMETER: u32 = 6;

fn token_type(kind: SyntaxKind) -> Option<u32> {
    if kind.is_keyword() {
        return Some(KEYWORD);
    }
    match kind {
        SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT => Some(COMMENT),
        SyntaxKind::STRING
        | SyntaxKind::DOLLAR_STRING
        | SyntaxKind::BIT_STRING
        | SyntaxKind::HEX_STRING => Some(STRING),
        SyntaxKind::INTEGER | SyntaxKind::FLOAT => Some(NUMBER),
        SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT => Some(VARIABLE),
        SyntaxKind::PARAM => Some(PARAMETER),
        SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => None,
        // Punctuation/operators: anything else that is a single token with
        // visible glyphs. Identifiers and literals are handled above; the rest
        // of the visible tokens read as operators/punctuation.
        _ => Some(OPERATOR),
    }
}

/// Builds delta-encoded semantic tokens for the whole document.
pub fn semantic_tokens(
    root: &SyntaxNode,
    index: &LineIndex,
    templated: Option<&Templated>,
) -> SemanticTokens {
    let mut data: Vec<SemanticToken> = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for element in root.descendants_with_tokens() {
        let Some(token) = element.into_token() else {
            continue;
        };
        let Some(mut ty) = token_type(token.kind()) else {
            continue;
        };
        let mut range = token.text_range();
        if let Some(templated) = templated {
            if templated.touches_placeholder(range) {
                ty = PARAMETER;
            }
            range = templated.map_range(range);
        }
        for (start, length) in index.line_segments(range) {
            let delta_line = start.line - prev_line;
            let delta_start = if delta_line == 0 {
                start.character - prev_start
            } else {
                start.character
            };

            data.push(SemanticToken {
                delta_line,
                delta_start,
                length,
                token_type: ty,
                token_modifiers_bitset: 0,
            });

            prev_line = start.line;
            prev_start = start.character;
        }
    }

    SemanticTokens {
        result_id: None,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_line_local_and_utf16_encoded() {
        let sql = "/* 😀\r\n\r\né */\rselect \"😀\", $$x\r\n😀$$;";
        let index = LineIndex::new(sql);
        let parse = banshee_parser::parse(sql);
        assert!(parse.errors().is_empty(), "{:?}", parse.errors());
        let tokens = semantic_tokens(&parse.syntax(), &index, None);
        let encoded: Vec<_> = tokens
            .data
            .iter()
            .map(|t| (t.delta_line, t.delta_start, t.length, t.token_type))
            .collect();
        assert_eq!(
            encoded,
            [
                (0, 0, 5, COMMENT),
                (2, 0, 4, COMMENT),
                (1, 0, 6, KEYWORD),
                (0, 7, 4, VARIABLE),
                (0, 4, 1, OPERATOR),
                (0, 2, 3, STRING),
                (1, 0, 4, STRING),
                (0, 4, 1, OPERATOR),
            ]
        );
    }

    #[test]
    fn parameters_and_adjoining_tokens_use_source_widths() {
        let sql = "SELECT %(😀)s,%s,$1";
        let index = LineIndex::new(sql);
        let settings = banshee_config::TemplaterSettings {
            style: Some(banshee_config::PlaceholderStyle::Percent),
        };
        let templated = settings.render(sql).unwrap();
        let parse = banshee_parser::parse(templated.text());
        let tokens = semantic_tokens(&parse.syntax(), &index, Some(&templated));
        let encoded: Vec<_> = tokens
            .data
            .iter()
            .map(|token| (token.delta_start, token.length, token.token_type))
            .collect();
        assert_eq!(
            encoded,
            [
                (0, 6, KEYWORD),
                (7, 6, PARAMETER),
                (6, 1, OPERATOR),
                (1, 2, PARAMETER),
                (2, 1, OPERATOR),
                (1, 2, PARAMETER),
            ]
        );
    }
}
