//! Migration-safety rules (Migration pack): squawk-class checks for DDL that
//! locks tables, destroys data, or picks a type that will need migrating later.
//!
//! Codes: `MG01` concurrent index · `MG02` constraint `NOT VALID` ·
//! `MG03` volatile column default · `MG04` `NOT NULL` without default ·
//! `MG05` drop column · `MG06` alter column type · `MG07` rename ·
//! `MG08` `TRUNCATE … CASCADE` · `MG09` prefer `text` · `MG10` prefer
//! `timestamptz` · `MG11` prefer `bigint` primary key · `MG12` concurrent
//! index drop · `MG13` add PK/UNIQUE under lock · `MG14` `SET NOT NULL` ·
//! `MG15` prefer identity over `serial` · `MG16` drop table ·
//! `MG17` drop `NOT NULL` · `MG18` drop database · `MG19` concurrent index
//! build inside a transaction · `MG20` uncommitted transaction · `MG21`
//! transaction nesting · `MG22` non-idempotent (`IF [NOT] EXISTS`) statement ·
//! `MG23` unqualified table name · `MG24` over-long identifier.

use banshee_syntax::SyntaxKind;
use banshee_syntax::SyntaxToken;
use banshee_syntax::ast::{
    AlterActionKind, AlterStmt, AlterTableAction, AstNode, ColumnDef, Constraint, CreateIndexStmt,
    CreateTableStmt, DropStmt, FuncCall, TruncateStmt, TypeName,
};
use text_size::TextRange;

use super::Rule;
use crate::analyze::{Analyzer, BuiltinLintPack, Diagnostic, Fix, RuleCode, TextEdit};

/// End offset of the first direct child token of `kind`, for inserting text.
fn token_end(node: &banshee_syntax::SyntaxNode, kind: SyntaxKind) -> Option<text_size::TextSize> {
    node.children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == kind)
        .map(|t| t.text_range().end())
}

/// End offset of the last non-trivia token anywhere under `node`.
fn last_token_end(node: &banshee_syntax::SyntaxNode) -> Option<text_size::TextSize> {
    node.descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| !t.kind().is_trivia())
        .map(|t| t.text_range().end())
        .max()
}

/// The base type word(s) of a column type, lowercased and without any modifier
/// (`varchar(255)` → `"varchar"`, `double precision` → `"double precision"`).
fn base_type(ty: &TypeName) -> String {
    let text = ty.text().to_ascii_lowercase();
    text.split(['(', '['])
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

// ===========================================================================
// MG01 / MG12 — concurrent index operations
// ===========================================================================

/// MG01 — `CREATE INDEX` without `CONCURRENTLY` takes an exclusive lock that
/// blocks writes for the whole build. `CONCURRENTLY` cannot run inside an
/// explicit transaction block.
pub(super) struct IndexConcurrently;

impl Rule for IndexConcurrently {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg01]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            let Some(stmt) = CreateIndexStmt::cast(node.clone()) else {
                continue;
            };
            // CONCURRENTLY cannot run inside a transaction block, so do not ask
            // for it when the statement is wrapped in one.
            if stmt.is_concurrent() || in_open_transaction(stmt.syntax(), root) {
                continue;
            }
            let mut diag = Diagnostic::warning(
                "CREATE INDEX without CONCURRENTLY locks the table against writes for the \
                 duration of the build",
            )
            .with_code(RuleCode::Mg01)
            .with_range(stmt.syntax().text_range());
            if let Some(at) = token_end(stmt.syntax(), SyntaxKind::INDEX_KW) {
                diag = diag.with_fix(Fix::new(
                    "Add CONCURRENTLY",
                    vec![TextEdit::replace(TextRange::empty(at), " CONCURRENTLY")],
                ));
            }
            analyzer.emit(diag);
        }
    }
}

/// MG12 — `DROP INDEX` without `CONCURRENTLY` takes an exclusive lock on the
/// table while the index is removed.
pub(super) struct DropIndexConcurrently;

