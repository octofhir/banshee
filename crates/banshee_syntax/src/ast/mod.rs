//! AST (Abstract Syntax Tree) projection over the CST.
//!
//! This module provides typed wrappers over the untyped CST (Concrete Syntax Tree),
//! enabling convenient and type-safe tree traversal and analysis.
//!
//! # Design Philosophy
//!
//! The AST layer is a **projection**, not a transformation. Each AST node is a
//! zero-cost newtype wrapper around a `SyntaxNode`, providing:
//!
//! - Type-safe access to specific node kinds
//! - Convenient accessor methods for child nodes and tokens
//! - Pattern matching on node types via enums
//!
//! # Usage
//!
//! ```ignore
//! use banshee_syntax::ast::{AstNode, SourceFile, Stmt, SelectStmt};
//!
//! let parse = banshee_parser::parse("SELECT * FROM users");
//! let source = SourceFile::cast(parse.syntax()).unwrap();
//!
//! for stmt in source.statements() {
//!     match stmt {
//!         Stmt::Select(select) => {
//!             if let Some(from) = select.from_clause() {
//!                 for table in from.table_refs() {
//!                     println!("Table: {:?}", table);
//!                 }
//!             }
//!         }
//!         _ => {}
//!     }
//! }
//! ```
//!
//! # Module Structure
//!
//! - `traits` - Core `AstNode` and `AstToken` traits
//! - `support` - Helper functions for node traversal
//! - `nodes` - All AST node definitions organized by category

mod call_names;
mod nodes;
mod support;
mod traits;

// Re-export traits at the module level
pub use call_names::{CallName, call_names};
pub use traits::{AstNode, AstToken};

// Re-export support functions
pub use support::{child, children, first_token, last_token, nth_child, token, tokens};

// Re-export all nodes
pub use nodes::*;

#[cfg(test)]
mod tests {
    use super::*;

    // Basic compile-time checks that traits are properly defined
    fn _assert_send_sync<T: Send + Sync>() {}
    fn _assert_clone<T: Clone>() {}
    fn _assert_debug<T: std::fmt::Debug>() {}
    fn _assert_eq<T: PartialEq + Eq>() {}
    fn _assert_hash<T: std::hash::Hash>() {}

    #[test]
    fn check_traits() {
        // Acceptance Criteria 5: All AST types implement Clone, Debug, PartialEq, Eq, Hash
        _assert_clone::<SourceFile>();
        _assert_clone::<SelectStmt>();
        _assert_clone::<InsertStmt>();
        _assert_clone::<UpdateStmt>();
        _assert_clone::<DeleteStmt>();
        _assert_clone::<Expr>();
        _assert_clone::<JsonbAccessExpr>();
        _assert_clone::<FromClause>();
        _assert_clone::<TableRef>();

        _assert_debug::<SourceFile>();
        _assert_debug::<SelectStmt>();
        _assert_debug::<Expr>();
        _assert_debug::<JsonbAccessExpr>();

        _assert_eq::<SourceFile>();
        _assert_eq::<SelectStmt>();
        _assert_eq::<Expr>();
        _assert_eq::<JsonbAccessExpr>();

        _assert_hash::<SourceFile>();
        _assert_hash::<SelectStmt>();
        _assert_hash::<Expr>();
        _assert_hash::<JsonbAccessExpr>();

        // Acceptance Criteria 7: All AST types are Send + Sync
        _assert_send_sync::<SourceFile>();
        _assert_send_sync::<SelectStmt>();
        _assert_send_sync::<InsertStmt>();
        _assert_send_sync::<UpdateStmt>();
        _assert_send_sync::<DeleteStmt>();
        _assert_send_sync::<Expr>();
        _assert_send_sync::<JsonbAccessExpr>();
        _assert_send_sync::<FromClause>();
        _assert_send_sync::<TableRef>();
    }

    #[test]
    fn test_zero_cost_abstraction() {
        // Acceptance Criteria 6: Zero-cost abstraction - AST nodes are just newtype wrappers
        // This is verified at compile time by the struct definitions.
        // Each AST node is a newtype wrapper around SyntaxNode:
        // - SelectStmt(SyntaxNode) - size of SyntaxNode
        // - FromClause(SyntaxNode) - size of SyntaxNode
        // etc.

        use crate::SyntaxNode;
        use std::mem::size_of;

        // All node types should be the same size as SyntaxNode
        assert_eq!(size_of::<SourceFile>(), size_of::<SyntaxNode>());
        assert_eq!(size_of::<SelectStmt>(), size_of::<SyntaxNode>());
        assert_eq!(size_of::<InsertStmt>(), size_of::<SyntaxNode>());
        assert_eq!(size_of::<UpdateStmt>(), size_of::<SyntaxNode>());
        assert_eq!(size_of::<DeleteStmt>(), size_of::<SyntaxNode>());
        assert_eq!(size_of::<FromClause>(), size_of::<SyntaxNode>());
        assert_eq!(size_of::<WhereClause>(), size_of::<SyntaxNode>());
        assert_eq!(size_of::<JsonbAccessExpr>(), size_of::<SyntaxNode>());
    }

