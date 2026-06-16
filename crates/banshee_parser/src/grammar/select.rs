use crate::parser::{ParseContext, Parser};
use crate::token_set::TokenSet;
use banshee_syntax::SyntaxKind;

use super::expressions::expr;
use super::expressions::frame_clause;
use super::{CLAUSE_RECOVERY, JOIN_RECOVERY, PAREN_RECOVERY, SUBQUERY_RECOVERY};

/// Checks if the current token is an identifier (IDENT or QUOTED_IDENT).
pub fn at_ident(p: &Parser<'_>) -> bool {
    matches!(p.current(), SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT)
}

/// Expects an identifier, with recovery on failure. Accepts a real identifier,
/// a quoted identifier, or any non-reserved keyword (PostgreSQL's `ColId`), so
/// unreserved keywords like `type` or `value` can name columns and objects.
pub fn expect_ident(p: &mut Parser<'_>, recovery: TokenSet) {
    if at_ident(p) {
        p.bump();
    } else if p.current().can_be_identifier() {
        // A non-reserved keyword in identifier position: record it as an IDENT.
        p.bump_remap(SyntaxKind::IDENT);
    } else {
        p.expect_recover(SyntaxKind::IDENT, recovery);
    }
}

/// True at the start of a query: SELECT, WITH, VALUES, or TABLE.
pub fn at_query_start(p: &Parser<'_>) -> bool {
    matches!(
        p.current(),
        SyntaxKind::SELECT_KW | SyntaxKind::WITH_KW | SyntaxKind::VALUES_KW | SyntaxKind::TABLE_KW
    )
}

/// True when the current identifier matches `word` (case-insensitive). Used for
/// PostgreSQL words that are not reserved keywords (ROLLUP, CUBE, TABLESAMPLE…).
pub fn at_ident_text(p: &Parser<'_>, word: &str) -> bool {
    at_ident(p) && p.current_text().eq_ignore_ascii_case(word)
}

pub fn select_stmt(p: &mut Parser<'_>) {
    let m = p.start();

    // WITH clause (CTEs)
    if p.at(SyntaxKind::WITH_KW) {
        with_clause(p);
    }

    query_primary(p);

    // Set operations: UNION, INTERSECT, EXCEPT
    while p.at(SyntaxKind::UNION_KW)
        || p.at(SyntaxKind::INTERSECT_KW)
        || p.at(SyntaxKind::EXCEPT_KW)
    {
        p.bump();
        let _ = p.eat(SyntaxKind::ALL_KW) || p.eat(SyntaxKind::DISTINCT_KW);
        query_primary(p);
    }

    // ORDER BY (for the whole query)
    if p.at(SyntaxKind::ORDER_KW) {
        order_by_clause(p);
    }

    // LIMIT / OFFSET
    if p.at(SyntaxKind::LIMIT_KW) {
        limit_clause(p);
    }
    if p.at(SyntaxKind::OFFSET_KW) {
        offset_clause(p);
    }

    // FETCH (SQL standard)
    if p.at(SyntaxKind::FETCH_KW) {
        fetch_clause(p);
    }

    // FOR UPDATE/SHARE
    if p.at(SyntaxKind::FOR_KW) {
        for_clause(p);
    }

    m.complete(p, SyntaxKind::SELECT_STMT);
}

/// One query term: a parenthesized query, VALUES, TABLE, or a SELECT core.
fn query_primary(p: &mut Parser<'_>) {
    match p.current() {
        SyntaxKind::L_PAREN => {
            p.bump(); // (
            p.push_context(ParseContext::Subquery);
            select_stmt(p);
            p.pop_context();
            p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
        }
        SyntaxKind::VALUES_KW => values_query(p),
        SyntaxKind::TABLE_KW => table_query(p),
        _ => select_core(p),
    }
}

/// `VALUES (…), (…)` as a query term.
fn values_query(p: &mut Parser<'_>) {
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
        expr(p);
        while p.eat(SyntaxKind::COMMA) {
            expr(p);
        }
    }
    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    m.complete(p, SyntaxKind::VALUES_ROW);
}

