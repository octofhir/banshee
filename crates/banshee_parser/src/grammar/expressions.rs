use crate::parser::{CompletedMarker, ParseContext, Parser};
use banshee_syntax::SyntaxKind;

use super::select::{at_ident, at_query_start, expect_ident};
use super::{CASE_RECOVERY, EXPR_LIST_RECOVERY, PAREN_RECOVERY, SUBQUERY_RECOVERY};

pub fn expr(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    expr_bp(p, 0)
}

fn expr_bp(p: &mut Parser<'_>, min_bp: u8) -> Option<CompletedMarker> {
    let mut lhs = lhs(p)?;

    loop {
        let op = p.current();
        let (l_bp, r_bp) = match infix_binding_power(op) {
            Some((l, r)) => (l, r),
            None => break,
        };

        if l_bp < min_bp {
            break;
        }

        // Create wrapper marker BEFORE bumping operator, so operator is inside
        let m = lhs.precede(p);

        // Check for JSONPath operators @? and @@
        let is_jsonpath_op = op == SyntaxKind::AT_QUESTION || op == SyntaxKind::AT_AT;

        // Check for comparison operators that might be followed by ANY/ALL/SOME
        let is_comparison = matches!(
            op,
            SyntaxKind::EQ
                | SyntaxKind::NE
                | SyntaxKind::LT
                | SyntaxKind::LE
                | SyntaxKind::GT
                | SyntaxKind::GE
        );

        p.bump(); // operator

        // For JSONPath operators, if the RHS is a string literal, mark it specially
        let has_rhs = if is_jsonpath_op && p.at(SyntaxKind::STRING) {
            let jm = p.start();
            p.bump(); // consume the string
            jm.complete(p, SyntaxKind::JSONPATH_LITERAL);
            true
        } else if is_comparison
            && (p.at(SyntaxKind::ANY_KW) || p.at(SyntaxKind::ALL_KW) || p.at(SyntaxKind::SOME_KW))
        {
            // expr op ANY/ALL/SOME (array_or_subquery)
            let quantifier = p.current();
            p.bump(); // ANY/ALL/SOME
            p.expect(SyntaxKind::L_PAREN);

            // Can be an array expression or subquery
            if at_query_start(p) {
                p.push_context(ParseContext::Subquery);
                parse_subquery_with_recovery(p);
                p.pop_context();
            } else {
                expr(p);
            }
            p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);

            // Complete with appropriate node type
            let node_kind = if quantifier == SyntaxKind::ALL_KW {
                SyntaxKind::ALL_EXPR
            } else {
                SyntaxKind::ANY_EXPR // ANY and SOME are synonyms
            };
            lhs = m.complete(p, node_kind);
            continue; // Skip the normal completion below
        } else {
            expr_bp(p, r_bp).is_some()
        };
        lhs = m.complete(p, SyntaxKind::BINARY_EXPR);

        if !has_rhs {
            break;
        }
    }

    Some(lhs)
}

