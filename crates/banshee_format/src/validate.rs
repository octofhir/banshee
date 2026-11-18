//! Formatter validation to ensure formatting preserves query semantics.
//!
//! These checks use banshee's parser and compare the original and formatted SQL
//! token streams, including comments, punctuation and significant boundaries.
//!
//! # Example
//!
//! ```ignore
//! use banshee_format::validate::{validate_format, format_validated};
//!
//! // Validate formatting
//! let original = "SELECT id FROM users";
//! let formatted = banshee_format::format_sqlstyle(original);
//! assert!(validate_format(original, &formatted).is_ok());
//!
//! // Format with validation in one step
//! let result = format_validated(original, &FormatConfig::sqlstyle());
//! assert!(result.is_valid);
//! ```

use banshee_syntax::{Parse, ParseError};

use crate::config::FormatConfig;

/// Errors that can occur during format validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// Original SQL was invalid (had parse errors).
    InvalidInput(Vec<ParseError>),

    /// Formatter produced SQL that doesn't parse correctly.
    FormatterBrokeQuery(Vec<ParseError>),

    /// Formatted SQL has different semantic content than original.
    SemanticMismatch {
        /// Normalized representation of original SQL.
        expected: String,
        /// Normalized representation of formatted SQL.
        actual: String,
    },
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::InvalidInput(errors) => {
                write!(f, "invalid input SQL: ")?;
                for (i, e) in errors.iter().enumerate() {
                    if i > 0 {
                        write!(f, "; ")?;
                    }
                    write!(f, "{}", e)?;
                }
                Ok(())
            }
            FormatError::FormatterBrokeQuery(errors) => {
                write!(f, "formatter produced invalid SQL: ")?;
                for (i, e) in errors.iter().enumerate() {
                    if i > 0 {
                        write!(f, "; ")?;
                    }
                    write!(f, "{}", e)?;
                }
                Ok(())
            }
            FormatError::SemanticMismatch { expected, actual } => {
                write!(
                    f,
                    "semantic mismatch: expected '{}', got '{}'",
                    expected, actual
                )
            }
        }
    }
}

impl std::error::Error for FormatError {}

/// Result of a format operation with validation.
#[derive(Debug, Clone)]
pub struct FormatResult {
    /// The formatted output.
    pub output: String,

    /// Whether the formatting passed validation.
    pub is_valid: bool,

    /// Any validation errors encountered.
    pub errors: Vec<FormatError>,
}

impl FormatResult {
    /// Creates a successful format result.
    #[must_use]
    pub fn ok(output: String) -> Self {
        Self {
            output,
            is_valid: true,
            errors: vec![],
        }
    }

    /// Creates a format result with errors.
    #[must_use]
    pub fn with_error(output: String, error: FormatError) -> Self {
        Self {
            output,
            is_valid: false,
            errors: vec![error],
        }
    }

    /// Returns `Ok` with the output if valid, `Err` with errors otherwise.
    pub fn into_result(self) -> Result<String, Vec<FormatError>> {
        if self.is_valid {
            Ok(self.output)
        } else {
            Err(self.errors)
        }
    }
}

/// Validates that formatting preserves query semantics.
///
/// This function:
/// 1. Parses the original SQL
/// 2. Parses the formatted SQL
/// 3. Checks that formatting preserved SQL tokens and significant boundaries
///
/// # Errors
///
/// Returns an error if:
/// - The original SQL is invalid (`InvalidInput`)
/// - The formatted SQL is invalid (`FormatterBrokeQuery`)
/// - The semantic content differs (`SemanticMismatch`)
///
/// # Example
///
/// ```ignore
/// use banshee_format::validate::validate_format;
///
/// let original = "SELECT id FROM users";
/// let formatted = "SELECT\n    id\nFROM users";
/// assert!(validate_format(original, formatted).is_ok());
/// ```
pub fn validate_format(original: &str, formatted: &str) -> Result<(), FormatError> {
    // Parse original
    let orig_parse = banshee_parser::parse(original);
    validate_input(&orig_parse)?;

    // Parse formatted output
    let fmt_parse = banshee_parser::parse(formatted);
    if !fmt_parse.errors().is_empty() {
        return Err(FormatError::FormatterBrokeQuery(
            fmt_parse.errors().to_vec(),
        ));
    }

    if !crate::safe::preserves_sql(original, formatted) {
        return Err(FormatError::SemanticMismatch {
            expected: normalize(&orig_parse),
            actual: normalize(&fmt_parse),
        });
    }

    Ok(())
}