/// `TABLE name` — shorthand for `SELECT * FROM name`.
fn table_query(p: &mut Parser<'_>) {
    p.bump(); // TABLE
    let m = p.start();
    expect_ident(p, CLAUSE_RECOVERY);
    if p.eat(SyntaxKind::DOT) {
        expect_ident(p, CLAUSE_RECOVERY);
    }
    m.complete(p, SyntaxKind::TABLE_REF);
}

fn select_core(p: &mut Parser<'_>) {
    p.expect_recover(SyntaxKind::SELECT_KW, CLAUSE_RECOVERY);

    // DISTINCT / DISTINCT ON / ALL
    if p.eat(SyntaxKind::DISTINCT_KW) {
        if p.eat(SyntaxKind::ON_KW) {
            p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
            expr(p);
            while p.eat(SyntaxKind::COMMA) {
                expr(p);
            }
            p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
        }
    } else {
        p.eat(SyntaxKind::ALL_KW);
    }

    // Select list
    select_list(p);

    // FROM clause
    if p.at(SyntaxKind::FROM_KW) {
        from_clause(p);
    }

    // WHERE clause
    if p.at(SyntaxKind::WHERE_KW) {
        where_clause(p);
    }

    // GROUP BY clause
    if p.at(SyntaxKind::GROUP_KW) {
        group_by_clause(p);
    }

    // HAVING clause
    if p.at(SyntaxKind::HAVING_KW) {
        having_clause(p);
    }

    // WINDOW clause
    if p.at(SyntaxKind::WINDOW_KW) {
        window_clause(p);
    }
}

pub(super) fn with_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // WITH
    p.push_context(ParseContext::WithClause);
    p.eat(SyntaxKind::RECURSIVE_KW);

    cte(p);
    while p.eat(SyntaxKind::COMMA) {
        cte(p);
    }

    p.pop_context();
    m.complete(p, SyntaxKind::WITH_CLAUSE);
}

fn cte(p: &mut Parser<'_>) {
    let m = p.start();
    expect_ident(p, PAREN_RECOVERY);

    // Optional column list
    if p.at(SyntaxKind::L_PAREN) {
        p.bump();
        expect_ident(p, PAREN_RECOVERY);
        while p.eat(SyntaxKind::COMMA) {
            expect_ident(p, PAREN_RECOVERY);
        }
        p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    }

    p.expect(SyntaxKind::AS_KW);

    // Optional MATERIALIZED / NOT MATERIALIZED hint
    if p.eat(SyntaxKind::NOT_KW) {
        p.expect(SyntaxKind::MATERIALIZED_KW);
    } else {
        p.eat(SyntaxKind::MATERIALIZED_KW);
    }

    p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);

    // CTE body: SELECT, INSERT, UPDATE, or DELETE
    match p.current() {
        SyntaxKind::SELECT_KW
        | SyntaxKind::WITH_KW
        | SyntaxKind::VALUES_KW
        | SyntaxKind::TABLE_KW
        | SyntaxKind::L_PAREN => {
            select_stmt(p);
        }
        SyntaxKind::INSERT_KW => {
            super::insert::insert_stmt(p);
        }
        SyntaxKind::UPDATE_KW => {
            super::update::update_stmt(p);
        }
        SyntaxKind::DELETE_KW => {
            super::delete::delete_stmt(p);
        }
        _ => {
            p.error("expected SELECT, INSERT, UPDATE, or DELETE in CTE body");
        }
    }

    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);

    m.complete(p, SyntaxKind::CTE);
}

fn select_list(p: &mut Parser<'_>) {
    let m = p.start();

    select_item(p);
    while p.eat(SyntaxKind::COMMA) {
        select_item(p);
    }

    m.complete(p, SyntaxKind::SELECT_ITEM_LIST);
}