fn lhs(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    let cm = match p.current() {
        // Prefix operators
        SyntaxKind::NOT_KW => {
            let m = p.start();
            p.bump();
            // NOT binds looser than comparison but tighter than AND/OR, so
            // `NOT a = b` parses as `NOT (a = b)`.
            expr_bp(p, NOT_BP);
            m.complete(p, SyntaxKind::UNARY_EXPR)
        }
        SyntaxKind::MINUS | SyntaxKind::PLUS => {
            let m = p.start();
            p.bump();
            expr_bp(p, PREFIX_BP);
            m.complete(p, SyntaxKind::UNARY_EXPR)
        }

        // Parenthesized expression or subquery
        SyntaxKind::L_PAREN => paren_expr(p),

        // CASE expression
        SyntaxKind::CASE_KW => case_expr(p),

        // CAST expression
        SyntaxKind::CAST_KW => cast_expr(p),

        // EXISTS subquery
        SyntaxKind::EXISTS_KW => exists_expr(p),

        // Special syntax functions
        SyntaxKind::COALESCE_KW => coalesce_expr(p),
        SyntaxKind::NULLIF_KW => nullif_expr(p),
        SyntaxKind::GREATEST_KW | SyntaxKind::LEAST_KW => greatest_least_expr(p),
        SyntaxKind::EXTRACT_KW => extract_expr(p),
        SyntaxKind::POSITION_KW => position_expr(p),
        SyntaxKind::SUBSTRING_KW => substring_expr(p),
        SyntaxKind::TRIM_KW => trim_expr(p),
        SyntaxKind::OVERLAY_KW => overlay_expr(p),

        // Niladic special functions: spelled without parentheses (with an
        // optional precision for the time-valued ones), e.g. CURRENT_TIMESTAMP.
        SyntaxKind::CURRENT_DATE_KW
        | SyntaxKind::CURRENT_TIME_KW
        | SyntaxKind::CURRENT_TIMESTAMP_KW
        | SyntaxKind::CURRENT_USER_KW
        | SyntaxKind::SESSION_USER_KW
        | SyntaxKind::LOCALTIME_KW
        | SyntaxKind::LOCALTIMESTAMP_KW => niladic_function_expr(p),

        // Array constructor
        SyntaxKind::ARRAY_KW => array_expr(p),

        // Literals
        SyntaxKind::INTEGER
        | SyntaxKind::FLOAT
        | SyntaxKind::STRING
        | SyntaxKind::DOLLAR_STRING
        | SyntaxKind::BIT_STRING
        | SyntaxKind::HEX_STRING
        | SyntaxKind::TRUE_KW
        | SyntaxKind::FALSE_KW
        | SyntaxKind::NULL_KW => {
            let m = p.start();
            p.bump();
            m.complete(p, SyntaxKind::LITERAL)
        }

        // Parameter
        SyntaxKind::PARAM => {
            let m = p.start();
            p.bump();
            m.complete(p, SyntaxKind::PARAM_REF)
        }

        // Identifier (column ref or function call)
        // Also handle aggregate/function keywords that can be used as function names
        SyntaxKind::IDENT
        | SyntaxKind::QUOTED_IDENT
        | SyntaxKind::COUNT_KW
        | SyntaxKind::SUM_KW
        | SyntaxKind::AVG_KW
        | SyntaxKind::MIN_KW
        | SyntaxKind::MAX_KW
        // Unreserved keywords that are also ordinary built-in functions, so
        // they may head a function call (e.g. replace()).
        | SyntaxKind::REPLACE_KW => name_ref(p),

        // Star (for SELECT *)
        SyntaxKind::STAR => {
            let m = p.start();
            p.bump();
            m.complete(p, SyntaxKind::STAR_EXPR)
        }

        // Any non-reserved keyword can stand in for a column or function name
        // (PostgreSQL's ColId), e.g. `type`, `value`, `source`.
        kind if kind.can_be_identifier() => name_ref(p),

        _ => return None,
    };

    Some(postfix(p, cm))
}

