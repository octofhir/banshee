//! DDL statement parsers: CREATE TABLE / INDEX, ALTER TABLE, DROP, TRUNCATE.
//!
//! Scope targets what migration linters care about: column definitions, the
//! constraint kinds, `NOT VALID`, `CONCURRENTLY`, `IF [NOT] EXISTS`, and the
//! `ALTER TABLE` action set. Rarely-linted tails (storage params, MATCH/ON
//! DELETE actions, opclasses) are consumed but not deeply structured.
//!
//! Note: some PostgreSQL words used here (VALID, ALWAYS, DATA, STORED,
//! VALIDATE) are not reserved keywords in banshee's lexer, so they arrive as
//! `IDENT`; the grammar consumes them positionally.

use crate::parser::Parser;
use crate::token_set::TokenSet;
use banshee_syntax::SyntaxKind;

use super::PAREN_RECOVERY;
use super::STMT_RECOVERY;
use super::expressions::{expr, type_name};
use super::select::{at_ident, expect_ident, select_stmt};

/// Recovery set for DDL list items (column defs, actions, constraints).
const ITEM_RECOVERY: TokenSet = TokenSet::new(&[
    SyntaxKind::COMMA,
    SyntaxKind::R_PAREN,
    SyntaxKind::SEMICOLON,
]);

/// Statement-boundary recovery for DDL names.
const DDL_RECOVERY: TokenSet = TokenSet::new(&[SyntaxKind::SEMICOLON, SyntaxKind::R_PAREN]);

// ===========================================================================
// CREATE
// ===========================================================================

/// Dispatch a `CREATE …` statement, looking past leading modifiers.
pub fn create_stmt(p: &mut Parser<'_>) {
    let mut n = 1; // 0 is CREATE
    loop {
        match p.nth(n) {
            SyntaxKind::TEMP_KW
            | SyntaxKind::TEMPORARY_KW
            | SyntaxKind::UNLOGGED_KW
            | SyntaxKind::CONSTRAINT_KW
            | SyntaxKind::UNIQUE_KW => n += 1,
            SyntaxKind::OR_KW => n += 2, // OR REPLACE
            _ => break,
        }
    }

    match p.nth(n) {
        SyntaxKind::TABLE_KW => create_table_stmt(p),
        SyntaxKind::INDEX_KW => create_index_stmt(p),
        SyntaxKind::VIEW_KW | SyntaxKind::MATERIALIZED_KW => create_view_stmt(p),
        SyntaxKind::SEQUENCE_KW => create_sequence_stmt(p),
        SyntaxKind::SCHEMA_KW => create_schema_stmt(p),
        SyntaxKind::EXTENSION_KW => create_extension_stmt(p),
        SyntaxKind::TYPE_KW => create_type_stmt(p),
        SyntaxKind::FUNCTION_KW | SyntaxKind::PROCEDURE_KW => create_function_stmt(p),
        SyntaxKind::TRIGGER_KW => create_trigger_stmt(p),
        // DOMAIN is non-reserved (lexed as an identifier), so match on its text.
        _ if p.nth_text(n).eq_ignore_ascii_case("domain") => create_domain_stmt(p),
        other => skip_unsupported(p, format!("unsupported CREATE statement: {other:?}")),
    }
}

fn create_type_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // CREATE
    p.expect(SyntaxKind::TYPE_KW);
    relation_name(p);
    // AS ENUM (…) | AS (…) | AS RANGE (…) | (shell type)
    consume_until_semi(p);
    m.complete(p, SyntaxKind::CREATE_TYPE_STMT);
}

fn create_domain_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // CREATE
    p.bump(); // DOMAIN (identifier)
    relation_name(p);
    // [AS] data_type [COLLATE c] [DEFAULT expr] [constraint …]
    consume_until_semi(p);
    m.complete(p, SyntaxKind::CREATE_DOMAIN_STMT);
}

fn alter_domain_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // ALTER
    p.bump(); // DOMAIN (identifier)
    relation_name(p);
    // ADD CONSTRAINT …, DROP CONSTRAINT …, SET/DROP DEFAULT, OWNER TO, …
    consume_until_semi(p);
    m.complete(p, SyntaxKind::ALTER_DOMAIN_STMT);
}