fn select_item(p: &mut Parser<'_>) {
    let m = p.start();

    expr(p);

    // Optional alias
    if p.eat(SyntaxKind::AS_KW) {
        expect_ident(p, CLAUSE_RECOVERY);
    } else if at_ident(p) && !is_clause_keyword(p.current()) {
        p.bump();
    }

    m.complete(p, SyntaxKind::SELECT_ITEM);
}

fn from_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // FROM

    table_ref(p);
    while p.eat(SyntaxKind::COMMA) {
        table_ref(p);
    }

    m.complete(p, SyntaxKind::FROM_CLAUSE);
}

/// Parse a table reference (table name, subquery, or joined tables).
/// Used in FROM clauses for SELECT, UPDATE, and DELETE statements.
pub fn table_ref(p: &mut Parser<'_>) {
    let m = p.start();

    // LATERAL
    p.eat(SyntaxKind::LATERAL_KW);

    // ONLY
    p.eat(SyntaxKind::ONLY_KW);

    // Table or subquery or function call
    if p.at(SyntaxKind::L_PAREN) {
        p.bump();
        if at_query_start(p) {
            // Subquery with isolated recovery
            p.push_context(ParseContext::Subquery);
            parse_subquery_in_from(p);
            p.pop_context();
        } else {
            // Joined table in parens
            table_ref(p);
        }
        p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    } else {
        // Table name or function call (supports both IDENT and QUOTED_IDENT)
        expect_ident(p, JOIN_RECOVERY);

        // Schema qualification
        if p.at(SyntaxKind::DOT) {
            p.bump();
            expect_ident(p, JOIN_RECOVERY);
        }

        // Function call in FROM clause
        if p.at(SyntaxKind::L_PAREN) {
            p.bump();
            if !p.at(SyntaxKind::R_PAREN) {
                expr(p);
                while p.eat(SyntaxKind::COMMA) {
                    expr(p);
                }
            }
            p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);

            // WITH ORDINALITY (set-returning function in FROM). At this point a
            // WITH can only introduce ORDINALITY.
            if p.at(SyntaxKind::WITH_KW) {
                p.bump(); // WITH
                if at_ident_text(p, "ORDINALITY") {
                    p.bump();
                } else {
                    p.error("expected ORDINALITY after WITH");
                }
            }
        }

        // TABLESAMPLE method ( args ) [ REPEATABLE ( seed ) ]
        if at_ident_text(p, "TABLESAMPLE") {
            p.bump(); // TABLESAMPLE
            expect_ident(p, JOIN_RECOVERY); // sampling method
            p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
            if !p.at(SyntaxKind::R_PAREN) {
                expr(p);
                while p.eat(SyntaxKind::COMMA) {
                    expr(p);
                }
            }
            p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
            if at_ident_text(p, "REPEATABLE") {
                p.bump();
                p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
                expr(p);
                p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
            }
        }
    }

    // Alias (supports both IDENT and QUOTED_IDENT)
    if p.eat(SyntaxKind::AS_KW) {
        expect_ident(p, JOIN_RECOVERY);
        // Column aliases for functions: AS alias(col1, col2)
        if p.at(SyntaxKind::L_PAREN) {
            p.bump();
            expect_ident(p, PAREN_RECOVERY);
            while p.eat(SyntaxKind::COMMA) {
                expect_ident(p, PAREN_RECOVERY);
            }
            p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
        }
    } else if at_ident(p) && !is_join_keyword(p.current()) && !is_clause_keyword(p.current()) {
        p.bump();
        if p.at(SyntaxKind::L_PAREN) {
            p.bump();
            expect_ident(p, PAREN_RECOVERY);
            while p.eat(SyntaxKind::COMMA) {
                expect_ident(p, PAREN_RECOVERY);
            }
            p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
        }
    }

    let mut lhs = m.complete(p, SyntaxKind::TABLE_REF);

    // Joins
    loop {
        if is_join_start(p) {
            lhs = join_expr(p, lhs);
        } else {
            break;
        }
    }
}

