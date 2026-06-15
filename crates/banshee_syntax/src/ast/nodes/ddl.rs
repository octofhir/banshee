//! DDL, transaction, and command statement AST nodes.
//!
//! Typed projections over the nodes the DDL/command grammar produces:
//! `CREATE TABLE/INDEX/VIEW/…`, `ALTER TABLE`, `DROP`, `TRUNCATE`, the
//! transaction/session statements, and the standalone commands. Accessors
//! expose what name resolution and the migration lints need — relation names,
//! column definitions, constraints, and the action set — while opaque tails
//! (storage params, function bodies) stay unstructured in the CST.

use crate::{SyntaxKind, SyntaxNode, SyntaxToken};

use super::super::support;
use super::super::traits::AstNode;
use super::clauses::WhereClause;
use super::expressions::{Expr, TypeName};
use super::statements::SelectStmt;
use super::table_refs::TableName;

/// Defines a newtype wrapper over a single `SyntaxKind` with its `AstNode` impl.
macro_rules! ast_node {
    ($(#[$meta:meta])* $name:ident => $kind:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        pub struct $name(pub(crate) SyntaxNode);

        impl AstNode for $name {
            fn can_cast(kind: SyntaxKind) -> bool {
                kind == SyntaxKind::$kind
            }

            fn cast(node: SyntaxNode) -> Option<Self> {
                Self::can_cast(node.kind()).then(|| Self(node))
            }

            fn syntax(&self) -> &SyntaxNode {
                &self.0
            }
        }
    };
}

/// The first identifier token (plain or quoted) directly under `node`.
fn first_ident(node: &SyntaxNode) -> Option<SyntaxToken> {
    support::tokens(node, SyntaxKind::IDENT)
        .next()
        .or_else(|| support::tokens(node, SyntaxKind::QUOTED_IDENT).next())
}

/// Whether `node` has a direct child token of `kind`.
fn has_token(node: &SyntaxNode, kind: SyntaxKind) -> bool {
    support::token(node, kind).is_some()
}

// ===========================================================================
// CREATE TABLE
// ===========================================================================

ast_node! {
    /// A `CREATE TABLE` statement (including `CREATE TABLE … AS`).
    CreateTableStmt => CREATE_TABLE_STMT
}

impl CreateTableStmt {
    /// The relation being created.
    pub fn name(&self) -> Option<TableName> {
        support::child(&self.0)
    }

    /// `TEMP` / `TEMPORARY` table.
    pub fn is_temporary(&self) -> bool {
        has_token(&self.0, SyntaxKind::TEMP_KW) || has_token(&self.0, SyntaxKind::TEMPORARY_KW)
    }

    /// `UNLOGGED` table.
    pub fn is_unlogged(&self) -> bool {
        has_token(&self.0, SyntaxKind::UNLOGGED_KW)
    }

    /// `IF NOT EXISTS` guard present.
    pub fn if_not_exists(&self) -> bool {
        has_token(&self.0, SyntaxKind::IF_KW) && has_token(&self.0, SyntaxKind::EXISTS_KW)
    }

    /// The parenthesized column/constraint list, when the table is defined
    /// with one (i.e. not `CREATE TABLE … AS`).
    pub fn column_def_list(&self) -> Option<ColumnDefList> {
        support::child(&self.0)
    }

    /// The backing query for `CREATE TABLE … AS SELECT`.
    pub fn as_query(&self) -> Option<SelectStmt> {
        support::child(&self.0)
    }

    /// The column definitions in declaration order.
    pub fn columns(&self) -> Vec<ColumnDef> {
        self.column_def_list()
            .map(|l| l.columns().collect())
            .unwrap_or_default()
    }

    /// The table-level constraints.
    pub fn constraints(&self) -> Vec<Constraint> {
        self.column_def_list()
            .map(|l| l.constraints().collect())
            .unwrap_or_default()
    }
}

ast_node! {
    /// The parenthesized body of a `CREATE TABLE`: column definitions plus
    /// table-level constraints.
    ColumnDefList => COLUMN_DEF_LIST
}

impl ColumnDefList {
    /// Column definitions in order.
    pub fn columns(&self) -> impl Iterator<Item = ColumnDef> + '_ {
        support::children(&self.0)
    }

    /// Table-level constraints (those written outside a column definition).
    pub fn constraints(&self) -> impl Iterator<Item = Constraint> + '_ {
        support::children(&self.0)
    }
}