fn postfix(p: &mut Parser<'_>, mut lhs: CompletedMarker) -> CompletedMarker {
    loop {
        lhs = match p.current() {
            // Type cast: expr::type
            SyntaxKind::DOUBLE_COLON => {
                let m = lhs.precede(p);
                p.bump();
                type_name(p, "after ::");
                m.complete(p, SyntaxKind::CAST_EXPR)
            }

            // Array subscript or slice: expr[index] / expr[lo:hi] / expr[:hi] / expr[lo:]
            SyntaxKind::L_BRACKET => {
                let m = lhs.precede(p);
                p.bump();
                // Lower bound (may be omitted in a slice)
                if !p.at(SyntaxKind::COLON) && !p.at(SyntaxKind::R_BRACKET) {
                    expr(p);
                }
                let is_slice = p.eat(SyntaxKind::COLON);
                if is_slice {
                    // Upper bound (may be omitted)
                    if !p.at(SyntaxKind::R_BRACKET) {
                        expr(p);
                    }
                }
                p.expect(SyntaxKind::R_BRACKET);
                let kind = if is_slice {
                    SyntaxKind::ARRAY_SLICE_EXPR
                } else {
                    SyntaxKind::JSONB_ACCESS_EXPR
                };
                m.complete(p, kind)
            }

            // OVER clause for window functions
            SyntaxKind::OVER_KW => {
                let m = lhs.precede(p);
                over_clause(p);
                m.complete(p, SyntaxKind::FUNC_CALL)
            }

            // FILTER clause for aggregates
            SyntaxKind::FILTER_KW => {
                let m = lhs.precede(p);
                filter_clause(p);
                m.complete(p, SyntaxKind::FUNC_CALL)
            }

            // IS NULL / IS NOT NULL / IS TRUE / etc.
            SyntaxKind::IS_KW => {
                let m = lhs.precede(p);
                p.bump();
                p.eat(SyntaxKind::NOT_KW);
                match p.current() {
                    SyntaxKind::NULL_KW
                    | SyntaxKind::TRUE_KW
                    | SyntaxKind::FALSE_KW
                    | SyntaxKind::UNKNOWN_KW => p.bump(),
                    SyntaxKind::DISTINCT_KW => {
                        p.bump();
                        p.expect(SyntaxKind::FROM_KW);
                        expr(p);
                    }
                    _ => p.error("expected NULL, TRUE, FALSE, UNKNOWN, or DISTINCT after IS"),
                }
                m.complete(p, SyntaxKind::IS_EXPR)
            }

            // IN (list) / IN (subquery)
            SyntaxKind::IN_KW => {
                let m = lhs.precede(p);
                p.bump();
                p.expect(SyntaxKind::L_PAREN);
                if at_query_start(p) {
                    // subquery with isolated recovery
                    p.push_context(ParseContext::Subquery);
                    parse_subquery_with_recovery(p);
                    p.pop_context();
                } else {
                    // expression list
                    expr(p);
                    while p.eat(SyntaxKind::COMMA) {
                        expr(p);
                    }
                }
                p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
                m.complete(p, SyntaxKind::IN_EXPR)
            }

            // NOT IN
            SyntaxKind::NOT_KW if p.nth(1) == SyntaxKind::IN_KW => {
                let m = lhs.precede(p);
                p.bump(); // NOT
                p.bump(); // IN
                p.expect(SyntaxKind::L_PAREN);
                if at_query_start(p) {
                    // subquery with isolated recovery
                    p.push_context(ParseContext::Subquery);
                    parse_subquery_with_recovery(p);
                    p.pop_context();
                } else {
                    expr(p);
                    while p.eat(SyntaxKind::COMMA) {
                        expr(p);
                    }
                }
                p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
                m.complete(p, SyntaxKind::IN_EXPR)
            }

            // BETWEEN expr AND expr
            SyntaxKind::BETWEEN_KW => {
                let m = lhs.precede(p);
                p.bump();
                expr_bp(p, BETWEEN_BP);
                p.expect(SyntaxKind::AND_KW);
                expr_bp(p, BETWEEN_BP);
                m.complete(p, SyntaxKind::BETWEEN_EXPR)
            }

            // NOT BETWEEN
            SyntaxKind::NOT_KW if p.nth(1) == SyntaxKind::BETWEEN_KW => {
                let m = lhs.precede(p);
                p.bump(); // NOT
                p.bump(); // BETWEEN
                expr_bp(p, BETWEEN_BP);
                p.expect(SyntaxKind::AND_KW);
                expr_bp(p, BETWEEN_BP);
                m.complete(p, SyntaxKind::BETWEEN_EXPR)
            }

            // LIKE / ILIKE
            SyntaxKind::LIKE_KW | SyntaxKind::ILIKE_KW => {
                let m = lhs.precede(p);
                p.bump();
                expr_bp(p, LIKE_BP);
                if p.eat(SyntaxKind::ESCAPE_KW) {
                    expr_bp(p, LIKE_BP);
                }
                m.complete(p, SyntaxKind::LIKE_EXPR)
            }

            // NOT LIKE / NOT ILIKE
            SyntaxKind::NOT_KW
                if p.nth(1) == SyntaxKind::LIKE_KW || p.nth(1) == SyntaxKind::ILIKE_KW =>
            {
                let m = lhs.precede(p);
                p.bump(); // NOT
                p.bump(); // LIKE/ILIKE
                expr_bp(p, LIKE_BP);
                if p.eat(SyntaxKind::ESCAPE_KW) {
                    expr_bp(p, LIKE_BP);
                }
                m.complete(p, SyntaxKind::LIKE_EXPR)
            }

            // SIMILAR TO / NOT SIMILAR TO
            SyntaxKind::SIMILAR_KW if p.nth(1) == SyntaxKind::TO_KW => {
                let m = lhs.precede(p);
                p.bump(); // SIMILAR
                p.bump(); // TO
                expr_bp(p, LIKE_BP);
                if p.eat(SyntaxKind::ESCAPE_KW) {
                    expr_bp(p, LIKE_BP);
                }
                m.complete(p, SyntaxKind::LIKE_EXPR)
            }
            SyntaxKind::NOT_KW
                if p.nth(1) == SyntaxKind::SIMILAR_KW && p.nth(2) == SyntaxKind::TO_KW =>
            {
                let m = lhs.precede(p);
                p.bump(); // NOT
                p.bump(); // SIMILAR
                p.bump(); // TO
                expr_bp(p, LIKE_BP);
                if p.eat(SyntaxKind::ESCAPE_KW) {
                    expr_bp(p, LIKE_BP);
                }
                m.complete(p, SyntaxKind::LIKE_EXPR)
            }

            // expr AT TIME ZONE zone
            SyntaxKind::AT_KW
                if p.nth(1) == SyntaxKind::TIME_KW && p.nth(2) == SyntaxKind::ZONE_KW =>
            {
                let m = lhs.precede(p);
                p.bump(); // AT
                p.bump(); // TIME
                p.bump(); // ZONE
                expr_bp(p, BETWEEN_BP);
                m.complete(p, SyntaxKind::AT_TIME_ZONE_EXPR)
            }

            // expr COLLATE collation
            SyntaxKind::COLLATE_KW => {
                let m = lhs.precede(p);
                p.bump(); // COLLATE
                // Collation name: ident, "quoted", or schema-qualified
                expect_ident(p, EXPR_LIST_RECOVERY);
                if p.at(SyntaxKind::DOT) {
                    p.bump();
                    expect_ident(p, EXPR_LIST_RECOVERY);
                }
                m.complete(p, SyntaxKind::COLLATE_EXPR)
            }

            // Postfix null tests: expr ISNULL / expr NOTNULL
            SyntaxKind::ISNULL_KW | SyntaxKind::NOTNULL_KW => {
                let m = lhs.precede(p);
                p.bump();
                m.complete(p, SyntaxKind::IS_EXPR)
            }

            _ => return lhs,
        };
    }
}

