pub mod commands;
pub mod ddl;
pub mod delete;
pub mod expressions;
pub mod insert;
pub mod jsonpath;
pub mod jsonpath_lexer;
pub mod select;
pub mod update;
pub mod util;

use crate::parser::Parser;
use banshee_syntax::SyntaxKind;

pub fn root(p: &mut Parser<'_>) {
    let m = p.start();

    while !p.at_end() {
        statement(p);
        // Consume optional semicolon
        p.eat(SyntaxKind::SEMICOLON);
    }

    m.complete(p, SyntaxKind::SOURCE_FILE);
}

pub(super) fn statement(p: &mut Parser<'_>) {
    // WITH can start a CTE for SELECT, INSERT, UPDATE, or DELETE
    if p.at(SyntaxKind::WITH_KW) {
        // Look ahead to determine which statement follows the CTE
        with_statement(p);
        return;
    }

    // REINDEX and CLUSTER are non-reserved words (kept as identifiers by the
    // lexer), so dispatch on their text rather than a keyword kind.
    if select::at_ident_text(p, "reindex") {
        commands::reindex_stmt(p);
        return;
    }
    if select::at_ident_text(p, "cluster") {
        commands::cluster_stmt(p);
        return;
    }

    match p.current() {
        SyntaxKind::SELECT_KW
        | SyntaxKind::VALUES_KW
        | SyntaxKind::TABLE_KW
        | SyntaxKind::L_PAREN => {
            select::select_stmt(p);
        }
        SyntaxKind::INSERT_KW => {
            insert::insert_stmt(p);
        }
        SyntaxKind::UPDATE_KW => {
            update::update_stmt(p);
        }
        SyntaxKind::DELETE_KW => {
            delete::delete_stmt(p);
        }
        SyntaxKind::CREATE_KW => {
            ddl::create_stmt(p);
        }
        SyntaxKind::ALTER_KW => {
            ddl::alter_stmt(p);
        }
        SyntaxKind::DROP_KW => {
            ddl::drop_stmt(p);
        }
        SyntaxKind::TRUNCATE_KW => {
            ddl::truncate_stmt(p);
        }
        SyntaxKind::BEGIN_KW
        | SyntaxKind::START_KW
        | SyntaxKind::COMMIT_KW
        | SyntaxKind::END_KW
        | SyntaxKind::ROLLBACK_KW
        | SyntaxKind::ABORT_KW
        | SyntaxKind::SAVEPOINT_KW
        | SyntaxKind::RELEASE_KW => {
            util::transaction_stmt(p);
        }
        SyntaxKind::SET_KW => {
            util::set_stmt(p);
        }
        SyntaxKind::SHOW_KW => {
            util::show_stmt(p);
        }
        SyntaxKind::RESET_KW => {
            util::reset_stmt(p);
        }
        SyntaxKind::EXPLAIN_KW => {
            util::explain_stmt(p);
        }
        SyntaxKind::COMMENT_KW => {
            util::comment_stmt(p);
        }
        SyntaxKind::CALL_KW => {
            commands::call_stmt(p);
        }
        SyntaxKind::DO_KW => {
            commands::do_stmt(p);
        }
        SyntaxKind::VACUUM_KW => {
            commands::vacuum_stmt(p);
        }
        SyntaxKind::ANALYZE_KW | SyntaxKind::ANALYSE_KW => {
            commands::analyze_stmt(p);
        }
        SyntaxKind::COPY_KW => {
            commands::copy_stmt(p);
        }
        SyntaxKind::GRANT_KW => {
            commands::grant_stmt(p);
        }
        SyntaxKind::REVOKE_KW => {
            commands::revoke_stmt(p);
        }
        SyntaxKind::MERGE_KW => {
            commands::merge_stmt(p);
        }
        _ => {
            if !p.at_end() {
                p.err_recover(
                    format!("expected statement, found {:?}", p.current()),
                    STMT_RECOVERY,
                );
            }
        }
    }
}