ast_node! {
    /// A single column definition: `name type [constraints…]`.
    ColumnDef => COLUMN_DEF
}

impl ColumnDef {
    /// The column name.
    pub fn name(&self) -> Option<SyntaxToken> {
        first_ident(&self.0)
    }

    /// The declared column type.
    pub fn type_name(&self) -> Option<TypeName> {
        support::child(&self.0)
    }

    /// Inline (column-level) constraints.
    pub fn constraints(&self) -> impl Iterator<Item = Constraint> + '_ {
        support::children(&self.0)
    }

    /// `NOT NULL` declared on the column.
    pub fn is_not_null(&self) -> bool {
        self.0
            .children()
            .any(|c| c.kind() == SyntaxKind::NOT_NULL_CONSTRAINT)
    }

    /// `PRIMARY KEY` declared inline on the column.
    pub fn is_primary_key(&self) -> bool {
        self.0
            .children()
            .any(|c| c.kind() == SyntaxKind::PRIMARY_KEY_CONSTRAINT)
    }

    /// The inline `DEFAULT` clause, if any.
    pub fn default_expr(&self) -> Option<DefaultExpr> {
        support::child(&self.0)
    }
}

ast_node! {
    /// A `DEFAULT <expr>` column clause.
    DefaultExpr => DEFAULT_EXPR
}

impl DefaultExpr {
    /// The default value expression.
    pub fn expr(&self) -> Option<Expr> {
        support::child(&self.0)
    }
}

// ===========================================================================
// Constraints
// ===========================================================================

/// A column- or table-level constraint.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Constraint {
    PrimaryKey(PrimaryKeyConstraint),
    ForeignKey(ForeignKeyConstraint),
    Unique(UniqueConstraint),
    Check(CheckConstraint),
    NotNull(NotNullConstraint),
    /// Any other constraint form (`NULL`, `GENERATED`, `EXCLUDE`, `LIKE`, …).
    Other(OtherConstraint),
}

impl AstNode for Constraint {
    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::PRIMARY_KEY_CONSTRAINT
                | SyntaxKind::FOREIGN_KEY_CONSTRAINT
                | SyntaxKind::UNIQUE_CONSTRAINT
                | SyntaxKind::CHECK_CONSTRAINT
                | SyntaxKind::NOT_NULL_CONSTRAINT
                | SyntaxKind::CONSTRAINT
        )
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::PRIMARY_KEY_CONSTRAINT => {
                Some(Constraint::PrimaryKey(PrimaryKeyConstraint(node)))
            }
            SyntaxKind::FOREIGN_KEY_CONSTRAINT => {
                Some(Constraint::ForeignKey(ForeignKeyConstraint(node)))
            }
            SyntaxKind::UNIQUE_CONSTRAINT => Some(Constraint::Unique(UniqueConstraint(node))),
            SyntaxKind::CHECK_CONSTRAINT => Some(Constraint::Check(CheckConstraint(node))),
            SyntaxKind::NOT_NULL_CONSTRAINT => Some(Constraint::NotNull(NotNullConstraint(node))),
            SyntaxKind::CONSTRAINT => Some(Constraint::Other(OtherConstraint(node))),
            _ => None,
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Constraint::PrimaryKey(it) => it.syntax(),
            Constraint::ForeignKey(it) => it.syntax(),
            Constraint::Unique(it) => it.syntax(),
            Constraint::Check(it) => it.syntax(),
            Constraint::NotNull(it) => it.syntax(),
            Constraint::Other(it) => it.syntax(),
        }
    }
}

impl Constraint {
    /// `NOT VALID` written on this constraint. `VALID` lexes as an identifier,
    /// so its presence in the constraint node marks the deferred-validation
    /// tail; `NOT NULL` constraints are a distinct node kind and never match.
    pub fn is_not_valid(&self) -> bool {
        if matches!(self, Constraint::NotNull(_)) {
            return false;
        }
        support::tokens(self.syntax(), SyntaxKind::IDENT)
            .any(|t| t.text().eq_ignore_ascii_case("valid"))
            && has_token(self.syntax(), SyntaxKind::NOT_KW)
    }
}