fn name_ref(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.start();
    // A non-reserved keyword used as a column/function name is recorded as an
    // IDENT; real identifiers (and quoted identifiers) keep their kind. The
    // aggregate keywords keep their kind so name resolution still recognises
    // them as aggregates.
    let kw = p.current();
    let is_aggregate = matches!(
        kw,
        SyntaxKind::COUNT_KW
            | SyntaxKind::SUM_KW
            | SyntaxKind::AVG_KW
            | SyntaxKind::MIN_KW
            | SyntaxKind::MAX_KW
    );
    if kw.is_keyword() && !is_aggregate {
        p.bump_remap(SyntaxKind::IDENT);
    } else {
        p.bump(); // IDENT, QUOTED_IDENT, or an aggregate keyword
    }

    // Check for qualified name or function call
    if p.at(SyntaxKind::DOT) {
        // schema.table or table.column
        p.bump();
        if p.at(SyntaxKind::STAR) {
            // table.*
            p.bump();
            return m.complete(p, SyntaxKind::STAR_EXPR);
        }
        expect_ident(p, EXPR_LIST_RECOVERY);

        // Could be schema.func() or schema.table.column
        if p.at(SyntaxKind::DOT) {
            p.bump();
            if p.at(SyntaxKind::STAR) {
                p.bump();
                return m.complete(p, SyntaxKind::STAR_EXPR);
            }
            expect_ident(p, EXPR_LIST_RECOVERY);
        }

        if p.at(SyntaxKind::L_PAREN) {
            return func_call_args(p, m);
        }

        return m.complete(p, SyntaxKind::COLUMN_REF);
    }

    // Function call
    if p.at(SyntaxKind::L_PAREN) {
        return func_call_args(p, m);
    }

    m.complete(p, SyntaxKind::COLUMN_REF)
}

fn func_call_args(p: &mut Parser<'_>, m: crate::parser::Marker) -> CompletedMarker {
    p.bump(); // (

    if !p.at(SyntaxKind::R_PAREN) {
        // Check for DISTINCT
        p.eat(SyntaxKind::DISTINCT_KW);
        p.eat(SyntaxKind::ALL_KW);

        // Special case: count(*)
        if p.at(SyntaxKind::STAR) {
            p.bump();
        } else {
            // Arguments
            func_arg(p);
            while p.eat(SyntaxKind::COMMA) {
                func_arg(p);
            }
        }

        // ORDER BY within aggregate
        if p.at(SyntaxKind::ORDER_KW) {
            order_by_in_aggregate(p);
        }
    }

    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);

    // WITHIN GROUP clause
    if p.at(SyntaxKind::WITHIN_KW) {
        p.bump();
        p.expect(SyntaxKind::GROUP_KW);
        p.expect(SyntaxKind::L_PAREN);
        p.expect(SyntaxKind::ORDER_KW);
        p.expect(SyntaxKind::BY_KW);
        order_by_item(p);
        while p.eat(SyntaxKind::COMMA) {
            order_by_item(p);
        }
        p.expect(SyntaxKind::R_PAREN);
    }

    m.complete(p, SyntaxKind::FUNC_CALL)
}