fn is_join_start(p: &Parser<'_>) -> bool {
    matches!(
        p.current(),
        SyntaxKind::JOIN_KW
            | SyntaxKind::INNER_KW
            | SyntaxKind::LEFT_KW
            | SyntaxKind::RIGHT_KW
            | SyntaxKind::FULL_KW
            | SyntaxKind::CROSS_KW
            | SyntaxKind::NATURAL_KW
    )
}

fn is_join_keyword(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::JOIN_KW
            | SyntaxKind::INNER_KW
            | SyntaxKind::LEFT_KW
            | SyntaxKind::RIGHT_KW
            | SyntaxKind::FULL_KW
            | SyntaxKind::CROSS_KW
            | SyntaxKind::NATURAL_KW
            | SyntaxKind::ON_KW
            | SyntaxKind::USING_KW
    )
}

fn join_expr(
    p: &mut Parser<'_>,
    lhs: crate::parser::CompletedMarker,
) -> crate::parser::CompletedMarker {
    let m = lhs.precede(p);
    p.push_context(ParseContext::JoinClause);

    // NATURAL
    let natural = p.eat(SyntaxKind::NATURAL_KW);

    // Join type
    let cross = match p.current() {
        SyntaxKind::JOIN_KW | SyntaxKind::INNER_KW => {
            if p.at(SyntaxKind::INNER_KW) {
                p.bump();
            }
            p.expect_recover(SyntaxKind::JOIN_KW, JOIN_RECOVERY);
            false
        }
        SyntaxKind::LEFT_KW | SyntaxKind::RIGHT_KW | SyntaxKind::FULL_KW => {
            p.bump();
            p.eat(SyntaxKind::OUTER_KW);
            p.expect_recover(SyntaxKind::JOIN_KW, JOIN_RECOVERY);
            false
        }
        SyntaxKind::CROSS_KW => {
            p.bump();
            p.expect_recover(SyntaxKind::JOIN_KW, JOIN_RECOVERY);
            true
        }
        _ => {
            p.error("expected JOIN, LEFT/RIGHT/FULL JOIN, CROSS JOIN, or NATURAL JOIN");
            false
        }
    };

    // Right-hand table
    table_ref(p);

    // Join condition (not for CROSS JOIN or NATURAL JOIN)
    if !cross && !natural {
        if p.eat(SyntaxKind::ON_KW) {
            let cm = p.start();
            expr(p);
            cm.complete(p, SyntaxKind::JOIN_CONDITION);
        } else if p.eat(SyntaxKind::USING_KW) {
            let cm = p.start();
            p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
            expect_ident(p, PAREN_RECOVERY);
            while p.eat(SyntaxKind::COMMA) {
                expect_ident(p, PAREN_RECOVERY);
            }
            p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
            cm.complete(p, SyntaxKind::JOIN_CONDITION);
        }
    }

    p.pop_context();
    m.complete(p, SyntaxKind::JOIN_EXPR)
}

fn where_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // WHERE
    expr(p);
    m.complete(p, SyntaxKind::WHERE_CLAUSE);
}

fn group_by_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // GROUP
    p.expect(SyntaxKind::BY_KW);

    // Optional ALL / DISTINCT (grouping over the whole list).
    let _ = p.eat(SyntaxKind::ALL_KW) || p.eat(SyntaxKind::DISTINCT_KW);

    group_by_elem(p);
    while p.eat(SyntaxKind::COMMA) {
        group_by_elem(p);
    }

    m.complete(p, SyntaxKind::GROUP_BY_CLAUSE);
}

/// A GROUP BY element: a plain expression, ROLLUP/CUBE (…), or GROUPING SETS (…).
/// ROLLUP/CUBE/GROUPING/SETS are non-reserved words, matched by text.
fn group_by_elem(p: &mut Parser<'_>) {
    if at_ident_text(p, "ROLLUP") || at_ident_text(p, "CUBE") {
        p.bump(); // ROLLUP / CUBE
        p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
        grouping_set_list(p);
        p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    } else if at_ident_text(p, "GROUPING") {
        p.bump(); // GROUPING
        if at_ident_text(p, "SETS") {
            p.bump(); // SETS
        } else {
            p.error("expected SETS after GROUPING");
        }
        p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
        grouping_set_list(p);
        p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    } else {
        expr(p);
    }
}