ast_node! {
    /// A `PRIMARY KEY` constraint.
    PrimaryKeyConstraint => PRIMARY_KEY_CONSTRAINT
}

ast_node! {
    /// A `FOREIGN KEY … REFERENCES` constraint.
    ForeignKeyConstraint => FOREIGN_KEY_CONSTRAINT
}

impl ForeignKeyConstraint {
    /// The referenced relation.
    pub fn references(&self) -> Option<TableName> {
        support::child(&self.0)
    }
}

ast_node! {
    /// A `UNIQUE` constraint.
    UniqueConstraint => UNIQUE_CONSTRAINT
}

ast_node! {
    /// A `CHECK (expr)` constraint.
    CheckConstraint => CHECK_CONSTRAINT
}

impl CheckConstraint {
    /// The checked predicate.
    pub fn condition(&self) -> Option<Expr> {
        support::child(&self.0)
    }
}

ast_node! {
    /// A `NOT NULL` constraint.
    NotNullConstraint => NOT_NULL_CONSTRAINT
}

ast_node! {
    /// Any other constraint form not modeled with a dedicated node.
    OtherConstraint => CONSTRAINT
}

// ===========================================================================
// CREATE INDEX
// ===========================================================================

ast_node! {
    /// A `CREATE INDEX` / `CREATE UNIQUE INDEX` statement.
    CreateIndexStmt => CREATE_INDEX_STMT
}

impl CreateIndexStmt {
    /// `UNIQUE` index.
    pub fn is_unique(&self) -> bool {
        has_token(&self.0, SyntaxKind::UNIQUE_KW)
    }

    /// `CONCURRENTLY` build.
    pub fn is_concurrent(&self) -> bool {
        has_token(&self.0, SyntaxKind::CONCURRENTLY_KW)
    }

    /// The indexed relation.
    pub fn table(&self) -> Option<TableName> {
        support::child(&self.0)
    }

    /// The access method named after `USING`, if any.
    pub fn using_method(&self) -> Option<SyntaxToken> {
        // The relation name is a TABLE_REF node, so the only bare IDENT directly
        // under the statement that follows USING is the access method (or the
        // optional index name before ON). Prefer the token adjacent to USING.
        let mut seen_using = false;
        for elem in self.0.children_with_tokens() {
            if let cstree::util::NodeOrToken::Token(t) = elem {
                if seen_using && matches!(t.kind(), SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT) {
                    return Some(t.clone());
                }
                if t.kind() == SyntaxKind::USING_KW {
                    seen_using = true;
                }
            }
        }
        None
    }

    /// The partial-index predicate, if any.
    pub fn where_clause(&self) -> Option<WhereClause> {
        support::child(&self.0)
    }
}

// ===========================================================================
// ALTER TABLE
// ===========================================================================

ast_node! {
    /// An `ALTER TABLE` statement.
    AlterStmt => ALTER_STMT
}

impl AlterStmt {
    /// The relation being altered.
    pub fn table(&self) -> Option<TableName> {
        support::child(&self.0)
    }

    /// `ALTER TABLE … RENAME …` (table or column rename); such statements carry
    /// no [`AlterTableAction`] children.
    pub fn is_rename(&self) -> bool {
        has_token(&self.0, SyntaxKind::RENAME_KW)
    }

    /// `RENAME COLUMN`/`RENAME CONSTRAINT` (as opposed to renaming the table).
    pub fn renames_subobject(&self) -> bool {
        self.is_rename()
            && (has_token(&self.0, SyntaxKind::COLUMN_KW)
                || has_token(&self.0, SyntaxKind::CONSTRAINT_KW))
    }

    /// The alter actions in order (empty for a `RENAME`).
    pub fn actions(&self) -> impl Iterator<Item = AlterTableAction> + '_ {
        support::children(&self.0)
    }
}

/// What an [`AlterTableAction`] does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlterActionKind {
    AddColumn,
    AddConstraint,
    DropColumn,
    DropConstraint,
    AlterColumn,
    Other,
}