/// A function-call argument, optionally named (`name => value`) or VARIADIC.
fn func_arg(p: &mut Parser<'_>) {
    p.eat(SyntaxKind::VARIADIC_KW);

    if at_ident(p) && p.nth(1) == SyntaxKind::FAT_ARROW {
        let m = p.start();
        p.bump(); // name
        p.bump(); // =>
        expr(p);
        m.complete(p, SyntaxKind::NAMED_ARG);
    } else {
        expr(p);
    }
}

fn order_by_in_aggregate(p: &mut Parser<'_>) {
    p.expect(SyntaxKind::ORDER_KW);
    p.expect(SyntaxKind::BY_KW);
    order_by_item(p);
    while p.eat(SyntaxKind::COMMA) {
        order_by_item(p);
    }
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

fn over_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // OVER

    if at_ident(p) {
        // Named window reference
        p.bump();
    } else {
        p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);

        // PARTITION BY
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

        // ORDER BY
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

        // Frame clause
        if p.at(SyntaxKind::ROWS_KW) || p.at(SyntaxKind::RANGE_KW) || p.at(SyntaxKind::GROUPS_KW) {
            frame_clause(p);
        }

        p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    }

    m.complete(p, SyntaxKind::OVER_CLAUSE);
}

pub(super) fn frame_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // ROWS/RANGE/GROUPS

    if p.eat(SyntaxKind::BETWEEN_KW) {
        frame_bound(p);
        p.expect(SyntaxKind::AND_KW);
        frame_bound(p);
    } else {
        frame_bound(p);
    }

    // Optional EXCLUDE clause (PostgreSQL 11+)
    if p.at(SyntaxKind::EXCLUDE_KW) {
        frame_exclusion(p);
    }

    m.complete(p, SyntaxKind::FRAME_CLAUSE);
}

/// Parse window frame exclusion: EXCLUDE { CURRENT ROW | GROUP | TIES | NO OTHERS }
fn frame_exclusion(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // EXCLUDE

    if p.at(SyntaxKind::CURRENT_KW) {
        p.bump();
        p.expect(SyntaxKind::ROW_KW);
    } else if p.eat(SyntaxKind::GROUP_KW) {
        // EXCLUDE GROUP
    } else if p.eat(SyntaxKind::TIES_KW) {
        // EXCLUDE TIES
    } else if p.at(SyntaxKind::NO_KW) {
        p.bump();
        p.expect(SyntaxKind::OTHERS_KW);
    } else {
        p.error("expected CURRENT ROW, GROUP, TIES, or NO OTHERS after EXCLUDE");
    }

    m.complete(p, SyntaxKind::FRAME_EXCLUSION);
}

fn frame_bound(p: &mut Parser<'_>) {
    let m = p.start();

    if p.eat(SyntaxKind::UNBOUNDED_KW) {
        let _ = p.eat(SyntaxKind::PRECEDING_KW) || p.eat(SyntaxKind::FOLLOWING_KW);
    } else if p.eat(SyntaxKind::CURRENT_KW) {
        p.expect(SyntaxKind::ROW_KW);
    } else {
        expr(p);
        let _ = p.eat(SyntaxKind::PRECEDING_KW) || p.eat(SyntaxKind::FOLLOWING_KW);
    }

    m.complete(p, SyntaxKind::FRAME_BOUND);
}

fn filter_clause(p: &mut Parser<'_>) {
    let m = p.start();
    p.bump(); // FILTER
    p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
    p.expect(SyntaxKind::WHERE_KW);
    expr(p);
    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    m.complete(p, SyntaxKind::FILTER_CLAUSE);
}

fn paren_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.start();
    p.bump(); // (

    // Check for subquery
    if at_query_start(p) {
        p.push_context(ParseContext::Subquery);
        parse_subquery_with_recovery(p);
        p.pop_context();
        p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
        return m.complete(p, SyntaxKind::SUBQUERY_EXPR);
    }

    // Regular parenthesized expression or row constructor
    p.push_context(ParseContext::ParenExpression);
    expr(p);

    if p.at(SyntaxKind::COMMA) {
        // Row constructor: (a, b, c)
        while p.eat(SyntaxKind::COMMA) {
            expr(p);
        }
        p.pop_context();
        p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
        return m.complete(p, SyntaxKind::ROW_EXPR);
    }

    p.pop_context();
    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    m.complete(p, SyntaxKind::PAREN_EXPR)
}

