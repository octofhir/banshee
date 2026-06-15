//! HIR (High-level Intermediate Representation) for semantic analysis.
//!
//! This crate provides semantic analysis for SQL queries, including:
//! - Scope management and name resolution
//! - Table and column binding tracking
//! - CTE (Common Table Expression) resolution
//! - SELECT alias tracking for ORDER BY
//! - JSONB column type tracking
//!
//! # Stability
//!
//! Pre-1.0. The intended public surface is the crate-root re-exports —
//! notably `analyze_query` / `analyze_query_with_options`, `SchemaProvider`,
//! `Diagnostic`, `RuleCode`, `Fix`, `TextEdit`, `Severity`, and the binding /
//! resolution types. Internals of the `analyze`, `lint` and `resolve` modules
//! may change without a semver bump.
//!
//! ## Extension surface
//!
//! Downstream crates can embed the engine without forking it. The following are
//! a committed contract and will not change incompatibly within a minor version:
//!
//! - [`SchemaProvider`] — supply schema (tables, columns, JSONB shape) from any
//!   source, e.g. a FHIR `StructureDefinition` instead of live introspection.
//! - [`LintRulePack`] — add domain-specific lint rules.
//! - [`AnalysisOptions`] / [`BuiltinLintPack`] — select built-in packs and inject
//!   external packs via `external_lint_packs`.
//! - [`analyze_query_with_options`] — the entry point that takes both.
//!
//! ```ignore
//! use std::sync::Arc;
//! use banshee_hir::{analyze_query_with_options, AnalysisOptions, LintRulePack};
//!
//! let options = AnalysisOptions::new().with_external_lint_pack(Arc::new(MyPack));
//! let analysis = analyze_query_with_options(&parse, &my_provider, &options);
//! ```
//!
//! # Architecture
//!
//! The HIR crate is organized into several modules:
//!
//! - [`binding`] - Types representing resolved references (tables, columns, CTEs)
//! - [`scope`] - Scope management for name resolution
//! - [`resolve`] - Name resolution functions
//! - [`analyze`] - Query analysis entry point
//!
//! # Usage
//!
//! ```ignore
//! use banshee_hir::{Analyzer, ScopeBuilder, TableBinding};
//!
//! // Build a scope with table bindings
//! let scope = ScopeBuilder::new()
//!     .add_table(TableBinding::table(None, "users".to_string()))
//!     .build();
//!
//! // Resolve a table reference
//! let resolution = scope.resolve_table("users");
//! assert!(resolution.ok().is_some());
//! ```
//!
//! # Design Notes
//!
//! - Scopes are immutable and use `Arc` for efficient sharing
//! - All types implement `Send + Sync` for thread-safe analysis
//! - Name resolution follows PostgreSQL's case-insensitivity rules
//! - Analysis can run without schema information (graceful degradation)

pub mod analyze;
pub mod binding;
pub mod lint;
pub mod noqa;
pub mod resolve;
pub mod scope;