fn alter_type_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // ALTER
    p.bump(); // TYPE
    relation_name(p);
    // ADD VALUE [IF NOT EXISTS] 'v' [BEFORE|AFTER 'w'], RENAME [VALUE|ATTRIBUTE|TO] …,
    // OWNER TO …, SET SCHEMA …, ADD/DROP/ALTER ATTRIBUTE …
    consume_until_semi(p);
    m.complete(p, SyntaxKind::ALTER_STMT);
}

fn create_function_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // CREATE
    if p.eat(SyntaxKind::OR_KW) {
        p.expect(SyntaxKind::REPLACE_KW);
    }
    let _ = p.eat(SyntaxKind::FUNCTION_KW) || p.eat(SyntaxKind::PROCEDURE_KW);
    relation_name(p);
    if p.at(SyntaxKind::L_PAREN) {
        consume_balanced_parens(p); // argument list
    }
    // RETURNS …, body ($$ … $$ is a single token), LANGUAGE …, options.
    consume_until_semi(p);
    m.complete(p, SyntaxKind::CREATE_FUNCTION_STMT);
}

fn create_trigger_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // CREATE
    if p.eat(SyntaxKind::OR_KW) {
        p.expect(SyntaxKind::REPLACE_KW);
    }
    p.eat(SyntaxKind::CONSTRAINT_KW);
    p.expect(SyntaxKind::TRIGGER_KW);
    expect_ident(p, DDL_RECOVERY);
    // timing/events ON table … EXECUTE { FUNCTION | PROCEDURE } f(…)
    consume_until_semi(p);
    m.complete(p, SyntaxKind::CREATE_TRIGGER_STMT);
}

fn create_view_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // CREATE

    if p.eat(SyntaxKind::OR_KW) {
        p.expect(SyntaxKind::REPLACE_KW);
    }
    let _ = p.eat(SyntaxKind::TEMP_KW)
        || p.eat(SyntaxKind::TEMPORARY_KW)
        || p.eat(SyntaxKind::RECURSIVE_KW);
    p.eat(SyntaxKind::MATERIALIZED_KW);
    p.expect(SyntaxKind::VIEW_KW);

    if p.eat(SyntaxKind::IF_KW) {
        p.expect(SyntaxKind::NOT_KW);
        p.expect(SyntaxKind::EXISTS_KW);
    }

    relation_name(p);

    if p.at(SyntaxKind::L_PAREN) {
        paren_column_list(p);
    }

    // WITH ( view_options ) before AS.
    if p.at(SyntaxKind::WITH_KW) {
        consume_until_as_or_semi(p);
    }

    if p.eat(SyntaxKind::AS_KW) {
        if p.at(SyntaxKind::WITH_KW) || p.at(SyntaxKind::SELECT_KW) {
            select_stmt(p);
        } else {
            p.error("expected SELECT after CREATE VIEW … AS");
        }
    }

    // Trailing WITH [NO] DATA / WITH CHECK OPTION.
    consume_until_semi(p);

    m.complete(p, SyntaxKind::CREATE_VIEW_STMT);
}

fn create_sequence_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // CREATE
    let _ = p.eat(SyntaxKind::TEMP_KW) || p.eat(SyntaxKind::TEMPORARY_KW);
    p.expect(SyntaxKind::SEQUENCE_KW);
    if p.eat(SyntaxKind::IF_KW) {
        p.expect(SyntaxKind::NOT_KW);
        p.expect(SyntaxKind::EXISTS_KW);
    }
    relation_name(p);
    consume_until_semi(p); // AS type / INCREMENT / START / OWNED BY …
    m.complete(p, SyntaxKind::CREATE_SEQUENCE_STMT);
}

fn create_schema_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // CREATE
    p.expect(SyntaxKind::SCHEMA_KW);
    if p.eat(SyntaxKind::IF_KW) {
        p.expect(SyntaxKind::NOT_KW);
        p.expect(SyntaxKind::EXISTS_KW);
    }
    if p.eat(SyntaxKind::AUTHORIZATION_KW) {
        expect_ident(p, DDL_RECOVERY);
    } else {
        relation_name(p);
        if p.eat(SyntaxKind::AUTHORIZATION_KW) {
            expect_ident(p, DDL_RECOVERY);
        }
    }
    // Optional inline schema elements (CREATE … inside).
    consume_until_semi(p);
    m.complete(p, SyntaxKind::CREATE_SCHEMA_STMT);
}