/// Parse a subquery with isolated error recovery.
/// Errors within the subquery are contained and don't leak to outer context.
fn parse_subquery_with_recovery(p: &mut Parser<'_>) {
    // Parse the subquery, but if we hit an error, recover to SUBQUERY_RECOVERY tokens
    // This ensures errors inside the subquery don't escape to outer queries
    super::select::select_stmt(p);

    // If we're not at R_PAREN after parsing, recover within subquery bounds
    if !p.at(SyntaxKind::R_PAREN) && !p.at_end() {
        // Skip tokens until we find a subquery boundary
        if p.at_set(SUBQUERY_RECOVERY) {
            // Already at a recovery point
        } else {
            let em = p.start();
            p.error_with_context("unexpected tokens in subquery");
            while !p.at_end() && !p.at_set(SUBQUERY_RECOVERY) && !p.at(SyntaxKind::R_PAREN) {
                p.bump_any();
            }
            em.complete(p, SyntaxKind::ERROR);
        }
    }
}

fn case_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.start();
    p.bump(); // CASE
    p.push_context(ParseContext::CaseExpression);

    // Simple CASE: CASE expr WHEN ...
    if !p.at(SyntaxKind::WHEN_KW) && !p.at(SyntaxKind::END_KW) {
        expr(p);
    }

    // Parse WHEN clauses with recovery
    while p.at(SyntaxKind::WHEN_KW) {
        let wm = p.start();
        p.bump(); // WHEN
        expr(p);
        p.expect_recover(SyntaxKind::THEN_KW, CASE_RECOVERY);
        if !p.at_set(CASE_RECOVERY) || p.at(SyntaxKind::WHEN_KW) || p.at(SyntaxKind::ELSE_KW) {
            expr(p);
        }
        wm.complete(p, SyntaxKind::CASE_WHEN);
    }

    // Parse optional ELSE clause
    if p.eat(SyntaxKind::ELSE_KW) {
        let em = p.start();
        expr(p);
        em.complete(p, SyntaxKind::CASE_ELSE);
    }

    // Expect END with recovery (use context-aware error)
    if !p.eat(SyntaxKind::END_KW) {
        p.error_with_context("expected END");
        // Try to recover
        if !p.at_set(CASE_RECOVERY) && !p.at_end() {
            let err = p.start();
            while !p.at_end() && !p.at(SyntaxKind::END_KW) && !p.at_set(CASE_RECOVERY) {
                p.bump_any();
            }
            err.complete(p, SyntaxKind::ERROR);
            p.eat(SyntaxKind::END_KW);
        }
    }

    p.pop_context();
    m.complete(p, SyntaxKind::CASE_EXPR)
}

fn cast_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.start();
    p.bump(); // CAST
    p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
    expr(p);
    p.expect(SyntaxKind::AS_KW);
    type_name(p, "after AS");
    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    m.complete(p, SyntaxKind::CAST_EXPR)
}

fn exists_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.start();
    p.bump(); // EXISTS
    p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
    p.push_context(ParseContext::Subquery);
    parse_subquery_with_recovery(p);
    p.pop_context();
    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    m.complete(p, SyntaxKind::EXISTS_EXPR)
}

fn coalesce_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.start();
    p.bump(); // COALESCE
    p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
    expr(p);
    while p.eat(SyntaxKind::COMMA) {
        expr(p);
    }
    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    m.complete(p, SyntaxKind::COALESCE_EXPR)
}

fn nullif_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.start();
    p.bump(); // NULLIF
    p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
    expr(p);
    p.expect(SyntaxKind::COMMA);
    expr(p);
    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    m.complete(p, SyntaxKind::NULLIF_EXPR)
}

fn greatest_least_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.start();
    let kind = if p.at(SyntaxKind::GREATEST_KW) {
        SyntaxKind::GREATEST_EXPR
    } else {
        SyntaxKind::LEAST_EXPR
    };
    p.bump();
    p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
    expr(p);
    while p.eat(SyntaxKind::COMMA) {
        expr(p);
    }
    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    m.complete(p, kind)
}

fn extract_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.start();
    p.bump(); // EXTRACT
    p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
    // Field name (YEAR, MONTH, DAY, etc.)
    p.bump_any();
    p.expect(SyntaxKind::FROM_KW);
    expr(p);
    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    m.complete(p, SyntaxKind::FUNC_CALL)
}

fn position_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.start();
    p.bump(); // POSITION
    p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
    expr(p);
    p.expect(SyntaxKind::IN_KW);
    expr(p);
    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    m.complete(p, SyntaxKind::FUNC_CALL)
}

fn substring_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.start();
    p.bump(); // SUBSTRING
    p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
    expr(p);
    if p.eat(SyntaxKind::FROM_KW) {
        expr(p);
        if p.eat(SyntaxKind::FOR_KW) {
            expr(p);
        }
    } else if p.eat(SyntaxKind::COMMA) {
        expr(p);
        if p.eat(SyntaxKind::COMMA) {
            expr(p);
        }
    }
    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    m.complete(p, SyntaxKind::FUNC_CALL)
}