impl Rule for DropIndexConcurrently {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg12]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            let Some(stmt) = DropStmt::cast(node.clone()) else {
                continue;
            };
            if stmt.object_kind() != Some(SyntaxKind::INDEX_KW)
                || stmt.is_concurrent()
                || in_open_transaction(stmt.syntax(), root)
            {
                continue;
            }
            let mut diag = Diagnostic::warning("DROP INDEX without CONCURRENTLY locks the table")
                .with_code(RuleCode::Mg12)
                .with_range(stmt.syntax().text_range());
            if let Some(at) = token_end(stmt.syntax(), SyntaxKind::INDEX_KW) {
                diag = diag.with_fix(Fix::new(
                    "Add CONCURRENTLY",
                    vec![TextEdit::replace(TextRange::empty(at), " CONCURRENTLY")],
                ));
            }
            analyzer.emit(diag);
        }
    }
}

// ===========================================================================
// MG02 — ADD CONSTRAINT without NOT VALID
// ===========================================================================

/// MG02 — adding a `FOREIGN KEY` or `CHECK` constraint without `NOT VALID`
/// scans and validates every existing row while holding a lock. Add the
/// constraint `NOT VALID`, then `VALIDATE CONSTRAINT` in a separate step.
pub(super) struct ConstraintNotValid;

impl Rule for ConstraintNotValid {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg02]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            let Some(action) = AlterTableAction::cast(node.clone()) else {
                continue;
            };
            if action.kind() != AlterActionKind::AddConstraint {
                continue;
            }
            let Some(constraint) = action.added_constraint() else {
                continue;
            };
            let validatable =
                matches!(constraint, Constraint::ForeignKey(_) | Constraint::Check(_));
            if !validatable || constraint.is_not_valid() {
                continue;
            }
            let mut diag = Diagnostic::warning(
                "ADD CONSTRAINT without NOT VALID validates every existing row under a lock; \
                 add it NOT VALID and VALIDATE CONSTRAINT separately",
            )
            .with_code(RuleCode::Mg02)
            .with_range(action.syntax().text_range());
            if let Some(at) = last_token_end(constraint.syntax()) {
                diag = diag.with_fix(Fix::new(
                    "Add NOT VALID",
                    vec![TextEdit::replace(TextRange::empty(at), " NOT VALID")],
                ));
            }
            analyzer.emit(diag);
        }
    }
}

// ===========================================================================
// MG03 / MG04 — ADD COLUMN hazards
// ===========================================================================

const VOLATILE_DEFAULT_FUNCS: &[&str] = &[
    "now",
    "clock_timestamp",
    "statement_timestamp",
    "transaction_timestamp",
    "timeofday",
    "random",
    "gen_random_uuid",
    "uuid_generate_v1",
    "uuid_generate_v1mc",
    "uuid_generate_v4",
    "nextval",
];

/// MG03 — `ADD COLUMN … DEFAULT <volatile>` evaluates the default for every
/// existing row, rewriting the whole table under an exclusive lock.
pub(super) struct AddColumnVolatileDefault;

impl Rule for AddColumnVolatileDefault {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg03]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for action in added_columns(root) {
            let Some(col) = action.added_column() else {
                continue;
            };
            let Some(default) = col.default_expr() else {
                continue;
            };
            let has_volatile = default.syntax().descendants().any(|n| {
                FuncCall::cast(n.clone())
                    .and_then(|f| f.name())
                    .map(|name| {
                        VOLATILE_DEFAULT_FUNCS
                            .iter()
                            .any(|v| name.text().eq_ignore_ascii_case(v))
                    })
                    .unwrap_or(false)
            });
            if has_volatile {
                analyzer.emit(
                    Diagnostic::warning(
                        "ADD COLUMN with a volatile DEFAULT rewrites the whole table; add the \
                         column without a default, then backfill",
                    )
                    .with_code(RuleCode::Mg03)
                    .with_range(action.syntax().text_range()),
                );
            }
        }
    }
}

