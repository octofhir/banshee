//! Safety rules for writes with unrestricted filters or implicit columns.

use banshee_syntax::SyntaxKind;
use banshee_syntax::ast::{AstNode, DeleteStmt, Expr, LiteralKind, UpdateStmt};

use super::Rule;
use crate::analyze::{Analyzer, BuiltinLintPack, Diagnostic, RuleCode};

/// SF01 — `UPDATE` without `WHERE`.
pub(super) struct UpdateWithoutWhere;

impl Rule for UpdateWithoutWhere {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Sf01]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Core
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            if let Some(update) = UpdateStmt::cast(node.clone()) {
                lint_update_without_where(&update, analyzer);
            }
        }
    }
}

/// SF02 — `DELETE` without `WHERE`.
pub(super) struct DeleteWithoutWhere;

impl Rule for DeleteWithoutWhere {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Sf02]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Core
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            if let Some(delete) = DeleteStmt::cast(node.clone()) {
                lint_delete_without_where(&delete, analyzer);
            }
        }
    }
}

/// SF03 — `INSERT` without an explicit column list.
pub(super) struct InsertWithoutColumns;

impl Rule for InsertWithoutColumns {
    fn codes(&self) -> &'static [RuleCode] {
        &[RuleCode::Sf03]
    }
    fn group(&self) -> BuiltinLintPack {
        BuiltinLintPack::Core
    }
    fn run(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
        for node in root.descendants() {
            if node.kind() == SyntaxKind::INSERT_STMT {
                lint_insert_without_columns(node, analyzer);
            }
        }
    }
}

/// SF03 — `INSERT INTO t VALUES (...)` binds values positionally, so adding,
/// dropping or reordering a column silently corrupts the insert. List the
/// target columns explicitly. `DEFAULT VALUES` is exempt.
fn lint_insert_without_columns(stmt: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
    if stmt
        .children()
        .any(|c| c.kind() == SyntaxKind::INSERT_COLUMNS)
    {
        return;
    }
    // `INSERT INTO t DEFAULT VALUES` names no columns by design.
    let has_default = stmt
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == SyntaxKind::DEFAULT_KW);
    if has_default {
        return;
    }
    let range = stmt
        .children()
        .find(|c| c.kind() == SyntaxKind::TABLE_REF)
        .map(|t| t.text_range())
        .unwrap_or_else(|| stmt.text_range());
    analyzer.emit(
        Diagnostic::warning("INSERT without an explicit column list binds values positionally")
            .with_code(RuleCode::Sf03)
            .with_range(range),
    );
}

fn lint_update_without_where(stmt: &UpdateStmt, analyzer: &mut Analyzer<'_>) {
    if let Some(filter) = stmt.where_clause() {
        if filter.expr().is_some_and(is_always_true) {
            analyzer.emit(
                Diagnostic::warning("UPDATE with an always-true WHERE can affect all rows")
                    .with_code(RuleCode::Sf01)
                    .with_range(filter.syntax().text_range()),
            );
        }
    } else {
        analyzer.emit(
            Diagnostic::warning("UPDATE without WHERE affects all rows")
                .with_code(RuleCode::Sf01)
                .with_range(stmt.syntax().text_range()),
        );
    }
}

fn lint_delete_without_where(stmt: &DeleteStmt, analyzer: &mut Analyzer<'_>) {
    if let Some(filter) = stmt.where_clause() {
        if filter.expr().is_some_and(is_always_true) {
            analyzer.emit(
                Diagnostic::warning("DELETE with an always-true WHERE can affect all rows")
                    .with_code(RuleCode::Sf02)
                    .with_range(filter.syntax().text_range()),
            );
        }
    } else {
        analyzer.emit(
            Diagnostic::warning("DELETE without WHERE affects all rows")
                .with_code(RuleCode::Sf02)
                .with_range(stmt.syntax().text_range()),
        );
    }
}

fn is_always_true(expr: Expr) -> bool {
    match expr {
        Expr::Paren(paren) => paren.expr().is_some_and(is_always_true),
        Expr::Literal(literal) => literal.kind() == Some(LiteralKind::Boolean(true)),
        Expr::Binary(binary)
            if binary
                .syntax()
                .children_with_tokens()
                .filter_map(|element| element.into_token())
                .find(|token| !token.kind().is_trivia())
                .is_some_and(|token| token.kind() == SyntaxKind::EQ) =>
        {
            let (Some(left), Some(right)) = (
                binary.lhs().and_then(decimal_int4),
                binary.rhs().and_then(decimal_int4),
            ) else {
                return false;
            };
            left == right
        }
        _ => false,
    }
}

fn decimal_int4(expr: Expr) -> Option<i32> {
    match expr {
        Expr::Paren(paren) => decimal_int4(paren.expr()?),
        Expr::Literal(literal) if literal.kind() == Some(LiteralKind::Integer) => {
            literal.token()?.text().parse().ok()
        }
        _ => None,
    }
}