fn trim_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.start();
    p.bump(); // TRIM
    p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
    // Optional LEADING/TRAILING/BOTH
    let _ = p.eat(SyntaxKind::LEADING_KW)
        || p.eat(SyntaxKind::TRAILING_KW)
        || p.eat(SyntaxKind::BOTH_KW);
    // Optional characters to trim
    if !p.at(SyntaxKind::FROM_KW) {
        expr(p);
    }
    if p.eat(SyntaxKind::FROM_KW) {
        expr(p);
    }
    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    m.complete(p, SyntaxKind::FUNC_CALL)
}

/// SQL niladic special functions, spelled without parentheses:
/// `CURRENT_DATE`, `CURRENT_USER`, `SESSION_USER`, etc. The time-valued forms
/// (`CURRENT_TIME`, `CURRENT_TIMESTAMP`, `LOCALTIME`, `LOCALTIMESTAMP`) accept an
/// optional precision, e.g. `CURRENT_TIMESTAMP(3)`.
fn niladic_function_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.start();
    let kw = p.current();
    p.bump(); // the keyword

    let takes_precision = matches!(
        kw,
        SyntaxKind::CURRENT_TIME_KW
            | SyntaxKind::CURRENT_TIMESTAMP_KW
            | SyntaxKind::LOCALTIME_KW
            | SyntaxKind::LOCALTIMESTAMP_KW
    );
    if takes_precision && p.at(SyntaxKind::L_PAREN) {
        p.bump(); // (
        p.expect(SyntaxKind::INTEGER);
        p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    }

    m.complete(p, SyntaxKind::FUNC_CALL)
}

fn overlay_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.start();
    p.bump(); // OVERLAY
    p.expect_recover(SyntaxKind::L_PAREN, PAREN_RECOVERY);
    expr(p);
    p.expect(SyntaxKind::PLACING_KW);
    expr(p);
    p.expect(SyntaxKind::FROM_KW);
    expr(p);
    if p.eat(SyntaxKind::FOR_KW) {
        expr(p);
    }
    p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    m.complete(p, SyntaxKind::FUNC_CALL)
}

fn array_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.start();
    p.bump(); // ARRAY

    if p.at(SyntaxKind::L_BRACKET) {
        // ARRAY[elem, ...]
        p.bump();
        if !p.at(SyntaxKind::R_BRACKET) {
            expr(p);
            while p.eat(SyntaxKind::COMMA) {
                expr(p);
            }
        }
        p.expect_recover(SyntaxKind::R_BRACKET, EXPR_LIST_RECOVERY);
    } else if p.at(SyntaxKind::L_PAREN) {
        // ARRAY(subquery)
        p.bump();
        p.push_context(ParseContext::Subquery);
        parse_subquery_with_recovery(p);
        p.pop_context();
        p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    }

    m.complete(p, SyntaxKind::ARRAY_EXPR)
}