/// MG04 — `ADD COLUMN … NOT NULL` without a `DEFAULT` fails immediately on a
/// table that already has rows.
pub(super) struct AddColumnNotNullNoDefault;

impl Rule for AddColumnNotNullNoDefault {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg04]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for action in added_columns(root) {
            let Some(col) = action.added_column() else {
                continue;
            };
            if col.is_not_null() && col.default_expr().is_none() {
                analyzer.emit(
                    Diagnostic::warning(
                        "ADD COLUMN NOT NULL without a DEFAULT fails on a table that already \
                         has rows",
                    )
                    .with_code(RuleCode::Mg04)
                    .with_range(action.syntax().text_range()),
                );
            }
        }
    }
}

/// `ALTER TABLE` actions that add a column.
fn added_columns(root: &banshee_syntax::SyntaxNode) -> impl Iterator<Item = AlterTableAction> + '_ {
    root.descendants().filter_map(|n| {
        let action = AlterTableAction::cast(n.clone())?;
        (action.kind() == AlterActionKind::AddColumn).then_some(action)
    })
}

// ===========================================================================
// MG05 / MG06 / MG07 — destructive ALTER TABLE actions
// ===========================================================================

/// MG05 — `DROP COLUMN` permanently destroys the column's data and breaks any
/// view, index, or application code that depends on it.
pub(super) struct DropColumn;

impl Rule for DropColumn {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg05]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            let Some(action) = AlterTableAction::cast(node.clone()) else {
                continue;
            };
            if action.kind() == AlterActionKind::DropColumn {
                analyzer.emit(
                    Diagnostic::warning(
                        "DROP COLUMN destroys the column's data and breaks dependent objects",
                    )
                    .with_code(RuleCode::Mg05)
                    .with_range(action.syntax().text_range()),
                );
            }
        }
    }
}

/// MG06 — `ALTER COLUMN … TYPE` rewrites the table under an exclusive lock and
/// can fail or lose data on an incompatible conversion.
pub(super) struct AlterColumnType;

impl Rule for AlterColumnType {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg06]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            let Some(action) = AlterTableAction::cast(node.clone()) else {
                continue;
            };
            if action.changes_type() {
                analyzer.emit(
                    Diagnostic::warning(
                        "ALTER COLUMN TYPE rewrites the table under an exclusive lock",
                    )
                    .with_code(RuleCode::Mg06)
                    .with_range(action.syntax().text_range()),
                );
            }
        }
    }
}

/// MG07 — renaming a table or column breaks every query, view, and client that
/// still refers to the old name.
pub(super) struct Rename;

impl Rule for Rename {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg07]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            let Some(alter) = AlterStmt::cast(node.clone()) else {
                continue;
            };
            if alter.is_rename() {
                let what = if alter.renames_subobject() {
                    "a column or constraint"
                } else {
                    "a table"
                };
                analyzer.emit(
                    Diagnostic::warning(format!(
                        "RENAME of {what} breaks code that still refers to the old name"
                    ))
                    .with_code(RuleCode::Mg07)
                    .with_range(alter.syntax().text_range()),
                );
            }
        }
    }
}

// ===========================================================================
// MG08 — TRUNCATE ... CASCADE
// ===========================================================================

/// MG08 — `TRUNCATE … CASCADE` empties not just the named tables but every
/// table with a foreign key into them, often far more than intended.
pub(super) struct TruncateCascade;

impl Rule for TruncateCascade {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg08]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            let Some(trunc) = TruncateStmt::cast(node.clone()) else {
                continue;
            };
            if trunc.is_cascade() {
                analyzer.emit(
                    Diagnostic::warning(
                        "TRUNCATE ... CASCADE also empties every table with a foreign key into \
                         the targets",
                    )
                    .with_code(RuleCode::Mg08)
                    .with_range(trunc.syntax().text_range()),
                );
            }
        }
    }
}

// ===========================================================================
// MG09 / MG10 / MG11 — type preferences
// ===========================================================================