fn create_extension_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // CREATE
    p.expect(SyntaxKind::EXTENSION_KW);
    if p.eat(SyntaxKind::IF_KW) {
        p.expect(SyntaxKind::NOT_KW);
        p.expect(SyntaxKind::EXISTS_KW);
    }
    expect_ident(p, DDL_RECOVERY);
    consume_until_semi(p); // WITH / SCHEMA / VERSION / CASCADE
    m.complete(p, SyntaxKind::CREATE_EXTENSION_STMT);
}

fn create_table_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // CREATE

    let _ = p.eat(SyntaxKind::TEMP_KW)
        || p.eat(SyntaxKind::TEMPORARY_KW)
        || p.eat(SyntaxKind::UNLOGGED_KW);

    p.expect(SyntaxKind::TABLE_KW);

    if p.eat(SyntaxKind::IF_KW) {
        p.expect(SyntaxKind::NOT_KW);
        p.expect(SyntaxKind::EXISTS_KW);
    }

    relation_name(p);

    // CREATE TABLE … AS [SELECT|WITH] …
    if p.at(SyntaxKind::AS_KW) {
        p.bump();
        if p.at(SyntaxKind::WITH_KW) || p.at(SyntaxKind::SELECT_KW) {
            select_stmt(p);
        } else {
            p.error("expected SELECT after CREATE TABLE … AS");
        }
        m.complete(p, SyntaxKind::CREATE_TABLE_STMT);
        return;
    }

    if p.at(SyntaxKind::L_PAREN) {
        column_def_list(p);
    } else if p.eat(SyntaxKind::OF_KW) {
        relation_name(p);
    }

    if p.eat(SyntaxKind::INHERITS_KW) {
        p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
        relation_name(p);
        while p.eat(SyntaxKind::COMMA) {
            relation_name(p);
        }
        p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    }

    // Trailing table options (WITH (...), TABLESPACE, PARTITION BY, …).
    consume_until_semi(p);

    m.complete(p, SyntaxKind::CREATE_TABLE_STMT);
}

fn column_def_list(p: &mut Parser<'_>) {
    let m = p.start();
    p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);

    if !p.at(SyntaxKind::R_PAREN) {
        table_element(p);
        while p.eat(SyntaxKind::COMMA) {
            if p.at(SyntaxKind::R_PAREN) {
                break; // tolerate trailing comma
            }
            table_element(p);
        }
    }

    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    m.complete(p, SyntaxKind::COLUMN_DEF_LIST);
}

fn table_element(p: &mut Parser<'_>) {
    if is_table_constraint_start(p) {
        table_constraint(p);
    } else if p.at(SyntaxKind::LIKE_KW) {
        let m = p.start();
        p.bump(); // LIKE
        relation_name(p);
        consume_item(p); // like_option list
        m.complete(p, SyntaxKind::CONSTRAINT);
    } else {
        column_def(p);
    }
}

fn is_table_constraint_start(p: &Parser<'_>) -> bool {
    matches!(
        p.current(),
        SyntaxKind::CONSTRAINT_KW
            | SyntaxKind::PRIMARY_KW
            | SyntaxKind::UNIQUE_KW
            | SyntaxKind::CHECK_KW
            | SyntaxKind::FOREIGN_KW
            | SyntaxKind::EXCLUDE_KW
    )
}

fn column_def(p: &mut Parser<'_>) {
    let m = p.start();

    expect_ident(p, ITEM_RECOVERY);
    type_name(p, "in column definition");

    if p.eat(SyntaxKind::COLLATE_KW) {
        expect_ident(p, ITEM_RECOVERY);
    }

    while is_column_constraint_start(p) {
        column_constraint(p);
    }

    m.complete(p, SyntaxKind::COLUMN_DEF);
}

fn is_column_constraint_start(p: &Parser<'_>) -> bool {
    matches!(
        p.current(),
        SyntaxKind::CONSTRAINT_KW
            | SyntaxKind::NOT_KW
            | SyntaxKind::NULL_KW
            | SyntaxKind::DEFAULT_KW
            | SyntaxKind::PRIMARY_KW
            | SyntaxKind::UNIQUE_KW
            | SyntaxKind::CHECK_KW
            | SyntaxKind::REFERENCES_KW
            | SyntaxKind::GENERATED_KW
    )
}