ast_node! {
    /// One action within an `ALTER TABLE` (e.g. `ADD COLUMN …`,
    /// `ALTER COLUMN … TYPE …`, `DROP CONSTRAINT …`).
    AlterTableAction => ALTER_TABLE_ACTION
}

impl AlterTableAction {
    /// Classifies the action by its leading keyword and contents.
    pub fn kind(&self) -> AlterActionKind {
        match support::first_token(&self.0).map(|t| t.kind()) {
            Some(SyntaxKind::ADD_KW) => {
                if self.added_column().is_some() {
                    AlterActionKind::AddColumn
                } else {
                    AlterActionKind::AddConstraint
                }
            }
            Some(SyntaxKind::DROP_KW) => {
                if has_token(&self.0, SyntaxKind::COLUMN_KW) {
                    AlterActionKind::DropColumn
                } else if has_token(&self.0, SyntaxKind::CONSTRAINT_KW) {
                    AlterActionKind::DropConstraint
                } else {
                    // Bare `DROP <name>` defaults to a column drop in PostgreSQL.
                    AlterActionKind::DropColumn
                }
            }
            Some(SyntaxKind::ALTER_KW) => AlterActionKind::AlterColumn,
            _ => AlterActionKind::Other,
        }
    }

    /// The column added by `ADD [COLUMN] …`, if this is an add-column action.
    pub fn added_column(&self) -> Option<ColumnDef> {
        support::child(&self.0)
    }

    /// The constraint added by `ADD …`, if this is an add-constraint action.
    pub fn added_constraint(&self) -> Option<Constraint> {
        support::child(&self.0)
    }

    /// The new type from `ALTER COLUMN … TYPE …` (or `SET DATA TYPE …`).
    pub fn new_type(&self) -> Option<TypeName> {
        support::child(&self.0)
    }

    /// The target column/constraint name for `DROP …` / `ALTER COLUMN …`.
    pub fn target_name(&self) -> Option<SyntaxToken> {
        first_ident(&self.0)
    }

    /// `ALTER COLUMN … TYPE …` changes a column's type.
    pub fn changes_type(&self) -> bool {
        self.kind() == AlterActionKind::AlterColumn
            && (has_token(&self.0, SyntaxKind::TYPE_KW) || self.new_type().is_some())
    }
}

// ===========================================================================
// DROP / TRUNCATE
// ===========================================================================

ast_node! {
    /// A `DROP <object> …` statement.
    DropStmt => DROP_STMT
}

impl DropStmt {
    /// The dropped object kind keyword (`TABLE`, `INDEX`, `VIEW`, …).
    pub fn object_kind(&self) -> Option<SyntaxKind> {
        self.0
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .map(|t| t.kind())
            .find(|k| {
                matches!(
                    k,
                    SyntaxKind::TABLE_KW
                        | SyntaxKind::INDEX_KW
                        | SyntaxKind::VIEW_KW
                        | SyntaxKind::SEQUENCE_KW
                        | SyntaxKind::SCHEMA_KW
                        | SyntaxKind::EXTENSION_KW
                        | SyntaxKind::TYPE_KW
                        | SyntaxKind::DATABASE_KW
                        | SyntaxKind::MATERIALIZED_KW
                        | SyntaxKind::FUNCTION_KW
                        | SyntaxKind::PROCEDURE_KW
                        | SyntaxKind::TRIGGER_KW
                )
            })
    }

    /// `CONCURRENTLY` (valid for `DROP INDEX`).
    pub fn is_concurrent(&self) -> bool {
        has_token(&self.0, SyntaxKind::CONCURRENTLY_KW)
    }

    /// `IF EXISTS` guard.
    pub fn if_exists(&self) -> bool {
        has_token(&self.0, SyntaxKind::IF_KW) && has_token(&self.0, SyntaxKind::EXISTS_KW)
    }

    /// `CASCADE`.
    pub fn is_cascade(&self) -> bool {
        has_token(&self.0, SyntaxKind::CASCADE_KW)
    }

    /// The dropped object names.
    pub fn names(&self) -> impl Iterator<Item = TableName> + '_ {
        support::children(&self.0)
    }
}

ast_node! {
    /// A `TRUNCATE` statement.
    TruncateStmt => TRUNCATE_STMT
}