/// MG09/MG10/MG11 — prefer durable column types: `text` over `char(n)` and
/// `varchar(n)`, `timestamptz` over `timestamp`, and `bigint` over a narrower
/// integer for a primary key.
pub(super) struct TypePreference;

impl Rule for TypePreference {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg09, RuleCode::Mg10, RuleCode::Mg11]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            if let Some(col) = ColumnDef::cast(node.clone()) {
                if let Some(ty) = col.type_name() {
                    check_char_varchar(&ty, analyzer);
                    check_timestamp(&ty, analyzer);
                }
                continue;
            }
            // `ALTER COLUMN … TYPE <type>` carries a bare type, not a ColumnDef.
            if let Some(action) = AlterTableAction::cast(node.clone())
                && action.changes_type()
                && let Some(ty) = action.new_type()
            {
                check_char_varchar(&ty, analyzer);
                check_timestamp(&ty, analyzer);
            }
        }

        // MG11 — primary-key columns typed with a narrow integer.
        for node in root.descendants() {
            let Some(table) = CreateTableStmt::cast(node.clone()) else {
                continue;
            };
            let pk_columns = primary_key_columns(&table);
            for col in table.columns() {
                let is_pk = col.is_primary_key()
                    || col
                        .name()
                        .map(|n| pk_columns.iter().any(|p| p.eq_ignore_ascii_case(n.text())))
                        .unwrap_or(false);
                if !is_pk {
                    continue;
                }
                // `serial`/`smallserial` are handled by MG15 (prefer identity).
                if let Some(ty) = col.type_name()
                    && matches!(
                        base_type(&ty).as_str(),
                        "int" | "integer" | "int4" | "smallint" | "int2"
                    )
                {
                    analyzer.emit(
                        Diagnostic::warning(
                            "prefer bigint for a primary key; a narrower integer can run out of \
                             values and is costly to widen later",
                        )
                        .with_code(RuleCode::Mg11)
                        .with_range(ty.syntax().text_range()),
                    );
                }
            }
        }
    }
}

/// MG09 — flag `char(n)`/`character(n)`/`varchar(n)`/`character varying`.
fn check_char_varchar(ty: &TypeName, analyzer: &mut Analyzer<'_>) {
    let base = base_type(ty);
    let flagged = matches!(
        base.as_str(),
        "char" | "character" | "varchar" | "character varying"
    );
    if !flagged {
        return;
    }
    analyzer.emit(
        Diagnostic::warning(
            "prefer text to char(n)/varchar(n); the length limit gives no storage benefit and \
             changing it later rewrites the table",
        )
        .with_code(RuleCode::Mg09)
        .with_range(ty.syntax().text_range())
        .with_fix(Fix::new(
            "Use text",
            vec![TextEdit::replace(ty.syntax().text_range(), "text")],
        )),
    );
}

/// MG10 — flag plain `timestamp` (no time-zone qualifier).
fn check_timestamp(ty: &TypeName, analyzer: &mut Analyzer<'_>) {
    if base_type(ty) != "timestamp" {
        return;
    }
    let text = ty.text().to_ascii_lowercase();
    if text.contains("time zone") {
        return; // `timestamp with/without time zone` written explicitly
    }
    let mut diag = Diagnostic::warning(
        "prefer timestamptz to timestamp; timestamp without time zone ignores the session \
         time zone and is a frequent source of bugs",
    )
    .with_code(RuleCode::Mg10)
    .with_range(ty.syntax().text_range());
    // Replace only the leading `timestamp` keyword so any precision is kept.
    if let Some(kw) = timestamp_keyword(ty) {
        diag = diag.with_fix(Fix::new(
            "Use timestamptz",
            vec![TextEdit::replace(kw.text_range(), "timestamptz")],
        ));
    }
    analyzer.emit(diag);
}

fn timestamp_keyword(ty: &TypeName) -> Option<SyntaxToken> {
    ty.syntax()
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == SyntaxKind::TIMESTAMP_KW)
        .cloned()
}