fn column_constraint(p: &mut Parser<'_>) {
    let m = p.start();

    if p.eat(SyntaxKind::CONSTRAINT_KW) {
        expect_ident(p, ITEM_RECOVERY);
    }

    let kind = match p.current() {
        SyntaxKind::NOT_KW => {
            p.bump();
            p.expect(SyntaxKind::NULL_KW);
            SyntaxKind::NOT_NULL_CONSTRAINT
        }
        SyntaxKind::NULL_KW => {
            p.bump();
            SyntaxKind::CONSTRAINT
        }
        SyntaxKind::DEFAULT_KW => {
            p.bump();
            expr(p);
            SyntaxKind::DEFAULT_EXPR
        }
        SyntaxKind::PRIMARY_KW => {
            p.bump();
            p.expect(SyntaxKind::KEY_KW);
            SyntaxKind::PRIMARY_KEY_CONSTRAINT
        }
        SyntaxKind::UNIQUE_KW => {
            p.bump();
            eat_nulls_distinct(p);
            SyntaxKind::UNIQUE_CONSTRAINT
        }
        SyntaxKind::CHECK_KW => {
            p.bump();
            p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
            expr(p);
            p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
            SyntaxKind::CHECK_CONSTRAINT
        }
        SyntaxKind::REFERENCES_KW => {
            references_clause(p);
            SyntaxKind::FOREIGN_KEY_CONSTRAINT
        }
        SyntaxKind::GENERATED_KW => {
            generated_clause(p);
            SyntaxKind::CONSTRAINT
        }
        _ => {
            p.err_recover("expected column constraint", ITEM_RECOVERY);
            SyntaxKind::CONSTRAINT
        }
    };

    m.complete(p, kind);
}

fn table_constraint(p: &mut Parser<'_>) {
    let m = p.start();

    if p.eat(SyntaxKind::CONSTRAINT_KW) {
        expect_ident(p, ITEM_RECOVERY);
    }

    let kind = match p.current() {
        SyntaxKind::PRIMARY_KW => {
            p.bump();
            p.expect(SyntaxKind::KEY_KW);
            paren_column_list(p);
            SyntaxKind::PRIMARY_KEY_CONSTRAINT
        }
        SyntaxKind::UNIQUE_KW => {
            p.bump();
            eat_nulls_distinct(p);
            paren_column_list(p);
            SyntaxKind::UNIQUE_CONSTRAINT
        }
        SyntaxKind::CHECK_KW => {
            p.bump();
            p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
            expr(p);
            p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
            SyntaxKind::CHECK_CONSTRAINT
        }
        SyntaxKind::FOREIGN_KW => {
            p.bump();
            p.expect(SyntaxKind::KEY_KW);
            paren_column_list(p);
            references_clause(p);
            SyntaxKind::FOREIGN_KEY_CONSTRAINT
        }
        SyntaxKind::EXCLUDE_KW => {
            p.bump();
            consume_item(p);
            SyntaxKind::CONSTRAINT
        }
        _ => {
            p.err_recover("expected table constraint", ITEM_RECOVERY);
            SyntaxKind::CONSTRAINT
        }
    };

    consume_not_valid(p);

    m.complete(p, kind);
}

/// Optional `NULLS [NOT] DISTINCT` modifier on a UNIQUE constraint/index.
fn eat_nulls_distinct(p: &mut Parser<'_>) {
    if p.eat(SyntaxKind::NULLS_KW) {
        p.eat(SyntaxKind::NOT_KW);
        p.expect(SyntaxKind::DISTINCT_KW);
    }
}

fn references_clause(p: &mut Parser<'_>) {
    p.bump(); // REFERENCES
    relation_name(p);
    if p.at(SyntaxKind::L_PAREN) {
        paren_column_list(p);
    }
    loop {
        match p.current() {
            SyntaxKind::MATCH_KW => {
                p.bump();
                p.bump_any(); // FULL / PARTIAL / SIMPLE
            }
            SyntaxKind::ON_KW => {
                p.bump(); // ON
                p.bump_any(); // DELETE / UPDATE
                referential_action(p);
            }
            _ => break,
        }
    }
}

fn referential_action(p: &mut Parser<'_>) {
    match p.current() {
        SyntaxKind::CASCADE_KW | SyntaxKind::RESTRICT_KW => {
            p.bump();
        }
        SyntaxKind::NO_KW => {
            p.bump();
            p.expect(SyntaxKind::ACTION_KW);
        }
        SyntaxKind::SET_KW => {
            p.bump();
            let _ = p.eat(SyntaxKind::NULL_KW) || p.eat(SyntaxKind::DEFAULT_KW);
            if p.at(SyntaxKind::L_PAREN) {
                paren_column_list(p);
            }
        }
        _ => {}
    }
}