impl TruncateStmt {
    /// The truncated relations.
    pub fn names(&self) -> impl Iterator<Item = TableName> + '_ {
        support::children(&self.0)
    }

    /// `CASCADE` truncates dependent tables too.
    pub fn is_cascade(&self) -> bool {
        has_token(&self.0, SyntaxKind::CASCADE_KW)
    }

    /// `RESTART IDENTITY`.
    pub fn restart_identity(&self) -> bool {
        has_token(&self.0, SyntaxKind::RESTART_KW)
    }
}

// ===========================================================================
// Other CREATE forms
// ===========================================================================

ast_node! {
    /// A `CREATE [MATERIALIZED] VIEW` statement.
    CreateViewStmt => CREATE_VIEW_STMT
}

impl CreateViewStmt {
    /// The view name.
    pub fn name(&self) -> Option<TableName> {
        support::child(&self.0)
    }

    /// `MATERIALIZED VIEW`.
    pub fn is_materialized(&self) -> bool {
        has_token(&self.0, SyntaxKind::MATERIALIZED_KW)
    }

    /// The backing query.
    pub fn query(&self) -> Option<SelectStmt> {
        support::child(&self.0)
    }
}

ast_node! {
    /// A `CREATE SEQUENCE` statement.
    CreateSequenceStmt => CREATE_SEQUENCE_STMT
}

impl CreateSequenceStmt {
    /// The sequence name.
    pub fn name(&self) -> Option<TableName> {
        support::child(&self.0)
    }
}

ast_node! {
    /// A `CREATE SCHEMA` statement.
    CreateSchemaStmt => CREATE_SCHEMA_STMT
}

ast_node! {
    /// A `CREATE EXTENSION` statement.
    CreateExtensionStmt => CREATE_EXTENSION_STMT
}

ast_node! {
    /// A `CREATE TYPE` statement.
    CreateTypeStmt => CREATE_TYPE_STMT
}

impl CreateTypeStmt {
    /// The type name.
    pub fn name(&self) -> Option<TableName> {
        support::child(&self.0)
    }
}

ast_node! {
    /// A `CREATE FUNCTION` / `CREATE PROCEDURE` statement.
    CreateFunctionStmt => CREATE_FUNCTION_STMT
}

impl CreateFunctionStmt {
    /// The function/procedure name.
    pub fn name(&self) -> Option<TableName> {
        support::child(&self.0)
    }
}

ast_node! {
    /// A `CREATE TRIGGER` statement.
    CreateTriggerStmt => CREATE_TRIGGER_STMT
}

ast_node! {
    /// A `COMMENT ON …` statement.
    CommentStmt => COMMENT_STMT
}

// ===========================================================================
// Transaction / session / command statements
// ===========================================================================

ast_node! {
    /// A transaction-control statement (`BEGIN`, `COMMIT`, `ROLLBACK`, …).
    TransactionStmt => TRANSACTION_STMT
}

ast_node! {
    /// A `SET` session statement.
    SetStmt => SET_STMT
}

ast_node! {
    /// A `SHOW` statement.
    ShowStmt => SHOW_STMT
}

ast_node! {
    /// A `RESET` statement.
    ResetStmt => RESET_STMT
}

ast_node! {
    /// An `EXPLAIN` statement.
    ExplainStmt => EXPLAIN_STMT
}

ast_node! {
    /// A `CALL` statement.
    CallStmt => CALL_STMT
}

ast_node! {
    /// A `DO` anonymous block.
    DoStmt => DO_STMT
}

ast_node! {
    /// A `VACUUM` statement.
    VacuumStmt => VACUUM_STMT
}

ast_node! {
    /// An `ANALYZE` statement.
    AnalyzeStmt => ANALYZE_STMT
}

ast_node! {
    /// A `COPY` statement.
    CopyStmt => COPY_STMT
}

ast_node! {
    /// A `GRANT` statement.
    GrantStmt => GRANT_STMT
}

ast_node! {
    /// A `REVOKE` statement.
    RevokeStmt => REVOKE_STMT
}

ast_node! {
    /// A `MERGE` statement.
    MergeStmt => MERGE_STMT
}

impl MergeStmt {
    /// The target relation (`MERGE INTO <target>`).
    pub fn target(&self) -> Option<TableName> {
        support::child(&self.0)
    }
}