/// Column names named by a single-column table-level `PRIMARY KEY`.
fn primary_key_columns(table: &CreateTableStmt) -> Vec<String> {
    let mut names = Vec::new();
    for constraint in table.constraints() {
        if let Constraint::PrimaryKey(pk) = constraint {
            for ident in pk
                .syntax()
                .descendants_with_tokens()
                .filter_map(|e| e.into_token())
                .filter(|t| matches!(t.kind(), SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT))
            {
                names.push(ident.text().to_string());
            }
        }
    }
    names
}

// ===========================================================================
// MG13 / MG14 — locking ALTER TABLE constraint operations
// ===========================================================================

/// MG13 — `ADD PRIMARY KEY`/`ADD UNIQUE` builds the backing index while holding
/// an exclusive lock. Build a unique index `CONCURRENTLY`, then attach it with
/// `ADD CONSTRAINT … USING INDEX`.
pub(super) struct AddPkUniqueConstraint;

impl Rule for AddPkUniqueConstraint {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg13]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            let Some(action) = AlterTableAction::cast(node.clone()) else {
                continue;
            };
            if action.kind() != AlterActionKind::AddConstraint {
                continue;
            }
            let Some(constraint) = action.added_constraint() else {
                continue;
            };
            if !matches!(
                constraint,
                Constraint::PrimaryKey(_) | Constraint::Unique(_)
            ) {
                continue;
            }
            // `ADD CONSTRAINT … PRIMARY KEY USING INDEX idx` is the safe form.
            if action
                .syntax()
                .descendants_with_tokens()
                .filter_map(|e| e.into_token())
                .any(|t| t.kind() == SyntaxKind::USING_KW)
            {
                continue;
            }
            analyzer.emit(
                Diagnostic::warning(
                    "ADD PRIMARY KEY/UNIQUE builds its index under an exclusive lock; build a \
                     unique index CONCURRENTLY and attach it with ADD CONSTRAINT ... USING INDEX",
                )
                .with_code(RuleCode::Mg13)
                .with_range(action.syntax().text_range()),
            );
        }
    }
}

/// MG14 — `ALTER COLUMN … SET NOT NULL` scans every existing row to verify the
/// constraint while holding a lock. Add a `CHECK (col IS NOT NULL) NOT VALID`,
/// validate it, then set `NOT NULL` (which can then reuse the validated check).
pub(super) struct SetNotNull;

impl Rule for SetNotNull {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg14]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            let Some(action) = AlterTableAction::cast(node.clone()) else {
                continue;
            };
            if action.kind() != AlterActionKind::AlterColumn {
                continue;
            }
            let tokens: Vec<SyntaxKind> = action
                .syntax()
                .descendants_with_tokens()
                .filter_map(|e| e.into_token())
                .map(|t| t.kind())
                .collect();
            let sets_not_null = tokens.contains(&SyntaxKind::SET_KW)
                && tokens.contains(&SyntaxKind::NOT_KW)
                && tokens.contains(&SyntaxKind::NULL_KW);
            if sets_not_null {
                analyzer.emit(
                    Diagnostic::warning(
                        "ALTER COLUMN SET NOT NULL scans the whole table under a lock; add a \
                         CHECK (col IS NOT NULL) NOT VALID, VALIDATE it, then SET NOT NULL",
                    )
                    .with_code(RuleCode::Mg14)
                    .with_range(action.syntax().text_range()),
                );
            }
        }
    }
}

// ===========================================================================
// MG15 — prefer GENERATED IDENTITY over serial
// ===========================================================================

/// MG15 — `serial`/`bigserial` are legacy pseudo-types that create a detached
/// sequence with awkward ownership and permissions. Prefer a standard
/// `GENERATED { ALWAYS | BY DEFAULT } AS IDENTITY` column.
pub(super) struct PreferIdentity;