fn generated_clause(p: &mut Parser<'_>) {
    p.bump(); // GENERATED
    // GENERATED ALWAYS AS ( expr ) STORED | GENERATED { ALWAYS | BY DEFAULT } AS IDENTITY
    if p.eat(SyntaxKind::BY_KW) {
        p.expect(SyntaxKind::DEFAULT_KW);
    } else if at_ident(p) {
        p.bump(); // ALWAYS (not a reserved keyword)
    }
    p.expect(SyntaxKind::AS_KW);
    if p.eat(SyntaxKind::IDENTITY_KW) {
        if p.at(SyntaxKind::L_PAREN) {
            consume_balanced_parens(p);
        }
    } else if p.at(SyntaxKind::L_PAREN) {
        p.bump();
        expr(p);
        p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
        consume_trailing_word(p); // STORED
    }
}

// ===========================================================================
// CREATE INDEX
// ===========================================================================

fn create_index_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // CREATE
    p.eat(SyntaxKind::UNIQUE_KW);
    p.expect(SyntaxKind::INDEX_KW);
    p.eat(SyntaxKind::CONCURRENTLY_KW);

    if p.eat(SyntaxKind::IF_KW) {
        p.expect(SyntaxKind::NOT_KW);
        p.expect(SyntaxKind::EXISTS_KW);
        expect_ident(p, DDL_RECOVERY);
    } else if at_ident(p) {
        p.bump(); // optional index name
    }

    p.expect(SyntaxKind::ON_KW);
    p.eat(SyntaxKind::ONLY_KW);
    relation_name(p);

    if p.eat(SyntaxKind::USING_KW) {
        expect_ident(p, PAREN_RECOVERY);
    }

    p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
    index_elem(p);
    while p.eat(SyntaxKind::COMMA) {
        index_elem(p);
    }
    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);

    // INCLUDE (cols), WITH (...), TABLESPACE … then optional WHERE predicate.
    consume_until_where_or_semi(p);
    if p.eat(SyntaxKind::WHERE_KW) {
        let wm = p.start();
        expr(p);
        wm.complete(p, SyntaxKind::WHERE_CLAUSE);
    }
    consume_until_semi(p);

    m.complete(p, SyntaxKind::CREATE_INDEX_STMT);
}

fn index_elem(p: &mut Parser<'_>) {
    if p.at(SyntaxKind::L_PAREN) {
        p.bump();
        expr(p);
        p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    } else {
        expr(p);
    }
    if at_ident(p) {
        p.bump(); // opclass
    }
    let _ = p.eat(SyntaxKind::ASC_KW) || p.eat(SyntaxKind::DESC_KW);
    if p.eat(SyntaxKind::NULLS_KW) {
        let _ = p.eat(SyntaxKind::FIRST_KW) || p.eat(SyntaxKind::LAST_KW);
    }
}

// ===========================================================================
// ALTER TABLE
// ===========================================================================

pub fn alter_stmt(p: &mut Parser<'_>) {
    if p.nth(1) != SyntaxKind::TABLE_KW {
        // DOMAIN is non-reserved (lexed as an identifier), so match on its text.
        if p.nth_text(1).eq_ignore_ascii_case("domain") {
            alter_domain_stmt(p);
        } else if p.nth(1) == SyntaxKind::TYPE_KW {
            alter_type_stmt(p);
        } else {
            skip_unsupported(p, format!("unsupported ALTER statement: {:?}", p.nth(1)));
        }
        return;
    }

    let m = p.start();
    p.bump(); // ALTER
    p.bump(); // TABLE

    if p.eat(SyntaxKind::IF_KW) {
        p.expect(SyntaxKind::EXISTS_KW);
    }
    p.eat(SyntaxKind::ONLY_KW);
    relation_name(p);

    if p.at(SyntaxKind::RENAME_KW) {
        alter_rename(p);
        m.complete(p, SyntaxKind::ALTER_STMT);
        return;
    }

    alter_action(p);
    while p.eat(SyntaxKind::COMMA) {
        alter_action(p);
    }

    m.complete(p, SyntaxKind::ALTER_STMT);
}