pub(super) fn type_name(p: &mut Parser<'_>, context: &str) {
    let m = p.start();

    // Handle simple types and schema-qualified types
    let first = p.current();
    match p.current() {
        SyntaxKind::IDENT
        | SyntaxKind::QUOTED_IDENT
        | SyntaxKind::INTEGER_KW
        | SyntaxKind::INT_KW
        | SyntaxKind::SMALLINT_KW
        | SyntaxKind::BIGINT_KW
        | SyntaxKind::REAL_KW
        | SyntaxKind::FLOAT_KW
        | SyntaxKind::DOUBLE_KW
        | SyntaxKind::DECIMAL_KW
        | SyntaxKind::NUMERIC_KW
        | SyntaxKind::BOOLEAN_KW
        | SyntaxKind::TEXT_KW
        | SyntaxKind::VARCHAR_KW
        | SyntaxKind::CHAR_KW
        | SyntaxKind::CHARACTER_KW
        | SyntaxKind::BIT_KW
        | SyntaxKind::DATE_KW
        | SyntaxKind::TIME_KW
        | SyntaxKind::TIMESTAMP_KW
        | SyntaxKind::TIMESTAMPTZ_KW
        | SyntaxKind::INTERVAL_KW
        | SyntaxKind::JSON_KW
        | SyntaxKind::JSONB_KW
        | SyntaxKind::SETOF_KW => {
            p.bump();
        }
        _ => {
            p.error(format!("expected type name {context}"));
        }
    }

    // Multi-word type names without a trailing modifier:
    //   DOUBLE PRECISION, CHARACTER/CHAR/BIT VARYING
    match first {
        SyntaxKind::DOUBLE_KW => {
            p.eat(SyntaxKind::PRECISION_KW);
        }
        SyntaxKind::CHARACTER_KW | SyntaxKind::CHAR_KW | SyntaxKind::BIT_KW => {
            p.eat(SyntaxKind::VARYING_KW);
        }
        _ => {}
    }

    // Schema qualification
    if p.at(SyntaxKind::DOT) {
        p.bump();
        p.bump_any(); // type name
    }

    // Type modifiers: (precision, scale)
    if p.at(SyntaxKind::L_PAREN) {
        p.bump();
        expr(p);
        if p.eat(SyntaxKind::COMMA) {
            expr(p);
        }
        p.expect_recover(SyntaxKind::R_PAREN, PAREN_RECOVERY);
    }

    // Trailing WITH/WITHOUT TIME ZONE for TIME / TIMESTAMP (may follow a
    // precision modifier, e.g. `timestamp(3) with time zone`).
    if matches!(first, SyntaxKind::TIME_KW | SyntaxKind::TIMESTAMP_KW)
        && (p.at(SyntaxKind::WITH_KW) || p.at(SyntaxKind::WITHOUT_KW))
    {
        p.bump(); // WITH / WITHOUT
        p.expect(SyntaxKind::TIME_KW);
        p.expect(SyntaxKind::ZONE_KW);
    }

    // Array type: type[]
    while p.at(SyntaxKind::L_BRACKET) {
        p.bump();
        // Optional array size
        if p.at(SyntaxKind::INTEGER) {
            p.bump();
        }
        p.expect_recover(SyntaxKind::R_BRACKET, EXPR_LIST_RECOVERY);
    }

    m.complete(p, SyntaxKind::TYPE_NAME);
}

// Binding power constants
const PREFIX_BP: u8 = 15;
const BETWEEN_BP: u8 = 7;
const LIKE_BP: u8 = 7;
/// Operand binding power for prefix `NOT`: above AND (4) so it captures a full
/// comparison, below comparison (6) so `NOT a = b` is `NOT (a = b)`.
const NOT_BP: u8 = 5;

fn infix_binding_power(op: SyntaxKind) -> Option<(u8, u8)> {
    let bp = match op {
        // Cast (highest, left-associative handled via postfix)
        // SyntaxKind::DOUBLE_COLON => (14, 15), // Handled in postfix

        // Exponentiation: binds tighter than unary +/- (PREFIX_BP), left-associative.
        // PostgreSQL: `-2 ^ 2` evaluates as `-(2 ^ 2)`.
        SyntaxKind::CARET => (21, 22),

        // JSONB access operators (right-associative for chaining)
        SyntaxKind::ARROW | SyntaxKind::ARROW_TEXT => (12, 11),
        SyntaxKind::HASH_ARROW | SyntaxKind::HASH_ARROW_TEXT => (12, 11),

        // Multiplicative
        SyntaxKind::STAR | SyntaxKind::SLASH | SyntaxKind::PERCENT => (10, 11),

        // String concatenation
        SyntaxKind::PIPE_PIPE => (10, 11),

        // Additive
        SyntaxKind::PLUS | SyntaxKind::MINUS => (8, 9),

        // Bitwise and shift operators (below additive, above comparison).
        SyntaxKind::AMP
        | SyntaxKind::PIPE
        | SyntaxKind::HASH
        | SyntaxKind::SHL
        | SyntaxKind::SHR => (7, 8),

        // JSONB containment and existence
        SyntaxKind::AT_GT | SyntaxKind::LT_AT => (6, 7),
        SyntaxKind::QUESTION | SyntaxKind::QUESTION_PIPE | SyntaxKind::QUESTION_AMP => (6, 7),
        SyntaxKind::HASH_MINUS => (6, 7),
        SyntaxKind::AT_QUESTION | SyntaxKind::AT_AT => (6, 7),

        // Comparison
        SyntaxKind::EQ
        | SyntaxKind::NE
        | SyntaxKind::LT
        | SyntaxKind::LE
        | SyntaxKind::GT
        | SyntaxKind::GE => (6, 7),

        // Pattern-match operators (regex). Same level as comparison.
        SyntaxKind::TILDE
        | SyntaxKind::TILDE_STAR
        | SyntaxKind::BANG_TILDE
        | SyntaxKind::BANG_TILDE_STAR => (6, 7),

        // Logical AND
        SyntaxKind::AND_KW => (4, 5),

        // Logical OR
        SyntaxKind::OR_KW => (2, 3),

        _ => return None,
    };
    Some(bp)
}