impl Rule for PreferIdentity {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg15]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            let Some(col) = ColumnDef::cast(node.clone()) else {
                continue;
            };
            let Some(ty) = col.type_name() else { continue };
            if matches!(
                base_type(&ty).as_str(),
                "serial" | "serial4" | "bigserial" | "serial8" | "smallserial" | "serial2"
            ) {
                analyzer.emit(
                    Diagnostic::warning(
                        "prefer GENERATED ... AS IDENTITY to serial; serial leaves a detached \
                         sequence with awkward ownership and grants",
                    )
                    .with_code(RuleCode::Mg15)
                    .with_range(ty.syntax().text_range()),
                );
            }
        }
    }
}

// ===========================================================================
// MG16 — DROP TABLE
// ===========================================================================

/// MG16 — `DROP TABLE` permanently destroys the table and cascades to views,
/// foreign keys, and policies that depend on it.
pub(super) struct DropTable;

impl Rule for DropTable {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg16]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            let Some(stmt) = DropStmt::cast(node.clone()) else {
                continue;
            };
            if stmt.object_kind() == Some(SyntaxKind::TABLE_KW) {
                analyzer.emit(
                    Diagnostic::warning(
                        "DROP TABLE destroys the table and everything depending on it",
                    )
                    .with_code(RuleCode::Mg16)
                    .with_range(stmt.syntax().text_range()),
                );
            }
        }
    }
}

// ===========================================================================
// MG17 — ALTER COLUMN DROP NOT NULL
// ===========================================================================

/// MG17 — `ALTER COLUMN … DROP NOT NULL` relaxes the column to accept nulls.
/// Code and clients that assumed the column was always set can break, and the
/// change is hard to reverse once nulls exist.
pub(super) struct DropNotNull;

impl Rule for DropNotNull {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg17]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            let Some(action) = AlterTableAction::cast(node.clone()) else {
                continue;
            };
            if action.kind() != AlterActionKind::AlterColumn {
                continue;
            }
            let kinds: Vec<SyntaxKind> = action
                .syntax()
                .descendants_with_tokens()
                .filter_map(|e| e.into_token())
                .map(|t| t.kind())
                .collect();
            let drops_not_null = kinds.contains(&SyntaxKind::DROP_KW)
                && kinds.contains(&SyntaxKind::NOT_KW)
                && kinds.contains(&SyntaxKind::NULL_KW);
            if drops_not_null {
                analyzer.emit(
                    Diagnostic::warning(
                        "ALTER COLUMN DROP NOT NULL lets nulls into a column clients may assume \
                         is always set",
                    )
                    .with_code(RuleCode::Mg17)
                    .with_range(action.syntax().text_range()),
                );
            }
        }
    }
}

// ===========================================================================
// MG18 — DROP DATABASE
// ===========================================================================

/// MG18 — `DROP DATABASE` destroys an entire database. It cannot run inside a
/// transaction and is almost never something a migration should do.
pub(super) struct DropDatabase;

impl Rule for DropDatabase {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg18]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            let Some(stmt) = DropStmt::cast(node.clone()) else {
                continue;
            };
            if stmt.object_kind() == Some(SyntaxKind::DATABASE_KW) {
                analyzer.emit(
                    Diagnostic::warning(
                        "DROP DATABASE destroys the whole database and all its data",
                    )
                    .with_code(RuleCode::Mg18)
                    .with_range(stmt.syntax().text_range()),
                );
            }
        }
    }
}

// ===========================================================================
// MG19 — CREATE INDEX CONCURRENTLY inside a transaction
// ===========================================================================

/// MG19 — `CREATE INDEX CONCURRENTLY` cannot run inside a transaction block;
/// Postgres rejects it at runtime. Move it out of the `BEGIN`/`COMMIT`.
pub(super) struct ConcurrentIndexInTransaction;

impl Rule for ConcurrentIndexInTransaction {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg19]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            let Some(stmt) = CreateIndexStmt::cast(node.clone()) else {
                continue;
            };
            if stmt.is_concurrent() && in_open_transaction(stmt.syntax(), root) {
                analyzer.emit(
                    Diagnostic::warning(
                        "CREATE INDEX CONCURRENTLY cannot run inside a transaction block and will \
                         be rejected by Postgres",
                    )
                    .with_code(RuleCode::Mg19)
                    .with_range(stmt.syntax().text_range()),
                );
            }
        }
    }
}