fn alter_rename(p: &mut Parser<'_>) {
    p.bump(); // RENAME
    if p.eat(SyntaxKind::TO_KW) {
        expect_ident(p, STMT_RECOVERY); // RENAME TO new_table
    } else {
        let _ = p.eat(SyntaxKind::COLUMN_KW) || p.eat(SyntaxKind::CONSTRAINT_KW);
        expect_ident(p, STMT_RECOVERY);
        p.expect(SyntaxKind::TO_KW);
        expect_ident(p, STMT_RECOVERY);
    }
}

fn alter_action(p: &mut Parser<'_>) {
    let m = p.start();

    match p.current() {
        SyntaxKind::ADD_KW => {
            p.bump();
            if is_table_constraint_start(p) {
                table_constraint(p);
            } else {
                p.eat(SyntaxKind::COLUMN_KW);
                if p.eat(SyntaxKind::IF_KW) {
                    p.expect(SyntaxKind::NOT_KW);
                    p.expect(SyntaxKind::EXISTS_KW);
                }
                column_def(p);
            }
        }
        SyntaxKind::DROP_KW => {
            p.bump();
            let _ = p.eat(SyntaxKind::CONSTRAINT_KW) || p.eat(SyntaxKind::COLUMN_KW);
            if p.eat(SyntaxKind::IF_KW) {
                p.expect(SyntaxKind::EXISTS_KW);
            }
            expect_ident(p, ITEM_RECOVERY);
            let _ = p.eat(SyntaxKind::CASCADE_KW) || p.eat(SyntaxKind::RESTRICT_KW);
        }
        SyntaxKind::ALTER_KW => {
            p.bump();
            p.eat(SyntaxKind::COLUMN_KW);
            expect_ident(p, ITEM_RECOVERY);
            alter_column_action(p);
        }
        _ => {
            // VALIDATE CONSTRAINT, OWNER TO, SET …, ENABLE/DISABLE … — opaque.
            consume_item(p);
        }
    }

    m.complete(p, SyntaxKind::ALTER_TABLE_ACTION);
}

fn alter_column_action(p: &mut Parser<'_>) {
    match p.current() {
        SyntaxKind::SET_KW => {
            p.bump();
            match p.current() {
                SyntaxKind::NOT_KW => {
                    p.bump();
                    p.expect(SyntaxKind::NULL_KW);
                }
                SyntaxKind::DEFAULT_KW => {
                    p.bump();
                    expr(p);
                }
                _ => consume_item(p), // SET DATA TYPE …, SET STATISTICS, SET (...)
            }
        }
        SyntaxKind::DROP_KW => {
            p.bump();
            if p.eat(SyntaxKind::DEFAULT_KW) {
                // DROP DEFAULT
            } else if p.eat(SyntaxKind::NOT_KW) {
                p.expect(SyntaxKind::NULL_KW);
            } else {
                consume_item(p);
            }
        }
        SyntaxKind::TYPE_KW => {
            p.bump();
            type_name(p, "in ALTER COLUMN TYPE");
            if p.eat(SyntaxKind::USING_KW) {
                expr(p);
            }
        }
        _ => consume_item(p),
    }
}

// ===========================================================================
// DROP / TRUNCATE
// ===========================================================================

pub fn drop_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // DROP

    p.eat(SyntaxKind::CONCURRENTLY_KW);
    p.bump_any(); // object-type keyword: TABLE / INDEX / VIEW / SEQUENCE / TYPE / …
    p.eat(SyntaxKind::CONCURRENTLY_KW);

    if p.eat(SyntaxKind::IF_KW) {
        p.expect(SyntaxKind::EXISTS_KW);
    }

    relation_name(p);
    while p.eat(SyntaxKind::COMMA) {
        relation_name(p);
    }

    let _ = p.eat(SyntaxKind::CASCADE_KW) || p.eat(SyntaxKind::RESTRICT_KW);

    m.complete(p, SyntaxKind::DROP_STMT);
}

pub fn truncate_stmt(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // TRUNCATE
    p.eat(SyntaxKind::TABLE_KW);
    p.eat(SyntaxKind::ONLY_KW);

    relation_name(p);
    while p.eat(SyntaxKind::COMMA) {
        p.eat(SyntaxKind::ONLY_KW);
        relation_name(p);
    }

    if p.eat(SyntaxKind::RESTART_KW) {
        p.expect(SyntaxKind::IDENTITY_KW);
    }
    let _ = p.eat(SyntaxKind::CASCADE_KW) || p.eat(SyntaxKind::RESTRICT_KW);

    m.complete(p, SyntaxKind::TRUNCATE_STMT);
}