    #[test]
    fn test_can_cast_logic() {
        use crate::SyntaxKind;

        // Test can_cast for various types
        assert!(SelectStmt::can_cast(SyntaxKind::SELECT_STMT));
        assert!(!SelectStmt::can_cast(SyntaxKind::INSERT_STMT));

        assert!(InsertStmt::can_cast(SyntaxKind::INSERT_STMT));
        assert!(!InsertStmt::can_cast(SyntaxKind::SELECT_STMT));

        assert!(FromClause::can_cast(SyntaxKind::FROM_CLAUSE));
        assert!(!FromClause::can_cast(SyntaxKind::WHERE_CLAUSE));

        // Stmt enum should match all statement kinds
        assert!(Stmt::can_cast(SyntaxKind::SELECT_STMT));
        assert!(Stmt::can_cast(SyntaxKind::INSERT_STMT));
        assert!(Stmt::can_cast(SyntaxKind::UPDATE_STMT));
        assert!(Stmt::can_cast(SyntaxKind::DELETE_STMT));
        assert!(!Stmt::can_cast(SyntaxKind::FROM_CLAUSE));

        // Expr enum should match expression kinds
        assert!(Expr::can_cast(SyntaxKind::LITERAL));
        assert!(Expr::can_cast(SyntaxKind::BINARY_EXPR));
        assert!(Expr::can_cast(SyntaxKind::FUNC_CALL));
        assert!(Expr::can_cast(SyntaxKind::JSONB_ACCESS_EXPR));
        assert!(!Expr::can_cast(SyntaxKind::SELECT_STMT));

        // JSONB expressions
        assert!(JsonbExpr::can_cast(SyntaxKind::JSONB_ACCESS_EXPR));
        assert!(JsonbExpr::can_cast(SyntaxKind::JSONB_PATH_EXPR));
        assert!(JsonbExpr::can_cast(SyntaxKind::JSONB_CONTAINS_EXPR));
        assert!(JsonbExpr::can_cast(SyntaxKind::JSONB_EXISTS_EXPR));
    }

    #[test]
    fn test_jsonb_access_expr_extracts_text() {
        // This tests the conceptual behavior of the extracts_text method
        // The actual test requires parsed nodes, but we can verify the logic:
        // - ARROW (->) does NOT extract text (returns jsonb)
        // - ARROW_TEXT (->>) DOES extract text (returns text)

        // JsonbAccessOp enum correctly distinguishes
        let arrow = JsonbAccessOp::Arrow;
        let arrow_text = JsonbAccessOp::ArrowText;

        assert_ne!(arrow, arrow_text);
    }

    #[test]
    fn test_order_by_direction() {
        // Test SortDirection default and values
        let default_dir = SortDirection::default();
        assert_eq!(default_dir, SortDirection::Asc);

        assert_ne!(SortDirection::Asc, SortDirection::Desc);
    }

    #[test]
    fn test_join_type_enum() {
        // Verify all join types are distinct
        let types = [
            JoinType::Inner,
            JoinType::Left,
            JoinType::Right,
            JoinType::Full,
            JoinType::Cross,
            JoinType::Natural,
        ];

        for (i, t1) in types.iter().enumerate() {
            for (j, t2) in types.iter().enumerate() {
                if i == j {
                    assert_eq!(t1, t2);
                } else {
                    assert_ne!(t1, t2);
                }
            }
        }
    }

    #[test]
    fn test_join_type_is_outer() {
        // LEFT, RIGHT, FULL are outer joins
        assert!(JoinType::Left.is_outer());
        assert!(JoinType::Right.is_outer());
        assert!(JoinType::Full.is_outer());

        // INNER, CROSS, NATURAL are not outer joins
        assert!(!JoinType::Inner.is_outer());
        assert!(!JoinType::Cross.is_outer());
        assert!(!JoinType::Natural.is_outer());
    }

    #[test]
    fn test_join_type_categories() {
        // Test is_inner
        assert!(JoinType::Inner.is_inner());
        assert!(!JoinType::Left.is_inner());

        // Test is_cross
        assert!(JoinType::Cross.is_cross());
        assert!(!JoinType::Inner.is_cross());

        // Test is_natural
        assert!(JoinType::Natural.is_natural());
        assert!(!JoinType::Inner.is_natural());
    }

    #[test]
    fn test_binary_op_enum() {
        // Verify binary operators are correctly defined
        assert_ne!(BinaryOp::Eq, BinaryOp::Ne);
        assert_ne!(BinaryOp::Add, BinaryOp::Sub);
        assert_ne!(BinaryOp::And, BinaryOp::Or);
        assert_ne!(BinaryOp::Like, BinaryOp::ILike);
    }

    #[test]
    fn test_jsonpath_mode() {
        // Verify JSONPath modes
        assert_ne!(JpModeKind::Strict, JpModeKind::Lax);
    }

    #[test]
    fn test_jsonpath_method_kinds() {
        // Verify all method kinds are distinct
        let kinds = [
            JpMethodKind::Type,
            JpMethodKind::Size,
            JpMethodKind::Double,
            JpMethodKind::Ceiling,
            JpMethodKind::Floor,
            JpMethodKind::Abs,
            JpMethodKind::Keyvalue,
            JpMethodKind::Datetime,
            JpMethodKind::Unknown,
        ];

        for (i, k1) in kinds.iter().enumerate() {
            for (j, k2) in kinds.iter().enumerate() {
                if i == j {
                    assert_eq!(k1, k2);
                } else {
                    assert_ne!(k1, k2);
                }
            }
        }
    }
}