// ===========================================================================
// MG20 / MG21 — transaction hygiene
// ===========================================================================

/// Whether a transaction statement opens or closes a transaction.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TxnEvent {
    Open,
    Close,
}

/// Top-level transaction statements in source order, classified as open/close.
fn transaction_events(
    root: &banshee_syntax::SyntaxNode,
) -> Vec<(banshee_syntax::SyntaxNode, TxnEvent)> {
    root.children()
        .filter(|c| c.kind() == SyntaxKind::TRANSACTION_STMT)
        .filter_map(|child| {
            let opener = child
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .find(|t| t.kind().is_keyword())
                .map(|t| t.kind());
            match opener {
                Some(SyntaxKind::BEGIN_KW | SyntaxKind::START_KW) => {
                    Some((child.clone(), TxnEvent::Open))
                }
                Some(
                    SyntaxKind::COMMIT_KW
                    | SyntaxKind::END_KW
                    | SyntaxKind::ROLLBACK_KW
                    | SyntaxKind::ABORT_KW,
                ) => Some((child.clone(), TxnEvent::Close)),
                _ => None,
            }
        })
        .collect()
}

/// MG20 — a transaction opened with `BEGIN`/`START` but never matched by a
/// `COMMIT`/`ROLLBACK` leaves the migration in an open transaction.
pub(super) struct UncommittedTransaction;

impl Rule for UncommittedTransaction {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg20]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        let mut open: Vec<banshee_syntax::SyntaxNode> = Vec::new();
        for (node, event) in transaction_events(root) {
            match event {
                TxnEvent::Open => open.push(node),
                TxnEvent::Close => {
                    open.pop();
                }
            }
        }
        for node in open {
            analyzer.emit(
                Diagnostic::warning(
                    "transaction opened with BEGIN/START is never committed or rolled back",
                )
                .with_code(RuleCode::Mg20)
                .with_range(node.text_range()),
            );
        }
    }
}

/// MG21 — a `BEGIN`/`START` issued while a transaction is already open. Postgres
/// ignores the inner `BEGIN` (with a warning) and the matching `COMMIT` ends the
/// outer transaction early, which is rarely what was intended.
pub(super) struct TransactionNesting;

impl Rule for TransactionNesting {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg21]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        let mut depth = 0i32;
        for (node, event) in transaction_events(root) {
            match event {
                TxnEvent::Open => {
                    if depth > 0 {
                        analyzer.emit(
                            Diagnostic::warning(
                                "BEGIN/START inside an open transaction nests; Postgres ignores \
                                 the inner BEGIN and the next COMMIT ends the outer transaction",
                            )
                            .with_code(RuleCode::Mg21)
                            .with_range(node.text_range()),
                        );
                    }
                    depth += 1;
                }
                TxnEvent::Close => depth = (depth - 1).max(0),
            }
        }
    }
}

// ===========================================================================
// MG22 — non-idempotent statements (prefer IF [NOT] EXISTS)
// ===========================================================================

/// Whether a statement carries an `IF` keyword (i.e. `IF EXISTS` / `IF NOT
/// EXISTS`); in these DDL forms `IF` appears only there.
fn has_if_clause(node: &banshee_syntax::SyntaxNode) -> bool {
    node.children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == SyntaxKind::IF_KW)
}

/// MG22 — a `CREATE TABLE`, `CREATE INDEX` or `DROP` without `IF [NOT] EXISTS`
/// fails on re-run, so a migration that partially applied cannot be retried
/// cleanly. Outside a transaction, prefer the idempotent form. Statements
/// already inside a transaction roll back atomically, so they are exempt.
pub(super) struct RobustStatements;