// ===========================================================================
// Helpers
// ===========================================================================

/// Emit an error and consume an unsupported statement up to its boundary,
/// always making forward progress so the top-level loop cannot stall.
fn skip_unsupported(p: &mut Parser<'_>, msg: String) {
    let m = p.start();
    p.error(msg);
    if !p.at(SyntaxKind::SEMICOLON) && !p.at_end() {
        p.bump_any(); // guarantee progress past the leading keyword
    }
    while !p.at_end() && !p.at(SyntaxKind::SEMICOLON) {
        p.bump_any();
    }
    m.complete(p, SyntaxKind::ERROR);
}

/// Schema-qualified relation name → TABLE_REF.
fn relation_name(p: &mut Parser<'_>) {
    let m = p.start();
    expect_ident(p, DDL_RECOVERY);
    if p.eat(SyntaxKind::DOT) {
        expect_ident(p, DDL_RECOVERY);
    }
    m.complete(p, SyntaxKind::TABLE_REF);
}

fn paren_column_list(p: &mut Parser<'_>) {
    p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
    expect_ident(p, PAREN_RECOVERY);
    while p.eat(SyntaxKind::COMMA) {
        expect_ident(p, PAREN_RECOVERY);
    }
    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
}

/// `NOT VALID` tail. VALID is not a reserved keyword, so it lexes as IDENT.
fn consume_not_valid(p: &mut Parser<'_>) {
    if p.at(SyntaxKind::NOT_KW) {
        p.bump();
        p.bump_any(); // VALID
    }
}

/// Consume one trailing bare word (e.g. STORED) when present.
fn consume_trailing_word(p: &mut Parser<'_>) {
    if !ITEM_RECOVERY.contains(p.current()) && (at_ident(p) || p.current().is_keyword()) {
        p.bump();
    }
}

/// Consume tokens up to the next top-level separator / closing paren / semicolon,
/// honoring nested parentheses. These are valid-but-unstructured DDL tails, so
/// the tokens are attached loosely (no ERROR node, no parse error).
fn consume_item(p: &mut Parser<'_>) {
    let mut depth = 0i32;
    while !p.at_end() {
        match p.current() {
            SyntaxKind::L_PAREN => depth += 1,
            SyntaxKind::R_PAREN if depth == 0 => break,
            SyntaxKind::R_PAREN => depth -= 1,
            SyntaxKind::COMMA | SyntaxKind::SEMICOLON if depth == 0 => break,
            _ => {}
        }
        p.bump_any();
    }
}

fn consume_balanced_parens(p: &mut Parser<'_>) {
    if !p.at(SyntaxKind::L_PAREN) {
        return;
    }
    let mut depth = 0i32;
    while !p.at_end() {
        match p.current() {
            SyntaxKind::L_PAREN => depth += 1,
            SyntaxKind::R_PAREN => {
                depth -= 1;
                p.bump();
                if depth == 0 {
                    break;
                }
                continue;
            }
            _ => {}
        }
        p.bump_any();
    }
}

fn consume_until_semi(p: &mut Parser<'_>) {
    while !p.at_end() && !p.at(SyntaxKind::SEMICOLON) {
        p.bump_any();
    }
}

fn consume_until_as_or_semi(p: &mut Parser<'_>) {
    let mut depth = 0i32;
    while !p.at_end() {
        match p.current() {
            SyntaxKind::L_PAREN => depth += 1,
            SyntaxKind::R_PAREN if depth > 0 => depth -= 1,
            SyntaxKind::AS_KW | SyntaxKind::SEMICOLON if depth == 0 => break,
            _ => {}
        }
        p.bump_any();
    }
}

fn consume_until_where_or_semi(p: &mut Parser<'_>) {
    let mut depth = 0i32;
    while !p.at_end() {
        match p.current() {
            SyntaxKind::L_PAREN => depth += 1,
            SyntaxKind::R_PAREN if depth > 0 => depth -= 1,
            SyntaxKind::WHERE_KW | SyntaxKind::SEMICOLON if depth == 0 => break,
            _ => {}
        }
        p.bump_any();
    }
}
