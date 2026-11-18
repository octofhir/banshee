use cstree::util::NodeOrToken;
use text_size::TextRange;

use crate::{SyntaxKind, SyntaxNode};

/// The source spelling of a qualified function name or a call-like expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallName {
    /// The span of the name, including any qualifiers.
    pub range: TextRange,
    /// The final name component, if function name casing can apply to it.
    pub function: Option<TextRange>,
}

/// Walks function names and call-like expression prefixes in source order.
pub fn call_names(root: &SyntaxNode) -> impl Iterator<Item = CallName> + '_ {
    root.descendants().filter_map(call_name)
}

fn call_name(node: &SyntaxNode) -> Option<CallName> {
    use SyntaxKind::*;

    let kind = node.kind();
    if !matches!(
        kind,
        FUNC_CALL
            | TABLE_REF
            | ARRAY_EXPR
            | CAST_EXPR
            | COALESCE_EXPR
            | NULLIF_EXPR
            | GREATEST_EXPR
            | LEAST_EXPR
    ) {
        return None;
    }
    let mut children = node
        .children_with_tokens()
        .filter(|element| {
            !element
                .as_token()
                .is_some_and(|token| token.kind().is_trivia())
        })
        .peekable();
    if kind == TABLE_REF {
        while children
            .next_if(|element| {
                element
                    .as_token()
                    .is_some_and(|t| matches!(t.kind(), LATERAL_KW | ONLY_KW))
            })
            .is_some()
        {}
    }
    let first = children.next()?;
    if !matches!(kind, FUNC_CALL | TABLE_REF) {
        let token = first.as_token()?;
        return matches!(
            token.kind(),
            ARRAY_KW | CAST_KW | COALESCE_KW | NULLIF_KW | GREATEST_KW | LEAST_KW
        )
        .then(|| CallName {
            range: token.text_range(),
            function: None,
        });
    }

    let (first, last) = match first {
        NodeOrToken::Token(first) if name_word(first.kind()) => {
            let mut last = first;
            while children
                .next_if(|element| element.as_token().is_some_and(|token| token.kind() == DOT))
                .is_some()
            {
                last = children
                    .next()?
                    .into_token()
                    .filter(|token| name_word(token.kind()))?;
            }
            (first, last)
        }
        NodeOrToken::Node(name) if name.kind() == QUALIFIED_NAME => {
            let mut words = name
                .descendants_with_tokens()
                .filter_map(|element| element.into_token())
                .filter(|token| name_word(token.kind()));
            let first = words.next()?;
            (first, words.last().unwrap_or(first))
        }
        _ => return None,
    };
    if kind == TABLE_REF
        && !children
            .next()
            .and_then(|element| element.into_token())
            .is_some_and(|token| token.kind() == L_PAREN)
    {
        return None;
    }
    Some(CallName {
        range: first.text_range().cover(last.text_range()),
        function: (last.kind() != QUOTED_IDENT).then(|| last.text_range()),
    })
}

fn name_word(kind: SyntaxKind) -> bool {
    kind.is_keyword() || matches!(kind, SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT)
}
