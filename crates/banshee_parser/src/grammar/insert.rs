//! INSERT statement parser.

use crate::parser::Parser;
use banshee_syntax::SyntaxKind;

use super::PAREN_RECOVERY;
use super::expressions::expr;
use super::select::{at_ident, at_ident_text, expect_ident, select_stmt, with_clause};

/// Parse INSERT statement.
///
/// ```text
/// INSERT INTO table [(columns)] VALUES (exprs), ...
/// INSERT INTO table [(columns)] SELECT ...
/// INSERT INTO table DEFAULT VALUES
/// ... ON CONFLICT ...
/// ... RETURNING ...
/// ```
pub fn insert_stmt(p: &mut Parser<'_>) {
    let m = p.start();

    if p.at(SyntaxKind::WITH_KW) {
        with_clause(p);
    }

    p.expect(SyntaxKind::INSERT_KW);
    p.expect(SyntaxKind::INTO_KW);

    // Table name (possibly schema-qualified)
    table_name(p);

    // Optional alias
    if p.eat(SyntaxKind::AS_KW) {
        expect_ident(p, PAREN_RECOVERY);
    } else if at_ident(p)
        && !matches!(
            p.current(),
            SyntaxKind::L_PAREN
                | SyntaxKind::VALUES_KW
                | SyntaxKind::SELECT_KW
                | SyntaxKind::DEFAULT_KW
                | SyntaxKind::WITH_KW
        )
    {
        // Check if this is an alias (not a keyword, and not the OVERRIDING clause)
        if !is_insert_keyword(p.current()) && !at_ident_text(p, "OVERRIDING") {
            p.bump();
        }
    }

    // Optional column list
    if p.at(SyntaxKind::L_PAREN) && !is_subquery_start(p) {
        column_list(p);
    }

    // Optional OVERRIDING { SYSTEM | USER } VALUE clause
    if at_ident_text(p, "OVERRIDING") {
        overriding_clause(p);
    }

    // Values source: VALUES, SELECT, or DEFAULT VALUES
    if p.at(SyntaxKind::VALUES_KW) {
        values_clause(p);
    } else if p.at(SyntaxKind::SELECT_KW) || p.at(SyntaxKind::WITH_KW) || p.at(SyntaxKind::L_PAREN)
    {
        // SELECT as source (possibly with CTE or parenthesized)
        if p.at(SyntaxKind::L_PAREN) {
            p.bump();
            select_stmt(p);
            p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
        } else {
            select_stmt(p);
        }
    } else if p.eat(SyntaxKind::DEFAULT_KW) {
        p.expect(SyntaxKind::VALUES_KW);
    }

    // ON CONFLICT clause
    if p.at(SyntaxKind::ON_KW) && p.nth(1) == SyntaxKind::CONFLICT_KW {
        on_conflict_clause(p);
    }

    // RETURNING clause
    if p.at(SyntaxKind::RETURNING_KW) {
        returning_clause(p);
    }

    m.complete(p, SyntaxKind::INSERT_STMT);
}

fn table_name(p: &mut Parser<'_>) {
    let m = p.start();
    expect_ident(p, PAREN_RECOVERY);

    // Schema qualification
    if p.at(SyntaxKind::DOT) {
        p.bump();
        expect_ident(p, PAREN_RECOVERY);
    }

    m.complete(p, SyntaxKind::TABLE_REF);
}

fn column_list(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // (

    if !p.at(SyntaxKind::R_PAREN) {
        expect_ident(p, PAREN_RECOVERY);
        while p.eat(SyntaxKind::COMMA) {
            expect_ident(p, PAREN_RECOVERY);
        }
    }

    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    m.complete(p, SyntaxKind::INSERT_COLUMNS);
}

fn values_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // VALUES

    values_row(p);
    while p.eat(SyntaxKind::COMMA) {
        values_row(p);
    }

    m.complete(p, SyntaxKind::VALUES_CLAUSE);
}

fn values_row(p: &mut Parser<'_>) {
    let m = p.start();
    p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);

    if !p.at(SyntaxKind::R_PAREN) {
        value_expr(p);
        while p.eat(SyntaxKind::COMMA) {
            value_expr(p);
        }
    }

    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    m.complete(p, SyntaxKind::VALUES_ROW);
}

fn value_expr(p: &mut Parser<'_>) {
    if p.eat(SyntaxKind::DEFAULT_KW) {
        // DEFAULT value
    } else {
        expr(p);
    }
}

