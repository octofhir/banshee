//! Remaining commands: CALL, DO, VACUUM, ANALYZE, COPY, GRANT, REVOKE, MERGE.
//!
//! Option-heavy commands keep a structured head and consume their tails
//! positionally. MERGE structures its INTO/USING/ON head and its WHEN list.

use crate::parser::Parser;
use banshee_syntax::SyntaxKind;

use super::PAREN_RECOVERY;
use super::expressions::expr;
use super::select::{at_query_start, expect_ident, select_stmt};

pub fn call_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // CALL
    expr(p); // procedure(args)
    m.complete(p, SyntaxKind::CALL_STMT);
}

pub fn do_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // DO
    consume_to_semi(p); // [LANGUAGE name] $$ body $$
    m.complete(p, SyntaxKind::DO_STMT);
}

pub fn vacuum_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // VACUUM
    consume_to_semi(p);
    m.complete(p, SyntaxKind::VACUUM_STMT);
}

pub fn analyze_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // ANALYZE / ANALYSE
    consume_to_semi(p);
    m.complete(p, SyntaxKind::ANALYZE_STMT);
}

pub fn reindex_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // REINDEX (lexed as an identifier)
    consume_to_semi(p); // [(VERBOSE)] { INDEX | TABLE | SCHEMA | DATABASE } [CONCURRENTLY] name
    m.complete(p, SyntaxKind::REINDEX_STMT);
}

pub fn cluster_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // CLUSTER (lexed as an identifier)
    consume_to_semi(p); // [VERBOSE] [table [USING index]]
    m.complete(p, SyntaxKind::CLUSTER_STMT);
}

pub fn copy_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // COPY
    consume_to_semi(p); // table (cols) FROM/TO … | ( query ) TO …
    m.complete(p, SyntaxKind::COPY_STMT);
}

pub fn grant_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // GRANT
    consume_to_semi(p);
    m.complete(p, SyntaxKind::GRANT_STMT);
}

pub fn revoke_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // REVOKE
    consume_to_semi(p);
    m.complete(p, SyntaxKind::REVOKE_STMT);
}

pub fn merge_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // MERGE
    p.expect(SyntaxKind::INTO_KW);
    relation_with_alias(p);

    p.expect(SyntaxKind::USING_KW);
    merge_source(p);

    p.expect(SyntaxKind::ON_KW);
    expr(p);

    while p.at(SyntaxKind::WHEN_KW) {
        merge_when(p);
    }

    consume_to_semi(p); // RETURNING …
    m.complete(p, SyntaxKind::MERGE_STMT);
}

fn merge_source(p: &mut Parser<'_>) {
    if p.at(SyntaxKind::L_PAREN) {
        p.bump();
        if at_query_start(p) {
            select_stmt(p);
        } else {
            expr(p);
        }
        p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
        // optional alias
        if p.eat(SyntaxKind::AS_KW) {
            expect_ident(p, PAREN_RECOVERY);
        } else if at_ident_or_quoted(p) {
            p.bump();
        }
    } else {
        relation_with_alias(p);
    }
}

fn merge_when(p: &mut Parser<'_>) {
    p.bump(); // WHEN
    p.eat(SyntaxKind::NOT_KW);
    p.expect(SyntaxKind::MATCHED_KW);
    if p.eat(SyntaxKind::AND_KW) {
        expr(p);
    }
    p.expect(SyntaxKind::THEN_KW);
    // Action: UPDATE SET … | DELETE | INSERT … | DO NOTHING. Consume up to the
    // next top-level WHEN or the statement boundary, ignoring WHEN inside a
    // nested CASE expression or parentheses.
    let mut paren = 0i32;
    let mut case = 0i32;
    while !p.at_end() {
        match p.current() {
            SyntaxKind::L_PAREN => paren += 1,
            SyntaxKind::R_PAREN if paren > 0 => paren -= 1,
            SyntaxKind::CASE_KW => case += 1,
            SyntaxKind::END_KW if case > 0 => case -= 1,
            SyntaxKind::WHEN_KW if paren == 0 && case == 0 => break,
            SyntaxKind::SEMICOLON if paren == 0 => break,
            _ => {}
        }
        p.bump_any();
    }
}

fn relation_with_alias(p: &mut Parser<'_>) {
    let m = p.start();
    expect_ident(p, PAREN_RECOVERY);
    if p.eat(SyntaxKind::DOT) {
        expect_ident(p, PAREN_RECOVERY);
    }
    if p.eat(SyntaxKind::AS_KW) {
        expect_ident(p, PAREN_RECOVERY);
    } else if at_ident_or_quoted(p) {
        p.bump();
    }
    m.complete(p, SyntaxKind::TABLE_REF);
}

fn at_ident_or_quoted(p: &Parser<'_>) -> bool {
    matches!(p.current(), SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT)
}

fn consume_to_semi(p: &mut Parser<'_>) {
    while !p.at_end() && !p.at(SyntaxKind::SEMICOLON) {
        p.bump_any();
    }
}