/// Rejects input with parse errors or a `COPY FROM STDIN` command whose
/// following bytes may be raw data.
pub fn validate_input(parse: &Parse) -> Result<(), FormatError> {
    crate::safe::check_input(parse, &parse.syntax())
}

/// Compares the source tokens of two parse trees, including comments and
/// significant whitespace.
#[must_use]
pub fn semantically_equal(a: &Parse, b: &Parse) -> bool {
    let a = crate::safe::source_text(&a.syntax());
    let b = crate::safe::source_text(&b.syntax());
    crate::safe::preserves_sql(&a, &b)
}

/// Normalizes a parse tree to a canonical string representation.
///
/// This extracts all non-trivia tokens and joins them with spaces,
/// converting keywords to uppercase. Use `validate_format` to verify output.
#[must_use]
pub fn normalize(parse: &Parse) -> String {
    let mut tokens = Vec::new();
    for element in parse.syntax().descendants_with_tokens() {
        if let cstree::util::NodeOrToken::Token(token) = element
            && !token.kind().is_trivia()
        {
            // Normalize case for keywords, preserve case for identifiers/strings
            let text = if token.kind().is_keyword() {
                token.text().to_ascii_uppercase()
            } else {
                token.text().to_string()
            };
            tokens.push(text);
        }
    }
    tokens.join(" ")
}

/// Formats SQL with validation.
///
/// This is a convenience function that formats SQL and validates
/// the result in one step.
///
/// # Example
///
/// ```ignore
/// use banshee_format::validate::format_validated;
/// use banshee_format::FormatConfig;
///
/// let result = format_validated("SELECT id FROM users", &FormatConfig::sqlstyle());
/// if result.is_valid {
///     println!("{}", result.output);
/// } else {
///     for error in &result.errors {
///         eprintln!("Validation error: {}", error);
///     }
/// }
/// ```
#[must_use]
pub fn format_validated(source: &str, config: &FormatConfig) -> FormatResult {
    match crate::format::try_format(source, config) {
        Ok(formatted) => FormatResult::ok(formatted),
        Err(e) => FormatResult::with_error(source.to_owned(), e),
    }
}

/// Formats SQL with pgFormatter and validates the result.
#[must_use]
pub fn format_pgformatter_validated(
    source: &str,
    config: &crate::pg_formatter::PgFormatterConfig,
) -> FormatResult {
    match crate::pg_format::try_format(source, config) {
        Ok(formatted) => FormatResult::ok(formatted),
        Err(e) => FormatResult::with_error(source.to_owned(), e),
    }
}

/// Counts significant tokens in SQL (ignoring trivia).
///
/// Useful for sanity checking that formatting doesn't dramatically
/// change the token count.
#[must_use]
pub fn count_tokens(source: &str) -> usize {
    let parse = banshee_parser::parse(source);
    parse
        .syntax()
        .descendants_with_tokens()
        .filter(|element| {
            if let cstree::util::NodeOrToken::Token(token) = element {
                !token.kind().is_trivia()
            } else {
                false
            }
        })
        .count()
}

/// Checks if formatting is idempotent (formatting twice gives same result).
///
/// Returns `Ok(formatted)` if idempotent, `Err` with both results otherwise.
pub fn check_idempotent(source: &str, config: &FormatConfig) -> Result<String, (String, String)> {
    let once = crate::format::format(source, config);
    let twice = crate::format::format(&once, config);

    if once == twice {
        Ok(once)
    } else {
        Err((once, twice))
    }
}

/// Detailed validation result with additional diagnostics.
#[derive(Debug, Clone)]
pub struct ValidationReport {
    /// The original SQL.
    pub original: String,

    /// The formatted SQL.
    pub formatted: String,

    /// Whether parsing succeeded for both.
    pub both_parse: bool,

    /// Whether semantic content matches.
    pub semantically_equal: bool,

    /// Whether formatting is idempotent.
    pub is_idempotent: bool,

    /// Token count in original.
    pub original_tokens: usize,

    /// Token count in formatted.
    pub formatted_tokens: usize,

    /// Any errors encountered.
    pub errors: Vec<FormatError>,
}

impl ValidationReport {
    /// Returns true if all validation checks pass.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.both_parse && self.semantically_equal && self.is_idempotent && self.errors.is_empty()
    }
}