/// Comma-separated grouping set members: plain expressions, parenthesized
/// column groups, the empty group `()`, or nested ROLLUP/CUBE/GROUPING SETS.
fn grouping_set_list(p: &mut Parser<'_>) {
    grouping_set_member(p);
    while p.eat(SyntaxKind::COMMA) {
        grouping_set_member(p);
    }
}

fn grouping_set_member(p: &mut Parser<'_>) {
    if at_ident_text(p, "ROLLUP") || at_ident_text(p, "CUBE") || at_ident_text(p, "GROUPING") {
        group_by_elem(p);
    } else if p.at(SyntaxKind::L_PAREN) {
        // ( ) empty group, or ( expr, … )
        p.bump();
        if !p.at(SyntaxKind::R_PAREN) {
            expr(p);
            while p.eat(SyntaxKind::COMMA) {
                expr(p);
            }
        }
        p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    } else {
        expr(p);
    }
}

fn having_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // HAVING
    expr(p);
    m.complete(p, SyntaxKind::HAVING_CLAUSE);
}

fn window_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // WINDOW

    // window_name AS (window_definition)
    expect_ident(p, PAREN_RECOVERY);
    p.expect(SyntaxKind::AS_KW);
    window_definition(p);

    while p.eat(SyntaxKind::COMMA) {
        expect_ident(p, PAREN_RECOVERY);
        p.expect(SyntaxKind::AS_KW);
        window_definition(p);
    }

    m.complete(p, SyntaxKind::WINDOW_SPEC);
}

fn window_definition(p: &mut Parser<'_>) {
    p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);

    if at_ident(p) {
        p.bump();
    }

    if p.at(SyntaxKind::PARTITION_KW) {
        let pm = p.start();
        p.bump();
        p.expect(SyntaxKind::BY_KW);
        expr(p);
        while p.eat(SyntaxKind::COMMA) {
            expr(p);
        }
        pm.complete(p, SyntaxKind::PARTITION_BY);
    }

    if p.at(SyntaxKind::ORDER_KW) {
        let om = p.start();
        p.bump();
        p.expect(SyntaxKind::BY_KW);
        order_by_item(p);
        while p.eat(SyntaxKind::COMMA) {
            order_by_item(p);
        }
        om.complete(p, SyntaxKind::ORDER_BY_CLAUSE);
    }

    if p.at(SyntaxKind::ROWS_KW) || p.at(SyntaxKind::RANGE_KW) || p.at(SyntaxKind::GROUPS_KW) {
        frame_clause(p);
    }

    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
}

fn order_by_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // ORDER
    p.expect(SyntaxKind::BY_KW);

    order_by_item(p);
    while p.eat(SyntaxKind::COMMA) {
        order_by_item(p);
    }

    m.complete(p, SyntaxKind::ORDER_BY_CLAUSE);
}

fn order_by_item(p: &mut Parser<'_>) {
    let m = p.start();
    expr(p);
    let _ = p.eat(SyntaxKind::ASC_KW) || p.eat(SyntaxKind::DESC_KW);
    if p.eat(SyntaxKind::NULLS_KW) {
        let _ = p.eat(SyntaxKind::FIRST_KW) || p.eat(SyntaxKind::LAST_KW);
    }
    m.complete(p, SyntaxKind::ORDER_BY_ITEM);
}

fn limit_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // LIMIT

    if p.eat(SyntaxKind::ALL_KW) {
        // LIMIT ALL
    } else {
        expr(p);
    }

    m.complete(p, SyntaxKind::LIMIT_CLAUSE);
}