impl Rule for RobustStatements {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg22]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            let (range, what) = match node.kind() {
                SyntaxKind::CREATE_TABLE_STMT => (node.text_range(), "CREATE TABLE"),
                SyntaxKind::CREATE_INDEX_STMT => (node.text_range(), "CREATE INDEX"),
                SyntaxKind::DROP_STMT => (node.text_range(), "DROP"),
                _ => continue,
            };
            if has_if_clause(node) || in_open_transaction(node, root) {
                continue;
            }
            analyzer.emit(
                Diagnostic::warning(format!(
                    "{what} without IF [NOT] EXISTS is not idempotent; a re-run after a partial \
                     failure will error"
                ))
                .with_code(RuleCode::Mg22)
                .with_range(range),
            );
        }
    }
}

// ===========================================================================
// MG23 — unqualified table name
// ===========================================================================

/// MG23 — `CREATE TABLE` without a schema qualifier relies on the session
/// `search_path`, so the table can land in an unexpected schema. Qualify it
/// (e.g. `public.t`). Temporary tables are exempt — they cannot be qualified.
pub(super) struct RequireTableSchema;

impl Rule for RequireTableSchema {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg23]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            let Some(table) = CreateTableStmt::cast(node.clone()) else {
                continue;
            };
            if table.is_temporary() {
                continue;
            }
            let Some(name) = table.name() else { continue };
            if name.schema().is_none() {
                analyzer.emit(
                    Diagnostic::warning(
                        "CREATE TABLE without a schema qualifier depends on search_path; qualify \
                         the name (e.g. public.t)",
                    )
                    .with_code(RuleCode::Mg23)
                    .with_range(name.syntax().text_range()),
                );
            }
        }
    }
}

// ===========================================================================
// MG24 — identifier longer than Postgres's 63-byte limit
// ===========================================================================

/// Postgres `NAMEDATALEN` is 64, so identifiers are limited to 63 bytes.
const MAX_IDENTIFIER_BYTES: usize = 63;

/// MG24 — an identifier longer than 63 bytes is silently truncated by Postgres,
/// so two distinct names can collide and a migration can act on the wrong object.
pub(super) struct IdentifierTooLong;

impl Rule for IdentifierTooLong {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Mg24]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Migration
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for token in root
            .descendants_with_tokens()
            .filter_map(|e| e.into_token())
        {
            let len = match token.kind() {
                SyntaxKind::IDENT => token.text().len(),
                // Strip the surrounding double quotes; "" escapes count once each.
                SyntaxKind::QUOTED_IDENT => {
                    token.text().trim_matches('"').replace("\"\"", "\"").len()
                }
                _ => continue,
            };
            if len > MAX_IDENTIFIER_BYTES {
                analyzer.emit(
                    Diagnostic::warning(format!(
                        "identifier is {len} bytes; Postgres truncates names to \
                         {MAX_IDENTIFIER_BYTES} bytes, which can cause collisions"
                    ))
                    .with_code(RuleCode::Mg24)
                    .with_range(token.text_range()),
                );
            }
        }
    }
}

// ===========================================================================
// Transaction tracking
// ===========================================================================

/// Whether `stmt` (a top-level statement) sits inside an open transaction
/// block — i.e. a `BEGIN`/`START TRANSACTION` precedes it without an
/// intervening `COMMIT`/`ROLLBACK`/`END`. `CONCURRENTLY` is invalid there.
fn in_open_transaction(
    stmt: &banshee_syntax::SyntaxNode,
    root: &banshee_syntax::SyntaxNode,
) -> bool {
    let start = stmt.text_range().start();
    let mut depth = 0i32;
    for child in root.children() {
        if child.text_range().start() >= start {
            break;
        }
        if child.kind() != SyntaxKind::TRANSACTION_STMT {
            continue;
        }
        let opener = child
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .find(|t| t.kind().is_keyword())
            .map(|t| t.kind());
        match opener {
            Some(SyntaxKind::BEGIN_KW | SyntaxKind::START_KW) => depth += 1,
            Some(
                SyntaxKind::COMMIT_KW
                | SyntaxKind::END_KW
                | SyntaxKind::ROLLBACK_KW
                | SyntaxKind::ABORT_KW,
            ) => depth = (depth - 1).max(0),
            _ => {}
        }
    }
    depth > 0
}