fn on_conflict_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // ON
    p.bump(); // CONFLICT

    // Conflict target (optional)
    if p.at(SyntaxKind::L_PAREN) {
        conflict_target(p);
    } else if p.eat(SyntaxKind::ON_KW) {
        // ON CONSTRAINT constraint_name
        p.expect(SyntaxKind::CONSTRAINT_KW);
        expect_ident(p, PAREN_RECOVERY);
    }

    // Conflict action
    p.expect(SyntaxKind::DO_KW);

    if p.eat(SyntaxKind::NOTHING_KW) {
        // DO NOTHING
    } else if p.eat(SyntaxKind::UPDATE_KW) {
        // DO UPDATE SET ...
        do_update(p);
    }

    m.complete(p, SyntaxKind::ON_CONFLICT_CLAUSE);
}

fn conflict_target(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // (

    // Column names or expressions
    expr(p);
    while p.eat(SyntaxKind::COMMA) {
        expr(p);
    }

    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);

    // Optional WHERE clause for partial index
    if p.at(SyntaxKind::WHERE_KW) {
        p.bump();
        expr(p);
    }

    m.complete(p, SyntaxKind::CONFLICT_TARGET);
}

fn do_update(p: &mut Parser<'_>) {
    let m = p.start();
    p.expect(SyntaxKind::SET_KW);

    set_item(p);
    while p.eat(SyntaxKind::COMMA) {
        set_item(p);
    }

    // Optional WHERE clause
    if p.at(SyntaxKind::WHERE_KW) {
        p.bump();
        expr(p);
    }

    m.complete(p, SyntaxKind::DO_UPDATE);
}

fn set_item(p: &mut Parser<'_>) {
    let m = p.start();

    // Column name or (col1, col2, ...)
    if p.at(SyntaxKind::L_PAREN) {
        p.bump();
        expect_ident(p, PAREN_RECOVERY);
        while p.eat(SyntaxKind::COMMA) {
            expect_ident(p, PAREN_RECOVERY);
        }
        p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    } else {
        expect_ident(p, PAREN_RECOVERY);
    }

    p.expect(SyntaxKind::EQ);

    // Value: expr, DEFAULT, or (subquery)
    if p.eat(SyntaxKind::DEFAULT_KW) {
        // DEFAULT
    } else if p.at(SyntaxKind::L_PAREN) && is_subquery_start(p) {
        // Row subquery
        p.bump();
        select_stmt(p);
        p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    } else {
        expr(p);
    }

    m.complete(p, SyntaxKind::SET_ITEM);
}

pub fn returning_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // RETURNING

    expr(p);
    // Optional alias
    if p.eat(SyntaxKind::AS_KW) {
        expect_ident(p, PAREN_RECOVERY);
    } else if at_ident(p) && !is_clause_end(p.current()) {
        p.bump();
    }

    while p.eat(SyntaxKind::COMMA) {
        expr(p);
        if p.eat(SyntaxKind::AS_KW) {
            expect_ident(p, PAREN_RECOVERY);
        } else if at_ident(p) && !is_clause_end(p.current()) {
            p.bump();
        }
    }

    m.complete(p, SyntaxKind::RETURNING_CLAUSE);
}

fn is_subquery_start(p: &Parser<'_>) -> bool {
    if !p.at(SyntaxKind::L_PAREN) {
        return false;
    }
    // Look ahead to see if this is (SELECT ...) or (WITH ...)
    p.nth(1) == SyntaxKind::SELECT_KW || p.nth(1) == SyntaxKind::WITH_KW
}

/// `OVERRIDING { SYSTEM | USER } VALUE` — controls identity-column overrides.
/// None of these words are reserved keywords, so they are matched by text.
fn overriding_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // OVERRIDING
    if at_ident_text(p, "SYSTEM") || at_ident_text(p, "USER") {
        p.bump();
    } else {
        p.error("expected SYSTEM or USER after OVERRIDING".to_string());
    }
    if at_ident_text(p, "VALUE") {
        p.bump();
    } else {
        p.error("expected VALUE after OVERRIDING SYSTEM/USER".to_string());
    }
    m.complete(p, SyntaxKind::OVERRIDING_CLAUSE);
}

fn is_insert_keyword(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::VALUES_KW
            | SyntaxKind::SELECT_KW
            | SyntaxKind::DEFAULT_KW
            | SyntaxKind::ON_KW
            | SyntaxKind::RETURNING_KW
            | SyntaxKind::WITH_KW
    )
}

fn is_clause_end(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::SEMICOLON | SyntaxKind::R_PAREN | SyntaxKind::COMMA
    )
}