fn offset_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // OFFSET
    expr(p);
    // Optional ROW/ROWS
    let _ = p.eat(SyntaxKind::ROW_KW) || p.eat(SyntaxKind::ROWS_KW);
    m.complete(p, SyntaxKind::OFFSET_CLAUSE);
}

fn fetch_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // FETCH
    let _ = p.eat(SyntaxKind::FIRST_KW) || p.eat(SyntaxKind::NEXT_KW);
    if !p.at(SyntaxKind::ROW_KW) && !p.at(SyntaxKind::ROWS_KW) {
        expr(p);
    }
    let _ = p.eat(SyntaxKind::ROW_KW) || p.eat(SyntaxKind::ROWS_KW);
    p.eat(SyntaxKind::ONLY_KW);
    m.complete(p, SyntaxKind::FETCH_CLAUSE);
}

fn for_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // FOR
    lock_strength(p);
    // Optional OF table_name
    if p.eat(SyntaxKind::OF_KW) {
        lock_relation(p);
        while p.eat(SyntaxKind::COMMA) {
            lock_relation(p);
        }
    }
    // Optional NOWAIT/SKIP LOCKED
    wait_policy(p);
    m.complete(p, SyntaxKind::FOR_CLAUSE);
}

fn lock_strength(p: &mut Parser<'_>) {
    if p.eat(SyntaxKind::UPDATE_KW) || p.eat(SyntaxKind::SHARE_KW) {
        return;
    }

    if p.eat(SyntaxKind::NO_KW) {
        p.expect(SyntaxKind::KEY_KW);
        p.expect(SyntaxKind::UPDATE_KW);
        return;
    }

    if p.eat(SyntaxKind::KEY_KW) {
        p.expect(SyntaxKind::SHARE_KW);
        return;
    }

    p.error("expected UPDATE, SHARE, NO KEY UPDATE, or KEY SHARE after FOR");
}

fn lock_relation(p: &mut Parser<'_>) {
    expect_ident(p, CLAUSE_RECOVERY);

    if p.at(SyntaxKind::DOT) {
        p.bump();
        expect_ident(p, CLAUSE_RECOVERY);
    }
}

fn wait_policy(p: &mut Parser<'_>) {
    if p.eat(SyntaxKind::NOWAIT_KW) {
        return;
    }

    if p.eat(SyntaxKind::SKIP_KW) {
        p.expect(SyntaxKind::LOCKED_KW);
    }
}

/// Parse a subquery in FROM clause with isolated error recovery.
fn parse_subquery_in_from(p: &mut Parser<'_>) {
    select_stmt(p);

    // If we're not at R_PAREN after parsing, recover within subquery bounds
    if !p.at(SyntaxKind::R_PAREN) && !p.at_end() && !p.at_set(SUBQUERY_RECOVERY) {
        let em = p.start();
        p.error_with_context("unexpected tokens in subquery");
        while !p.at_end() && !p.at_set(SUBQUERY_RECOVERY) && !p.at(SyntaxKind::R_PAREN) {
            p.bump_any();
        }
        em.complete(p, SyntaxKind::ERROR);
    }
}

/// Check if a token is a clause keyword that shouldn't be treated as an alias.
/// Includes keywords for SELECT, UPDATE, and DELETE statements.
pub fn is_clause_keyword(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        // SELECT clause keywords
        SyntaxKind::FROM_KW
            | SyntaxKind::WHERE_KW
            | SyntaxKind::GROUP_KW
            | SyntaxKind::HAVING_KW
            | SyntaxKind::ORDER_KW
            | SyntaxKind::LIMIT_KW
            | SyntaxKind::OFFSET_KW
            | SyntaxKind::FETCH_KW
            | SyntaxKind::FOR_KW
            | SyntaxKind::UNION_KW
            | SyntaxKind::INTERSECT_KW
            | SyntaxKind::EXCEPT_KW
            | SyntaxKind::WINDOW_KW
            // UPDATE/DELETE clause keywords
            | SyntaxKind::SET_KW
            | SyntaxKind::RETURNING_KW
            | SyntaxKind::USING_KW
    )
}