// Re-export main types
pub use analyze::{
    Analysis, AnalysisOptions, Analyzer, BREAKING, BuiltinLintPack, ColumnInfo, Diagnostic, Fix,
    LintRulePack, MigrationColumn, MigrationColumns, NullSchemaProvider, RelatedInfo,
    ResolvedReference, ResolvedTableRef, RuleCode, SchemaProvider, Severity, TableInfo, TableType,
    TextEdit, analyze_query, analyze_query_with_options, suggest_similar,
};
pub use binding::{
    ColumnBinding, CteBinding, DataType, Resolution, ResolvedColumn, TableBinding, TableSource,
};
pub use lint::collect_migration_columns;
pub use resolve::{
    ColumnValidation, JsonbChainError, JsonbChainValidation, JsonbChild, JsonbColumnInfo,
    JsonbStructure, JsonbValueType, QualifiedResolution, TableValidation, find_jsonb_columns,
    jsonb_access_result_type, resolve_column, resolve_cte, resolve_order_by, resolve_table,
    validate_column_reference, validate_jsonb_chain, validate_table_reference,
};
pub use scope::{Scope, ScopeBuilder};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_workflow() {
        // 1. Create table bindings
        let users = TableBinding::table(Some("public".to_string()), "users".to_string())
            .with_alias("u".to_string())
            .with_columns(vec![
                ColumnBinding::new("id".to_string(), 0).with_type(DataType::Integer),
                ColumnBinding::new("name".to_string(), 1).with_type(DataType::Text),
                ColumnBinding::new("email".to_string(), 2).with_type(DataType::Text),
            ]);

        let orders = TableBinding::table(Some("public".to_string()), "orders".to_string())
            .with_alias("o".to_string())
            .with_columns(vec![
                ColumnBinding::new("id".to_string(), 0).with_type(DataType::Integer),
                ColumnBinding::new("user_id".to_string(), 1).with_type(DataType::Integer),
                ColumnBinding::new("data".to_string(), 2).with_type(DataType::Jsonb),
            ]);

        // 2. Build scope
        let scope = ScopeBuilder::new()
            .add_table(users)
            .add_table(orders)
            .build();

        // 3. Resolve table by alias (acceptance criteria 1)
        let resolved = scope.resolve_table("u");
        assert!(resolved.ok().is_some());

        // 4. Resolve qualified column reference
        let result = scope.resolve_column("id", Some("u"));
        assert!(result.ok().is_some());

        // 5. Detect ambiguous column reference (acceptance criteria 7)
        let ambiguous = scope.resolve_column("id", None);
        assert!(ambiguous.is_ambiguous());

        // 6. Find JSONB columns
        let jsonb_cols = find_jsonb_columns(&scope);
        assert_eq!(jsonb_cols.len(), 1);
        assert_eq!(jsonb_cols[0].column.name, "data");
    }

    #[test]
    fn test_cte_resolution() {
        // Acceptance criteria 2: Resolve CTE reference
        let cte =
            CteBinding::new("x".to_string()).with_columns(vec!["a".to_string(), "b".to_string()]);

        let scope = ScopeBuilder::new().add_cte(cte).build();

        // CTE should be resolvable
        assert!(scope.resolve_cte("x").ok().is_some());

        // CTE should also be available as a table
        assert!(scope.resolve_table("x").ok().is_some());
    }

    #[test]
    fn test_subquery_alias() {
        // Acceptance criteria 3: Resolve subquery alias
        let subquery = TableBinding::subquery("s".to_string())
            .with_columns(vec![ColumnBinding::new("val".to_string(), 0)]);

        let scope = ScopeBuilder::new().add_table(subquery).build();

        assert!(scope.resolve_table("s").ok().is_some());

        let col = scope.resolve_column("val", Some("s"));
        assert!(col.ok().is_some());
    }

    #[test]
    fn test_order_by_resolution() {
        // Acceptance criteria 4 & 5: ORDER BY ordinal and alias
        let select_columns = vec![
            ColumnBinding::new("id".to_string(), 0),
            ColumnBinding::new("name".to_string(), 1),
        ];

        let scope = ScopeBuilder::new()
            .add_select_alias(
                "the_name".to_string(),
                ColumnBinding::new("name".to_string(), 1),
            )
            .build();

        // Ordinal reference (ORDER BY 1)
        let result = scope.resolve_order_by("1", &select_columns);
        let col = result.ok().unwrap();
        assert_eq!(col.name, "id");

        // Ordinal reference (ORDER BY 2)
        let result = scope.resolve_order_by("2", &select_columns);
        let col = result.ok().unwrap();
        assert_eq!(col.name, "name");

        // Alias reference (ORDER BY the_name)
        let result = scope.resolve_order_by("the_name", &select_columns);
        let col = result.ok().unwrap();
        assert_eq!(col.name, "name");
    }

    #[test]
    fn test_lateral_scope() {
        // Acceptance criteria 6: LATERAL scope
        let t1 = TableBinding::table(None, "t1".to_string())
            .with_columns(vec![ColumnBinding::new("id".to_string(), 0)]);

        let parent = ScopeBuilder::new().add_table(t1).build();

        // Create LATERAL scope that can reference t1
        let lateral_scope = analyze::analyze_lateral_scope(
            vec!["t1".to_string()],
            vec![TableBinding::table(None, "t2".to_string())],
            parent,
        );

        // t1 should be visible from LATERAL subquery
        assert!(lateral_scope.resolve_table("t1").ok().is_some());
        assert!(lateral_scope.resolve_table("t2").ok().is_some());
        assert!(lateral_scope.is_lateral());
    }

    #[test]
    fn test_types_are_send_sync() {
        // Acceptance criteria 8: All types are Send + Sync
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<Scope>();
        assert_send_sync::<ScopeBuilder>();
        assert_send_sync::<TableBinding>();
        assert_send_sync::<ColumnBinding>();
        assert_send_sync::<CteBinding>();
        assert_send_sync::<TableSource>();
        assert_send_sync::<Analysis>();
        assert_send_sync::<Diagnostic>();
        assert_send_sync::<DataType>();
        assert_send_sync::<Resolution<ResolvedColumn>>();
        assert_send_sync::<Severity>();
        assert_send_sync::<TableInfo>();
        assert_send_sync::<TableType>();
        assert_send_sync::<RelatedInfo>();
        assert_send_sync::<QualifiedResolution>();
        assert_send_sync::<JsonbColumnInfo>();
        assert_send_sync::<JsonbStructure>();
        assert_send_sync::<JsonbValueType>();
        assert_send_sync::<JsonbChild>();
        assert_send_sync::<TableValidation>();
        assert_send_sync::<ColumnValidation>();
        assert_send_sync::<JsonbChainError>();
        assert_send_sync::<JsonbChainValidation>();
    }
}