/// Handle WITH clause that can precede SELECT, INSERT, UPDATE, or DELETE.
fn with_statement(p: &mut Parser<'_>) {
    let checkpoint = p.checkpoint();
    select::with_clause(p);
    let next = p.current();
    p.rollback(checkpoint);

    match next {
        SyntaxKind::SELECT_KW => select::select_stmt(p),
        SyntaxKind::INSERT_KW => insert::insert_stmt(p),
        SyntaxKind::UPDATE_KW => update::update_stmt(p),
        SyntaxKind::DELETE_KW => delete::delete_stmt(p),
        _ => {
            select::with_clause(p);
            p.err_recover(
                format!(
                    "expected SELECT, INSERT, UPDATE, or DELETE after WITH clause, found {:?}",
                    p.current()
                ),
                STMT_RECOVERY,
            );
        }
    }
}

use crate::token_set::TokenSet;

/// Recovery set for statement boundaries.
pub const STMT_RECOVERY: TokenSet = TokenSet::new(&[
    SyntaxKind::SELECT_KW,
    SyntaxKind::INSERT_KW,
    SyntaxKind::UPDATE_KW,
    SyntaxKind::DELETE_KW,
    SyntaxKind::CREATE_KW,
    SyntaxKind::ALTER_KW,
    SyntaxKind::DROP_KW,
    SyntaxKind::TRUNCATE_KW,
    SyntaxKind::SEMICOLON,
]);

/// Recovery set for clause boundaries in SELECT statements.
pub const CLAUSE_RECOVERY: TokenSet = TokenSet::new(&[
    SyntaxKind::FROM_KW,
    SyntaxKind::WHERE_KW,
    SyntaxKind::GROUP_KW,
    SyntaxKind::HAVING_KW,
    SyntaxKind::ORDER_KW,
    SyntaxKind::LIMIT_KW,
    SyntaxKind::OFFSET_KW,
    SyntaxKind::UNION_KW,
    SyntaxKind::INTERSECT_KW,
    SyntaxKind::EXCEPT_KW,
    SyntaxKind::SEMICOLON,
    SyntaxKind::R_PAREN,
]);

/// Recovery set for expression list boundaries.
pub const EXPR_LIST_RECOVERY: TokenSet = TokenSet::new(&[
    SyntaxKind::COMMA,
    SyntaxKind::R_PAREN,
    SyntaxKind::R_BRACKET,
    SyntaxKind::FROM_KW,
    SyntaxKind::WHERE_KW,
    SyntaxKind::GROUP_KW,
    SyntaxKind::ORDER_KW,
    SyntaxKind::SEMICOLON,
]);

/// Recovery set for parenthesized expressions.
pub const PAREN_RECOVERY: TokenSet = TokenSet::new(&[
    SyntaxKind::R_PAREN,
    SyntaxKind::COMMA,
    SyntaxKind::SEMICOLON,
]);

/// Recovery set for JOIN clauses.
pub const JOIN_RECOVERY: TokenSet = TokenSet::new(&[
    SyntaxKind::JOIN_KW,
    SyntaxKind::LEFT_KW,
    SyntaxKind::RIGHT_KW,
    SyntaxKind::INNER_KW,
    SyntaxKind::FULL_KW,
    SyntaxKind::CROSS_KW,
    SyntaxKind::WHERE_KW,
    SyntaxKind::GROUP_KW,
    SyntaxKind::ORDER_KW,
    SyntaxKind::SEMICOLON,
]);

/// Recovery set for CASE expressions.
pub const CASE_RECOVERY: TokenSet = TokenSet::new(&[
    SyntaxKind::WHEN_KW,
    SyntaxKind::THEN_KW,
    SyntaxKind::ELSE_KW,
    SyntaxKind::END_KW,
    SyntaxKind::R_PAREN,
    SyntaxKind::COMMA,
    SyntaxKind::FROM_KW,
    SyntaxKind::WHERE_KW,
    SyntaxKind::SEMICOLON,
]);

/// Recovery set for subquery boundaries.
pub const SUBQUERY_RECOVERY: TokenSet = TokenSet::new(&[
    SyntaxKind::SELECT_KW,
    SyntaxKind::FROM_KW,
    SyntaxKind::WHERE_KW,
    SyntaxKind::GROUP_KW,
    SyntaxKind::ORDER_KW,
    SyntaxKind::UNION_KW,
    SyntaxKind::R_PAREN,
    SyntaxKind::SEMICOLON,
]);
