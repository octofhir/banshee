//! DELETE statement parser.

use crate::parser::Parser;
use banshee_syntax::SyntaxKind;

use super::CLAUSE_RECOVERY;
use super::expressions::required_expr;
use super::insert::returning_clause;
use super::select::{at_ident, expect_ident, with_clause};

/// Parse DELETE statement.
///
/// ```text
/// DELETE FROM table
/// DELETE FROM table USING other_table WHERE ...
/// DELETE FROM table WHERE ...
/// DELETE FROM table RETURNING ...
/// ```
pub fn delete_stmt(p: &mut Parser<'_>) {
    let m = p.start();

    if p.at(SyntaxKind::WITH_KW) {
        with_clause(p);
    }

    p.expect(SyntaxKind::DELETE_KW);
    p.expect(SyntaxKind::FROM_KW);

    // Optional ONLY
    p.eat(SyntaxKind::ONLY_KW);

    // Table name
    table_name(p);

    // Optional alias
    if p.eat(SyntaxKind::AS_KW) {
        expect_ident(p, CLAUSE_RECOVERY);
    } else if at_ident(p) && !is_delete_keyword(p.current()) {
        p.bump();
    }

    // USING clause (PostgreSQL extension)
    if p.at(SyntaxKind::USING_KW) {
        using_clause(p);
    }

    // WHERE clause
    if p.at(SyntaxKind::WHERE_KW) {
        where_clause(p);
    }

    // RETURNING clause
    if p.at(SyntaxKind::RETURNING_KW) {
        returning_clause(p);
    }

    m.complete(p, SyntaxKind::DELETE_STMT);
}

fn table_name(p: &mut Parser<'_>) {
    let m = p.start();
    expect_ident(p, CLAUSE_RECOVERY);

    // Schema qualification
    if p.at(SyntaxKind::DOT) {
        p.bump();
        expect_ident(p, CLAUSE_RECOVERY);
    }

    m.complete(p, SyntaxKind::TABLE_REF);
}

fn using_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // USING

    table_ref(p);
    while p.eat(SyntaxKind::COMMA) {
        table_ref(p);
    }

    m.complete(p, SyntaxKind::USING_CLAUSE);
}

fn table_ref(p: &mut Parser<'_>) {
    let m = p.start();

    // Table name
    expect_ident(p, CLAUSE_RECOVERY);

    // Schema qualification
    if p.at(SyntaxKind::DOT) {
        p.bump();
        expect_ident(p, CLAUSE_RECOVERY);
    }

    // Optional alias
    if p.eat(SyntaxKind::AS_KW) {
        expect_ident(p, CLAUSE_RECOVERY);
    } else if at_ident(p) && !is_clause_keyword(p.current()) {
        p.bump();
    }

    m.complete(p, SyntaxKind::TABLE_REF);
}

fn where_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // WHERE

    // WHERE CURRENT OF cursor_name (for cursor-based deletes)
    if p.eat(SyntaxKind::CURRENT_KW) {
        p.expect(SyntaxKind::OF_KW);
        expect_ident(p, CLAUSE_RECOVERY);
    } else {
        required_expr(p);
    }

    m.complete(p, SyntaxKind::WHERE_CLAUSE);
}

fn is_delete_keyword(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::USING_KW | SyntaxKind::WHERE_KW | SyntaxKind::RETURNING_KW
    )
}

fn is_clause_keyword(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::USING_KW
            | SyntaxKind::WHERE_KW
            | SyntaxKind::RETURNING_KW
            | SyntaxKind::JOIN_KW
            | SyntaxKind::LEFT_KW
            | SyntaxKind::RIGHT_KW
            | SyntaxKind::INNER_KW
            | SyntaxKind::CROSS_KW
            | SyntaxKind::COMMA
    )
}