/// Performs comprehensive validation of formatted SQL.
///
/// This runs all validation checks and returns a detailed report.
#[must_use]
pub fn validate_comprehensive(source: &str, config: &FormatConfig) -> ValidationReport {
    let FormatResult {
        output: formatted,
        mut errors,
        ..
    } = format_validated(source, config);

    // Check parsing
    let orig_parse = banshee_parser::parse(source);
    let fmt_parse = banshee_parser::parse(&formatted);

    let orig_ok = orig_parse.errors().is_empty();
    let fmt_ok = fmt_parse.errors().is_empty();
    let both_parse = orig_ok && fmt_ok;

    if !orig_ok && errors.is_empty() {
        errors.push(FormatError::InvalidInput(orig_parse.errors().to_vec()));
    }
    if !fmt_ok {
        errors.push(FormatError::FormatterBrokeQuery(
            fmt_parse.errors().to_vec(),
        ));
    }

    // Check semantic equality (only if both parse)
    let semantically_equal = if both_parse {
        if !crate::safe::preserves_sql(source, &formatted) {
            errors.push(FormatError::SemanticMismatch {
                expected: normalize(&orig_parse),
                actual: normalize(&fmt_parse),
            });
            false
        } else {
            true
        }
    } else {
        false
    };

    // Check idempotency
    let is_idempotent = check_idempotent(source, config).is_ok();

    // Count tokens
    let original_tokens = count_tokens(source);
    let formatted_tokens = count_tokens(&formatted);

    ValidationReport {
        original: source.to_string(),
        formatted,
        both_parse,
        semantically_equal,
        is_idempotent,
        original_tokens,
        formatted_tokens,
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_simple_select() {
        let original = "SELECT id FROM users";
        let formatted = crate::format_sqlstyle(original);
        assert!(validate_format(original, &formatted).is_ok());
    }

    #[test]
    fn test_validate_with_where() {
        let original = "SELECT id, name FROM users WHERE active = true";
        let formatted = crate::format_sqlstyle(original);
        assert!(validate_format(original, &formatted).is_ok());
    }

    #[test]
    fn test_validate_join() {
        let original = "SELECT u.id, o.amount FROM users u LEFT JOIN orders o ON u.id = o.user_id";
        let formatted = crate::format_sqlstyle(original);
        assert!(validate_format(original, &formatted).is_ok());
    }

    #[test]
    fn test_validate_cte() {
        let original = "WITH active AS (SELECT * FROM users WHERE active) SELECT * FROM active";
        let formatted = crate::format_sqlstyle(original);
        assert!(validate_format(original, &formatted).is_ok());
    }

    #[test]
    fn test_validate_invalid_input() {
        let original = "SELECT FROM";
        let formatted = "SELECT FROM";
        let result = validate_format(original, formatted);
        assert!(matches!(result, Err(FormatError::InvalidInput(_))));
    }

    #[test]
    fn test_normalize_ignores_whitespace() {
        let sql1 = "SELECT id FROM users";
        let sql2 = "SELECT\n    id\n  FROM\n    users";

        let parse1 = banshee_parser::parse(sql1);
        let parse2 = banshee_parser::parse(sql2);

        assert_eq!(normalize(&parse1), normalize(&parse2));
    }

    #[test]
    fn test_normalize_ignores_case() {
        let sql1 = "SELECT id FROM users";
        let sql2 = "select id from users";

        let parse1 = banshee_parser::parse(sql1);
        let parse2 = banshee_parser::parse(sql2);

        assert_eq!(normalize(&parse1), normalize(&parse2));
    }

    #[test]
    fn test_count_tokens() {
        let sql = "SELECT id, name FROM users";
        let count = count_tokens(sql);
        // SELECT, id, COMMA, name, FROM, users = 6 tokens
        assert!((5..=7).contains(&count));
    }

    #[test]
    fn test_idempotent() {
        let sql = "SELECT id FROM users";
        let config = FormatConfig::sqlstyle();
        assert!(check_idempotent(sql, &config).is_ok());
    }

    #[test]
    fn test_format_validated() {
        let sql = "SELECT id, name FROM users WHERE active = true";
        let result = format_validated(sql, &FormatConfig::sqlstyle());
        assert!(result.is_valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_comprehensive_validation() {
        let sql = "SELECT id, name FROM users";
        let report = validate_comprehensive(sql, &FormatConfig::sqlstyle());
        assert!(report.is_valid());
        assert!(report.both_parse);
        assert!(report.semantically_equal);
        assert!(report.is_idempotent);
    }

    #[test]
    fn test_format_result_into_result() {
        let result = FormatResult::ok("SELECT id FROM users".to_string());
        assert!(result.into_result().is_ok());

        let result = FormatResult::with_error(
            "broken".to_string(),
            FormatError::SemanticMismatch {
                expected: "a".to_string(),
                actual: "b".to_string(),
            },
        );
        assert!(result.into_result().is_err());
    }
}
