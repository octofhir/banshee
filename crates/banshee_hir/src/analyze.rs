//! Query analysis entry point.
//!
//! This module provides the main entry point for semantic analysis of SQL queries.

use std::collections::HashMap;
use std::sync::Arc;

use text_size::TextRange;

use crate::binding::{ColumnBinding, CteBinding, DataType, Resolution, TableBinding, TableSource};
use crate::scope::{Scope, ScopeBuilder};

/// The result of analyzing a query.
#[derive(Clone, Debug)]
pub struct Analysis {
    /// The root scope of the analyzed query.
    pub scope: Arc<Scope>,

    /// Resolved column references in the query.
    pub columns: Vec<ResolvedReference>,

    /// Resolved table references in the query.
    pub tables: Vec<ResolvedTableRef>,

    /// Diagnostics (errors, warnings) from analysis.
    pub diagnostics: Vec<Diagnostic>,

    /// The output columns of the query (for SELECT).
    pub output_columns: Vec<ColumnBinding>,
}

impl Analysis {
    /// Creates an empty analysis result.
    pub fn empty() -> Self {
        Self {
            scope: ScopeBuilder::new().build(),
            columns: Vec::new(),
            tables: Vec::new(),
            diagnostics: Vec::new(),
            output_columns: Vec::new(),
        }
    }

    /// Returns true if the analysis has errors.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Returns all error diagnostics.
    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
    }

    /// Returns all warning diagnostics.
    pub fn warnings(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
    }
}

/// A resolved column reference in the query.
#[derive(Clone, Debug)]
pub struct ResolvedReference {
    /// The range in the source where the reference appears.
    pub range: TextRange,

    /// The resolved table binding.
    pub table: Arc<TableBinding>,

    /// The resolved column binding.
    pub column: ColumnBinding,

    /// Whether this reference was ambiguous (resolved to first match).
    pub was_ambiguous: bool,
}

/// A resolved table reference in the query.
#[derive(Clone, Debug)]
pub struct ResolvedTableRef {
    /// The range in the source where the reference appears.
    pub range: TextRange,

    /// The resolved table binding.
    pub binding: Arc<TableBinding>,
}

/// A single text replacement that, when applied, resolves (part of) a fix.
///
/// Edits are expressed against the original source offsets. Multiple edits in
/// one [`Fix`] must not overlap; consumers apply them right-to-left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    /// The source range to replace.
    pub range: TextRange,
    /// The replacement text (empty string = deletion).
    pub new_text: String,
}

impl TextEdit {
    /// Creates a replacement edit.
    pub fn replace(range: TextRange, new_text: impl Into<String>) -> Self {
        Self {
            range,
            new_text: new_text.into(),
        }
    }
}

/// A named, applicable fix attached to a diagnostic.
///
/// Maps directly onto an LSP `CodeAction` / quick-fix.
#[derive(Clone, Debug)]
pub struct Fix {
    /// Human-readable title shown in the editor (e.g. "Uppercase keyword").
    pub title: String,
    /// The edits that make up this fix.
    pub edits: Vec<TextEdit>,
}

impl Fix {
    /// Creates a fix from a title and one or more edits.
    pub fn new(title: impl Into<String>, edits: Vec<TextEdit>) -> Self {
        Self {
            title: title.into(),
            edits,
        }
    }
}

/// Stable identifier for a built-in lint/analysis rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RuleCode {
    /// Avoid `SELECT *`.
    Am04,
    /// Implicit cross join.
    Am05,
    /// `UNION`/`EXCEPT`/`INTERSECT` without explicit `ALL`/`DISTINCT`.
    Am02,
    /// `DISTINCT` is redundant alongside `GROUP BY`.
    Am01,
    /// `LIMIT`/`OFFSET` without `ORDER BY` (non-deterministic).
    Am09,
    /// `ORDER BY` mixes explicit and implicit sort directions.
    Am03,
    /// Set-operation branches select a different number of columns.
    Am07,
    /// Redundant `ELSE NULL` in a `CASE` expression.
    St01,
    /// CTE defined but never referenced.
    St03,
    /// Subquery in `FROM`/`JOIN`; prefer a CTE.
    St05,
    /// `NATURAL JOIN` relies on implicit, fragile join columns.
    St07,
    /// `DISTINCT ON` without `ORDER BY` is non-deterministic.
    St08,
    /// Table alias defined without the `AS` keyword.
    Al01,
    /// Column/expression alias defined without the `AS` keyword.
    Al02,
    /// Complex select expression without an alias.
    Al03,
    /// Two tables in one statement share an alias.
    Al04,
    /// Table alias declared but never used as a qualifier.
    Al05,
    /// Two select items share an output alias.
    Al08,
    /// Inconsistent column qualification in a single-table query.
    Rf03,
    /// `UPDATE` without `WHERE`.
    Sf01,
    /// `DELETE` without `WHERE`.
    Sf02,
    /// `INSERT` without an explicit column list.
    Sf03,
    /// JSONB text comparison via `->`.
    Jb01,
    /// Inconsistent `!=` / `<>` spelling.
    Cv01,
    /// `count(1)`/`count(0)` where `count(*)` is idiomatic.
    Cv04,
    /// Relational operator used to compare with NULL.
    Cv05,
    /// Statement not terminated with a semicolon.
    Cv06,
    /// `RIGHT JOIN` used where a `LEFT JOIN` reads more naturally.
    Cv08,
    /// A configured blocked word appears in the SQL.
    Cv09,
    /// `LIKE` with no wildcard behaves like `=`.
    Cv10,
    /// Cast style (`::` vs `CAST`) is inconsistent within a statement.
    Cv11,
    /// `IN` with a single value is equivalent to `=`.
    Cv13,
    /// Keyword capitalisation.
    Cp01,
    /// Unquoted identifier capitalisation.
    Cp02,
    /// Unresolved table/column/alias reference (needs schema).
    Rf01,
    /// Ambiguous column reference (needs schema).
    Rf02,
    /// Identifier quoted unnecessarily.
    Rf06,
    /// `CREATE INDEX` without `CONCURRENTLY` locks the table against writes.
    Mg01,
    /// `ADD CONSTRAINT` (FK/CHECK) without `NOT VALID` validates under a lock.
    Mg02,
    /// `ADD COLUMN` with a volatile `DEFAULT` rewrites the whole table.
    Mg03,
    /// `ADD COLUMN NOT NULL` without a `DEFAULT` fails on a non-empty table.
    Mg04,
    /// `DROP COLUMN` destroys data and breaks dependent objects.
    Mg05,
    /// `ALTER COLUMN … TYPE` rewrites the table under an exclusive lock.
    Mg06,
    /// `RENAME` of a table or column breaks code referring to the old name.
    Mg07,
    /// `TRUNCATE … CASCADE` silently empties dependent tables.
    Mg08,
    /// Prefer `text` over `char(n)`/`varchar(n)`.
    Mg09,
    /// Prefer `timestamptz` over `timestamp`.
    Mg10,
    /// Prefer `bigint` over a narrower integer type for a primary key.
    Mg11,
    /// `DROP INDEX` without `CONCURRENTLY` locks the table.
    Mg12,
    /// `ADD PRIMARY KEY`/`UNIQUE` constraint that builds its index under a lock.
    Mg13,
    /// `ALTER COLUMN … SET NOT NULL` scans the whole table under a lock.
    Mg14,
    /// Prefer a `GENERATED … AS IDENTITY` column to `serial`.
    Mg15,
    /// `DROP TABLE` destroys the table and everything depending on it.
    Mg16,
    /// `ALTER COLUMN … DROP NOT NULL` lets nulls into a column clients assume is set.
    Mg17,
    /// `DROP DATABASE` destroys the whole database and everything in it.
    Mg18,
    /// `CREATE INDEX CONCURRENTLY` inside a transaction block is rejected by Postgres.
    Mg19,
    /// A transaction opened (`BEGIN`/`START`) without a matching `COMMIT`/`ROLLBACK`.
    Mg20,
    /// A `BEGIN`/`START` issued while a transaction is already open.
    Mg21,
    /// A `CREATE`/`DROP` that is not idempotent (`IF [NOT] EXISTS` missing).
    Mg22,
    /// A `CREATE TABLE` whose name is not schema-qualified.
    Mg23,
    /// An identifier longer than Postgres's 63-byte limit (silently truncated).
    Mg24,
    /// `REINDEX` without `CONCURRENTLY` locks out writes for the rebuild.
    Mg25,
    /// `VACUUM FULL`/`CLUSTER` rewrite the table under an exclusive lock.
    Mg26,
    /// A lock-taking migration without a `statement_timeout`/`lock_timeout`.
    Mg27,
    /// `CREATE DOMAIN` with a constraint validates under a lock and is hard to alter.
    Mg28,
    /// `ALTER DOMAIN ADD CONSTRAINT` validates every dependent column under a lock.
    Mg29,
    /// `DETACH PARTITION` without `CONCURRENTLY` holds an exclusive lock.
    Mg30,
}

/// Rule codes for backward-incompatible (breaking) schema changes: operations
/// that drop data, drop objects, or change an existing contract in a way that
/// breaks deployed clients. Used by `banshee lint --breaking`.
pub const BREAKING: &[RuleCode] = &[
    RuleCode::Mg04, // ADD COLUMN NOT NULL without DEFAULT (fails on a non-empty table)
    RuleCode::Mg05, // DROP COLUMN
    RuleCode::Mg06, // ALTER COLUMN TYPE
    RuleCode::Mg07, // RENAME table/column
    RuleCode::Mg08, // TRUNCATE ... CASCADE
    RuleCode::Mg16, // DROP TABLE
    RuleCode::Mg17, // DROP NOT NULL
    RuleCode::Mg18, // DROP DATABASE
];

impl RuleCode {
    /// The canonical string form (e.g. `"AM05"`).
    pub fn as_str(self) -> &'static str {
        match self {
            RuleCode::Am04 => "AM04",
            RuleCode::Am05 => "AM05",
            RuleCode::Am02 => "AM02",
            RuleCode::Am01 => "AM01",
            RuleCode::Am09 => "AM09",
            RuleCode::Am03 => "AM03",
            RuleCode::Am07 => "AM07",
            RuleCode::Al01 => "AL01",
            RuleCode::Al02 => "AL02",
            RuleCode::St01 => "ST01",
            RuleCode::St03 => "ST03",
            RuleCode::St05 => "ST05",
            RuleCode::St07 => "ST07",
            RuleCode::St08 => "ST08",
            RuleCode::Al03 => "AL03",
            RuleCode::Al04 => "AL04",
            RuleCode::Al05 => "AL05",
            RuleCode::Al08 => "AL08",
            RuleCode::Rf03 => "RF03",
            RuleCode::Sf01 => "SF01",
            RuleCode::Sf02 => "SF02",
            RuleCode::Sf03 => "SF03",
            RuleCode::Jb01 => "JB01",
            RuleCode::Cv01 => "CV01",
            RuleCode::Cv04 => "CV04",
            RuleCode::Cv05 => "CV05",
            RuleCode::Cv06 => "CV06",
            RuleCode::Cv08 => "CV08",
            RuleCode::Cv09 => "CV09",
            RuleCode::Cv10 => "CV10",
            RuleCode::Cv11 => "CV11",
            RuleCode::Cv13 => "CV13",
            RuleCode::Cp01 => "CP01",
            RuleCode::Cp02 => "CP02",
            RuleCode::Rf01 => "RF01",
            RuleCode::Rf02 => "RF02",
            RuleCode::Rf06 => "RF06",
            RuleCode::Mg01 => "MG01",
            RuleCode::Mg02 => "MG02",
            RuleCode::Mg03 => "MG03",
            RuleCode::Mg04 => "MG04",
            RuleCode::Mg05 => "MG05",
            RuleCode::Mg06 => "MG06",
            RuleCode::Mg07 => "MG07",
            RuleCode::Mg08 => "MG08",
            RuleCode::Mg09 => "MG09",
            RuleCode::Mg10 => "MG10",
            RuleCode::Mg11 => "MG11",
            RuleCode::Mg12 => "MG12",
            RuleCode::Mg13 => "MG13",
            RuleCode::Mg14 => "MG14",
            RuleCode::Mg15 => "MG15",
            RuleCode::Mg16 => "MG16",
            RuleCode::Mg17 => "MG17",
            RuleCode::Mg18 => "MG18",
            RuleCode::Mg19 => "MG19",
            RuleCode::Mg20 => "MG20",
            RuleCode::Mg21 => "MG21",
            RuleCode::Mg22 => "MG22",
            RuleCode::Mg23 => "MG23",
            RuleCode::Mg24 => "MG24",
            RuleCode::Mg25 => "MG25",
            RuleCode::Mg26 => "MG26",
            RuleCode::Mg27 => "MG27",
            RuleCode::Mg28 => "MG28",
            RuleCode::Mg29 => "MG29",
            RuleCode::Mg30 => "MG30",
        }
    }

    /// Whether this is a schema-dependent reference check.
    pub fn is_reference(self) -> bool {
        matches!(self, RuleCode::Rf01 | RuleCode::Rf02)
    }

    /// Whether this rule flags a backward-incompatible (breaking) change.
    pub fn is_breaking(self) -> bool {
        BREAKING.contains(&self)
    }
}

impl std::fmt::Display for RuleCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A diagnostic message from analysis.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    /// The severity of the diagnostic.
    pub severity: Severity,

    /// A stable rule code, when the diagnostic comes from a rule.
    pub code: Option<RuleCode>,

    /// The diagnostic message.
    pub message: String,

    /// The source range associated with this diagnostic.
    pub range: Option<TextRange>,

    /// Additional related information.
    pub related: Vec<RelatedInfo>,

    /// Suggested fixes (quick-fixes / autofixes) for this diagnostic.
    pub fixes: Vec<Fix>,

    /// An optional `help:` line (e.g. a "did you mean …?" suggestion).
    pub help: Option<String>,
}

impl Diagnostic {
    /// Creates an error diagnostic.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            code: None,
            message: message.into(),
            range: None,
            related: Vec::new(),
            fixes: Vec::new(),
            help: None,
        }
    }

    /// Creates a warning diagnostic.
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            code: None,
            message: message.into(),
            range: None,
            related: Vec::new(),
            fixes: Vec::new(),
            help: None,
        }
    }

    /// Creates an info diagnostic.
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Info,
            code: None,
            message: message.into(),
            range: None,
            related: Vec::new(),
            fixes: Vec::new(),
            help: None,
        }
    }

    /// Sets the rule code for this diagnostic.
    pub fn with_code(mut self, code: RuleCode) -> Self {
        self.code = Some(code);
        self
    }

    /// Sets the source range for this diagnostic.
    pub fn with_range(mut self, range: TextRange) -> Self {
        self.range = Some(range);
        self
    }

    /// Adds related information to this diagnostic.
    pub fn with_related(mut self, info: RelatedInfo) -> Self {
        self.related.push(info);
        self
    }

    /// Attaches a fix to this diagnostic.
    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fixes.push(fix);
        self
    }

    /// Sets the `help:` line for this diagnostic.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

/// Severity level of a diagnostic.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

/// Built-in lint packs that can be enabled for analysis.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltinLintPack {
    /// General SQL quality and safety checks.
    Core,
    /// JSONB-specific checks.
    Jsonb,
    /// Capitalisation/case-consistency checks (fixable). Opt-in: not enabled by
    /// default because the formatter already normalises case.
    Capitalisation,
    /// Convention checks (operator spelling, NULL comparison, semicolons).
    Convention,
    /// Migration-safety checks for DDL (squawk-class): locking operations,
    /// destructive changes, and type preferences.
    Migration,
}

/// External lint pack hook.
///
/// Consumers can implement this trait and pass instances in [`AnalysisOptions`]
/// to add domain-specific lint rules without coupling them to the parser core.
pub trait LintRulePack: Send + Sync {
    /// Applies diagnostics for this pack.
    fn apply(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>);
}

/// Options controlling semantic analysis behavior.
pub struct AnalysisOptions {
    /// Enabled built-in lint packs.
    pub builtin_lint_packs: Vec<BuiltinLintPack>,
    /// Additional external lint packs.
    pub external_lint_packs: Vec<Arc<dyn LintRulePack>>,
    /// Per-rule options, keyed by rule code then option name. Values are
    /// stringified; list-valued options are comma-joined.
    pub rule_options: RuleOptions,
}

/// Per-rule options: `code -> (key -> value)`.
pub type RuleOptions =
    std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>;

/// An empty option set, used when an analyzer is built without options.
static EMPTY_RULE_OPTIONS: RuleOptions = std::collections::BTreeMap::new();

impl AnalysisOptions {
    /// Creates options with default built-in lint packs enabled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces enabled built-in lint packs.
    pub fn with_builtin_lint_packs(
        mut self,
        packs: impl IntoIterator<Item = BuiltinLintPack>,
    ) -> Self {
        self.builtin_lint_packs = packs.into_iter().collect();
        self
    }

    /// Appends an external lint pack.
    pub fn with_external_lint_pack(mut self, pack: Arc<dyn LintRulePack>) -> Self {
        self.external_lint_packs.push(pack);
        self
    }

    /// Sets a single per-rule option (`code`, `key` → `value`).
    pub fn with_rule_option(
        mut self,
        code: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.rule_options
            .entry(code.into())
            .or_default()
            .insert(key.into(), value.into());
        self
    }

    pub(crate) fn has_builtin_pack(&self, pack: BuiltinLintPack) -> bool {
        self.builtin_lint_packs.contains(&pack)
    }
}

impl std::fmt::Debug for AnalysisOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnalysisOptions")
            .field("builtin_lint_packs", &self.builtin_lint_packs)
            .field("external_lint_packs_len", &self.external_lint_packs.len())
            .finish()
    }
}

impl Clone for AnalysisOptions {
    fn clone(&self) -> Self {
        Self {
            builtin_lint_packs: self.builtin_lint_packs.clone(),
            external_lint_packs: self.external_lint_packs.clone(),
            rule_options: self.rule_options.clone(),
        }
    }
}

impl Default for AnalysisOptions {
    fn default() -> Self {
        Self {
            builtin_lint_packs: vec![BuiltinLintPack::Core, BuiltinLintPack::Jsonb],
            external_lint_packs: Vec::new(),
            rule_options: RuleOptions::new(),
        }
    }
}

/// Related information for a diagnostic.
#[derive(Clone, Debug)]
pub struct RelatedInfo {
    /// The message.
    pub message: String,

    /// The source range.
    pub range: Option<TextRange>,
}

/// Schema provider trait for looking up table and column information.
///
/// Implement this trait to provide schema information to the analyzer.
pub trait SchemaProvider: Send + Sync {
    /// Looks up a table by name.
    fn lookup_table(&self, schema: Option<&str>, name: &str) -> Option<TableInfo>;

    /// Looks up columns for a table.
    fn lookup_columns(&self, schema: Option<&str>, table: &str) -> Vec<ColumnInfo>;

    /// Checks if a table exists in the schema.
    fn table_exists(&self, schema: Option<&str>, name: &str) -> bool {
        self.lookup_table(schema, name).is_some()
    }

    /// Checks if a schema exists.
    fn schema_exists(&self, _schema: &str) -> bool {
        true // Default: assume schema exists
    }

    /// Returns all available table names for suggestions.
    fn all_table_names(&self) -> Vec<String> {
        Vec::new()
    }

    /// Returns all available schema names for suggestions.
    fn all_schema_names(&self) -> Vec<String> {
        Vec::new()
    }

    /// Suggests similar table names (for "did you mean" messages).
    fn suggest_tables(&self, _schema: Option<&str>, name: &str) -> Vec<String> {
        suggest_similar(name, &self.all_table_names(), 3)
    }

    /// Suggests similar schema names (for "did you mean" messages).
    fn suggest_schemas(&self, name: &str) -> Vec<String> {
        suggest_similar(name, &self.all_schema_names(), 2)
    }

    /// Suggests similar column names for a table.
    fn suggest_columns(&self, schema: Option<&str>, table: &str, name: &str) -> Vec<String> {
        let columns = self.lookup_columns(schema, table);
        let col_names: Vec<String> = columns.into_iter().map(|c| c.name).collect();
        suggest_similar(name, &col_names, 3)
    }

    /// Returns valid JSONB field names for a given column at a specific path.
    ///
    /// This is used to validate JSONB path expressions like `resource->'name'->'given'`.
    /// The path is the list of keys accessed so far (e.g., `["name"]` when validating `given`).
    ///
    /// Returns `None` if the column has no JSONB schema, or `Some(fields)` with valid
    /// field names at that path.
    fn lookup_jsonb_fields(
        &self,
        _schema: Option<&str>,
        _table: &str,
        _column: &str,
        _path: &[&str],
    ) -> Option<Vec<String>> {
        None // Default: no JSONB schema information
    }

    /// Checks if a JSONB field exists at a given path.
    fn jsonb_field_exists(
        &self,
        schema: Option<&str>,
        table: &str,
        column: &str,
        path: &[&str],
        field: &str,
    ) -> Option<bool> {
        self.lookup_jsonb_fields(schema, table, column, path)
            .map(|fields| fields.iter().any(|f| f.eq_ignore_ascii_case(field)))
    }

    /// Checks if a JSONB field at a given path is an array type.
    ///
    /// Returns `Some(true)` if the field is an array, `Some(false)` if not,
    /// or `None` if the field type cannot be determined.
    fn jsonb_field_is_array(
        &self,
        _schema: Option<&str>,
        _table: &str,
        _column: &str,
        _path: &[&str],
    ) -> Option<bool> {
        None // Default: unknown field type
    }
}

/// Suggests similar strings using prefix matching and Levenshtein distance.
///
/// Priority order:
/// 1. Prefix matches (needle is a prefix of the candidate)
/// 2. Levenshtein distance within max_distance
///
/// This ensures that typing 'pa' suggests 'patient' before 'app'.
/// Formats a list of candidate names as a `help:` suggestion line.
fn did_you_mean(suggestions: &[String]) -> String {
    match suggestions {
        [one] => format!("did you mean '{one}'?"),
        many => format!("did you mean one of: {}?", many.join(", ")),
    }
}

/// Collects every column name visible in a scope (for "did you mean" hints).
fn scope_column_names(scope: &Arc<Scope>) -> Vec<String> {
    let mut names = Vec::new();
    for table in scope.tables() {
        for column in &table.columns {
            names.push(column.name.clone());
        }
    }
    names.sort();
    names.dedup();
    names
}

pub fn suggest_similar(needle: &str, haystack: &[String], max_distance: usize) -> Vec<String> {
    let needle_lower = needle.to_lowercase();

    // Collect matches with their scores
    // Score: (is_prefix_match, levenshtein_distance)
    // Lower is better for both components
    let mut matches: Vec<(String, bool, usize)> = haystack
        .iter()
        .filter_map(|s| {
            let s_lower = s.to_lowercase();
            let is_prefix = s_lower.starts_with(&needle_lower);
            let dist = levenshtein_distance(&needle_lower, &s_lower);

            // Include if it's a prefix match OR within levenshtein distance
            if is_prefix || dist <= max_distance {
                Some((s.clone(), is_prefix, dist))
            } else {
                None
            }
        })
        .collect();

    // Sort: prefix matches first, then by levenshtein distance
    matches.sort_by(|(_, is_prefix_a, dist_a), (_, is_prefix_b, dist_b)| {
        // Prefix matches come first (true > false, so reverse)
        match (is_prefix_a, is_prefix_b) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => dist_a.cmp(dist_b),
        }
    });

    matches.into_iter().take(3).map(|(s, _, _)| s).collect()
}

/// Simple Levenshtein distance implementation.
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev_row: Vec<usize> = (0..=b_len).collect();
    let mut curr_row: Vec<usize> = vec![0; b_len + 1];

    for i in 1..=a_len {
        curr_row[0] = i;
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr_row[j] = (prev_row[j] + 1)
                .min(curr_row[j - 1] + 1)
                .min(prev_row[j - 1] + cost);
        }
        std::mem::swap(&mut prev_row, &mut curr_row);
    }

    prev_row[b_len]
}

/// Information about a table from the schema provider.
#[derive(Clone, Debug)]
pub struct TableInfo {
    /// The schema name.
    pub schema: Option<String>,

    /// The table name.
    pub name: String,

    /// The table type (table, view, etc.).
    pub table_type: TableType,
}

/// Type of a database table.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableType {
    Table,
    View,
    MaterializedView,
    ForeignTable,
}

/// Information about a column from the schema provider.
#[derive(Clone, Debug)]
pub struct ColumnInfo {
    /// The column name.
    pub name: String,

    /// The data type.
    pub data_type: DataType,

    /// Whether the column is nullable.
    pub nullable: bool,

    /// The ordinal position (0-indexed).
    pub ordinal: usize,
}

/// A null schema provider that returns no information.
#[derive(Clone, Debug, Default)]
pub struct NullSchemaProvider;

impl SchemaProvider for NullSchemaProvider {
    fn lookup_table(&self, _schema: Option<&str>, _name: &str) -> Option<TableInfo> {
        None
    }

    fn lookup_columns(&self, _schema: Option<&str>, _table: &str) -> Vec<ColumnInfo> {
        Vec::new()
    }
}

/// The query analyzer.
pub struct Analyzer<'a> {
    /// The schema provider for table/column lookups.
    provider: &'a dyn SchemaProvider,

    /// Collected diagnostics during analysis.
    diagnostics: Vec<Diagnostic>,

    /// Resolved column references.
    resolved_columns: Vec<ResolvedReference>,

    /// Resolved table references.
    resolved_tables: Vec<ResolvedTableRef>,

    /// Per-rule options consulted by config-driven rules.
    rule_options: &'a RuleOptions,
}

impl<'a> Analyzer<'a> {
    /// Creates a new analyzer with the given schema provider.
    pub fn new(provider: &'a dyn SchemaProvider) -> Self {
        Self {
            provider,
            rule_options: &EMPTY_RULE_OPTIONS,
            diagnostics: Vec::new(),
            resolved_columns: Vec::new(),
            resolved_tables: Vec::new(),
        }
    }

    /// The schema provider backing this analyzer.
    pub fn provider(&self) -> &dyn SchemaProvider {
        self.provider
    }

    /// Attaches per-rule options (consumed by config-driven rules).
    pub fn with_rule_options(mut self, options: &'a RuleOptions) -> Self {
        self.rule_options = options;
        self
    }

    /// A scalar option for `code`, e.g. `rule_option("CV11", "prefer")`.
    pub fn rule_option(&self, code: &str, key: &str) -> Option<&str> {
        self.rule_options
            .get(code)
            .and_then(|m| m.get(key))
            .map(String::as_str)
    }

    /// A list-valued option, split on commas with surrounding space trimmed.
    pub fn rule_option_list(&self, code: &str, key: &str) -> Vec<String> {
        self.rule_option(code, key)
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Creates an analyzer with no schema information.
    pub fn without_schema() -> Analyzer<'static> {
        static NULL_PROVIDER: NullSchemaProvider = NullSchemaProvider;
        Analyzer::new(&NULL_PROVIDER)
    }

    /// Adds an error diagnostic.
    pub fn error(&mut self, message: impl Into<String>, range: Option<TextRange>) {
        let mut diag = Diagnostic::error(message);
        if let Some(r) = range {
            diag = diag.with_range(r);
        }
        self.diagnostics.push(diag);
    }

    /// Adds a warning diagnostic.
    pub fn warning(&mut self, message: impl Into<String>, range: Option<TextRange>) {
        let mut diag = Diagnostic::warning(message);
        if let Some(r) = range {
            diag = diag.with_range(r);
        }
        self.diagnostics.push(diag);
    }

    /// Pushes a fully-built diagnostic (with code, fixes, related info).
    pub fn emit(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Builds a table binding from schema information.
    pub fn build_table_binding(
        &self,
        schema: Option<&str>,
        name: &str,
        alias: Option<&str>,
    ) -> TableBinding {
        let columns = self
            .provider
            .lookup_columns(schema, name)
            .into_iter()
            .map(|c| {
                ColumnBinding::new(c.name, c.ordinal)
                    .with_type(c.data_type)
                    .with_nullable(c.nullable)
            })
            .collect();

        let mut binding =
            TableBinding::table(schema.map(String::from), name.to_string()).with_columns(columns);

        if let Some(a) = alias {
            binding = binding.with_alias(a.to_string());
        }

        binding
    }

    /// Builds a CTE binding.
    pub fn build_cte_binding(
        &self,
        name: &str,
        columns: Vec<String>,
        is_recursive: bool,
    ) -> CteBinding {
        let mut binding = CteBinding::new(name.to_string()).with_columns(columns);
        if is_recursive {
            binding = binding.recursive();
        }
        binding
    }

    /// Records a resolved column reference.
    pub fn record_column_reference(
        &mut self,
        range: TextRange,
        table: Arc<TableBinding>,
        column: ColumnBinding,
        was_ambiguous: bool,
    ) {
        self.resolved_columns.push(ResolvedReference {
            range,
            table,
            column,
            was_ambiguous,
        });
    }

    /// Records a resolved table reference.
    pub fn record_table_reference(&mut self, range: TextRange, binding: Arc<TableBinding>) {
        self.resolved_tables
            .push(ResolvedTableRef { range, binding });
    }

    /// Completes the analysis and returns the result.
    pub fn finish(self, scope: Arc<Scope>, output_columns: Vec<ColumnBinding>) -> Analysis {
        Analysis {
            scope,
            columns: self.resolved_columns,
            tables: self.resolved_tables,
            diagnostics: self.diagnostics,
            output_columns,
        }
    }
}

/// Analyzes a FROM clause and builds the scope.
pub fn analyze_from_clause(
    tables: Vec<TableBinding>,
    ctes: Vec<CteBinding>,
    parent_scope: Option<Arc<Scope>>,
) -> Arc<Scope> {
    let mut builder = match parent_scope {
        Some(parent) => ScopeBuilder::with_parent(parent),
        None => ScopeBuilder::new(),
    };

    // Add CTEs first (they shadow physical tables)
    for cte in ctes {
        builder = builder.add_cte(cte);
    }

    // Add tables from FROM clause
    for table in tables {
        builder = builder.add_table(table);
    }

    builder.build()
}

/// Analyzes a LATERAL subquery and builds the scope.
///
/// LATERAL allows referencing tables that appear earlier in the FROM clause.
pub fn analyze_lateral_scope(
    preceding_tables: Vec<String>,
    subquery_tables: Vec<TableBinding>,
    parent_scope: Arc<Scope>,
) -> Arc<Scope> {
    let mut builder = ScopeBuilder::with_parent(parent_scope).lateral();

    for table_name in preceding_tables {
        builder = builder.add_lateral_table(table_name);
    }

    for table in subquery_tables {
        builder = builder.add_table(table);
    }

    builder.build()
}

/// Expands SELECT * to include all columns from all tables in scope.
pub fn expand_star(scope: &Arc<Scope>) -> Vec<ColumnBinding> {
    let mut columns = Vec::new();
    let mut ordinal = 0;

    for table in scope.tables() {
        for col in &table.columns {
            columns.push(ColumnBinding {
                name: col.name.clone(),
                table: Some(table.reference_name().to_string()),
                data_type: col.data_type.clone(),
                ordinal,
                range: None,
                nullable: col.nullable,
            });
            ordinal += 1;
        }
    }

    columns
}

/// Expands SELECT table.* to include all columns from a specific table.
pub fn expand_table_star(
    table_name: &str,
    scope: &Arc<Scope>,
    start_ordinal: usize,
) -> Resolution<Vec<ColumnBinding>> {
    match scope.resolve_table(table_name) {
        Resolution::Resolved(table) => {
            let columns = table
                .columns
                .iter()
                .enumerate()
                .map(|(i, col)| ColumnBinding {
                    name: col.name.clone(),
                    table: Some(table.reference_name().to_string()),
                    data_type: col.data_type.clone(),
                    ordinal: start_ordinal + i,
                    range: None,
                    nullable: col.nullable,
                })
                .collect();
            Resolution::Resolved(columns)
        }
        Resolution::Ambiguous(_) => Resolution::Ambiguous(vec![]),
        Resolution::Unresolved => Resolution::Unresolved,
    }
}

// =============================================================================
// Semantic Query Analysis
// =============================================================================

use banshee_syntax::Parse;
use banshee_syntax::SyntaxKind;
use banshee_syntax::ast::{
    AstNode, ColumnRef, CreateTableStmt, CreateViewStmt, JsonbAccessExpr, WithClause,
};

use crate::resolve::{validate_column_reference, validate_table_reference};

/// A relation defined by DDL in the source under analysis.
struct OverlayRelation {
    table_type: TableType,
    columns: Vec<ColumnInfo>,
}

/// Overlays relations defined by DDL in the same source on top of an external
/// schema provider, so that references to just-created tables (and their
/// columns) resolve instead of being reported as missing. Lookups consult the
/// file-local relations first, then fall back to the inner provider.
struct DdlOverlay<'a> {
    inner: &'a dyn SchemaProvider,
    relations: std::collections::BTreeMap<String, OverlayRelation>,
}

impl<'a> DdlOverlay<'a> {
    fn new(inner: &'a dyn SchemaProvider, root: &banshee_syntax::SyntaxNode) -> Self {
        let mut relations = std::collections::BTreeMap::new();
        for node in root.descendants() {
            match node.kind() {
                SyntaxKind::CREATE_TABLE_STMT => {
                    let Some(ct) = CreateTableStmt::cast(node.clone()) else {
                        continue;
                    };
                    let Some(name) = ct.name().and_then(|n| n.name()) else {
                        continue;
                    };
                    let mut columns = Vec::new();
                    for col in ct.columns() {
                        let Some(col_name) = col.name() else { continue };
                        columns.push(ColumnInfo {
                            name: col_name.text().to_string(),
                            data_type: col
                                .type_name()
                                .map(|t| data_type_from_text(&t.text()))
                                .unwrap_or(DataType::Unknown),
                            nullable: !col.is_not_null() && !col.is_primary_key(),
                            ordinal: columns.len(),
                        });
                    }
                    relations.insert(
                        name.text().to_ascii_lowercase(),
                        OverlayRelation {
                            table_type: TableType::Table,
                            columns,
                        },
                    );
                }
                SyntaxKind::CREATE_VIEW_STMT => {
                    let Some(cv) = CreateViewStmt::cast(node.clone()) else {
                        continue;
                    };
                    if let Some(name) = cv.name().and_then(|n| n.name()) {
                        let table_type = if cv.is_materialized() {
                            TableType::MaterializedView
                        } else {
                            TableType::View
                        };
                        relations.insert(
                            name.text().to_ascii_lowercase(),
                            OverlayRelation {
                                table_type,
                                columns: Vec::new(),
                            },
                        );
                    }
                }
                _ => {}
            }
        }
        Self { inner, relations }
    }

    fn relation(&self, name: &str) -> Option<&OverlayRelation> {
        self.relations.get(&name.to_ascii_lowercase())
    }
}

impl SchemaProvider for DdlOverlay<'_> {
    fn lookup_table(&self, schema: Option<&str>, name: &str) -> Option<TableInfo> {
        if let Some(rel) = self.relation(name) {
            return Some(TableInfo {
                schema: schema.map(str::to_string),
                name: name.to_string(),
                table_type: rel.table_type,
            });
        }
        self.inner.lookup_table(schema, name)
    }

    fn lookup_columns(&self, schema: Option<&str>, table: &str) -> Vec<ColumnInfo> {
        match self.relation(table) {
            Some(rel) if !rel.columns.is_empty() => rel.columns.clone(),
            Some(_) => Vec::new(),
            None => self.inner.lookup_columns(schema, table),
        }
    }

    fn table_exists(&self, schema: Option<&str>, name: &str) -> bool {
        self.relation(name).is_some() || self.inner.table_exists(schema, name)
    }

    fn schema_exists(&self, schema: &str) -> bool {
        self.inner.schema_exists(schema)
    }

    fn all_table_names(&self) -> Vec<String> {
        let mut names = self.inner.all_table_names();
        names.extend(self.relations.keys().cloned());
        names
    }

    fn all_schema_names(&self) -> Vec<String> {
        self.inner.all_schema_names()
    }

    fn lookup_jsonb_fields(
        &self,
        schema: Option<&str>,
        table: &str,
        column: &str,
        path: &[&str],
    ) -> Option<Vec<String>> {
        self.inner.lookup_jsonb_fields(schema, table, column, path)
    }

    fn jsonb_field_is_array(
        &self,
        schema: Option<&str>,
        table: &str,
        column: &str,
        path: &[&str],
    ) -> Option<bool> {
        self.inner.jsonb_field_is_array(schema, table, column, path)
    }
}

/// Maps a column type's text to a coarse [`DataType`] for file-local relations.
/// Resolution only relies on column names, so unrecognized types fall back to
/// [`DataType::Custom`].
fn data_type_from_text(text: &str) -> DataType {
    let lower = text.to_ascii_lowercase();
    let base = lower.split(['(', '[']).next().unwrap_or("").trim();
    match base {
        "smallint" | "int2" => DataType::SmallInt,
        "integer" | "int" | "int4" => DataType::Integer,
        "bigint" | "int8" => DataType::BigInt,
        "real" | "float4" => DataType::Real,
        "double precision" | "float8" => DataType::DoublePrecision,
        "numeric" | "decimal" => DataType::Numeric {
            precision: None,
            scale: None,
        },
        "text" => DataType::Text,
        "bytea" => DataType::ByteA,
        "date" => DataType::Date,
        "boolean" | "bool" => DataType::Boolean,
        "uuid" => DataType::Uuid,
        "json" => DataType::Json,
        "jsonb" => DataType::Jsonb,
        "interval" => DataType::Interval,
        b if b.starts_with("varchar") || b.starts_with("character varying") => {
            DataType::VarChar { length: None }
        }
        b if b.starts_with("char") || b.starts_with("character") => DataType::Char { length: None },
        b if b.starts_with("timestamp") => DataType::Timestamp {
            with_timezone: lower.contains("with time zone") || base.contains("timestamptz"),
        },
        b if b.starts_with("time") => DataType::Time {
            with_timezone: lower.contains("with time zone"),
        },
        "" => DataType::Unknown,
        other => DataType::Custom(other.to_string()),
    }
}

/// Whether `kind` is a DDL or command statement (as opposed to a query).
fn is_ddl_or_command_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::CREATE_TABLE_STMT
            | SyntaxKind::CREATE_INDEX_STMT
            | SyntaxKind::CREATE_VIEW_STMT
            | SyntaxKind::CREATE_SEQUENCE_STMT
            | SyntaxKind::CREATE_SCHEMA_STMT
            | SyntaxKind::CREATE_EXTENSION_STMT
            | SyntaxKind::CREATE_TYPE_STMT
            | SyntaxKind::CREATE_FUNCTION_STMT
            | SyntaxKind::CREATE_TRIGGER_STMT
            | SyntaxKind::ALTER_STMT
            | SyntaxKind::DROP_STMT
            | SyntaxKind::TRUNCATE_STMT
            | SyntaxKind::COMMENT_STMT
            | SyntaxKind::TRANSACTION_STMT
            | SyntaxKind::SET_STMT
            | SyntaxKind::SHOW_STMT
            | SyntaxKind::RESET_STMT
            | SyntaxKind::CALL_STMT
            | SyntaxKind::DO_STMT
            | SyntaxKind::VACUUM_STMT
            | SyntaxKind::ANALYZE_STMT
            | SyntaxKind::COPY_STMT
            | SyntaxKind::GRANT_STMT
            | SyntaxKind::REVOKE_STMT
            | SyntaxKind::REINDEX_STMT
            | SyntaxKind::CLUSTER_STMT
            | SyntaxKind::CREATE_DOMAIN_STMT
            | SyntaxKind::ALTER_DOMAIN_STMT
    )
}

/// Whether `node` lies within a DDL/command statement. Column references there
/// are not part of a query scope, so the column checks are skipped to avoid
/// false "column not found" diagnostics.
fn within_ddl_or_command(node: &banshee_syntax::SyntaxNode) -> bool {
    std::iter::successors(Some(node.clone()), |n| n.parent().cloned())
        .any(|n| is_ddl_or_command_kind(n.kind()))
}

/// Analyzes a parsed SQL query for semantic errors using default options.
///
/// This function walks the AST and validates:
/// - Table references (checks if tables exist)
/// - Column references (checks if columns exist and are unambiguous)
/// - JSONB access chains (validates path structure)
///
/// Returns an Analysis containing diagnostics for any errors found.
pub fn analyze_query(parse: &Parse, provider: &dyn SchemaProvider) -> Analysis {
    let options = AnalysisOptions::default();
    analyze_query_with_options(parse, provider, &options)
}

/// Analyzes a parsed SQL query for semantic errors with explicit options.
pub fn analyze_query_with_options(
    parse: &Parse,
    provider: &dyn SchemaProvider,
    options: &AnalysisOptions,
) -> Analysis {
    // Walk the syntax tree to find table and column references
    let root = parse.syntax();

    // Overlay relations defined by DDL in this source so their names and
    // columns resolve. All resolution below goes through the overlay.
    let overlay = DdlOverlay::new(provider, &root);
    let provider: &dyn SchemaProvider = &overlay;

    let mut analyzer = Analyzer::new(provider).with_rule_options(&options.rule_options);

    for node in root.descendants() {
        if node.kind() != SyntaxKind::TABLE_REF {
            continue;
        }

        // DDL/command statements name objects (definitions and targets) that
        // banshee does not track in the provider; the overlay resolves file-local
        // ones, and existence checks on the rest would be noise.
        if within_ddl_or_command(node) {
            continue;
        }

        let Some((name, _)) = extract_table_ref_name_and_alias(node) else {
            continue;
        };

        let (schema, name_only) = split_qualified_table_name(&name);
        let range = node.text_range();
        let visible_ctes = visible_ctes_for_node(node, &analyzer);

        if schema.is_none()
            && visible_ctes
                .iter()
                .any(|cte| cte.name.eq_ignore_ascii_case(&name_only))
        {
            continue;
        }

        let validation =
            validate_table_reference(schema.as_deref(), &name_only, provider, Some(range));

        if !validation.exists {
            let diag = if !validation.schema_exists {
                Diagnostic::error(format!(
                    "Schema '{}' does not exist",
                    schema.as_deref().unwrap_or("")
                ))
                .with_code(RuleCode::Rf01)
                .with_range(range)
            } else {
                let mut d = Diagnostic::error(format!("Table '{}' does not exist", name_only))
                    .with_code(RuleCode::Rf01)
                    .with_range(range);
                if !validation.suggestions.is_empty() {
                    d = d.with_help(did_you_mean(&validation.suggestions));
                }
                d
            };
            analyzer.emit(diag);
        }
    }

    let scope = build_root_scope(&root, provider, &analyzer);

    // Second pass: validate column references
    for node in root.descendants() {
        // Column references inside DDL/command statements (index elements,
        // CHECK predicates, DEFAULT expressions) are not part of a query scope.
        if within_ddl_or_command(node) {
            continue;
        }
        if let Some(col_ref) = ColumnRef::cast(node.clone()) {
            let column = col_ref.column().map(|t| t.text().to_string());
            let table_qualifier = col_ref.table().map(|t| t.text().to_string());
            let range = col_ref.syntax().text_range();
            let local_scope = build_scope_for_node(col_ref.syntax(), provider, &analyzer);

            if let Some(ref column) = column {
                let validation = validate_column_reference(
                    table_qualifier.as_deref(),
                    column,
                    &local_scope,
                    provider,
                    Some(range),
                );

                match validation {
                    crate::resolve::ColumnValidation::Found { .. } => {}
                    crate::resolve::ColumnValidation::UnknownColumn {
                        column,
                        table,
                        suggestions,
                        range,
                        ..
                    } => {
                        let mut diag = Diagnostic::error(format!(
                            "Column '{}' not found in table '{}'",
                            column, table
                        ))
                        .with_code(RuleCode::Rf01);
                        if let Some(r) = range {
                            diag = diag.with_range(r);
                        }
                        if !suggestions.is_empty() {
                            diag = diag.with_help(did_you_mean(&suggestions));
                        }
                        analyzer.emit(diag);
                    }
                    crate::resolve::ColumnValidation::UnknownAlias {
                        alias,
                        available_aliases,
                        range,
                    } => {
                        let mut diag =
                            Diagnostic::error(format!("Unknown table alias '{}'", alias))
                                .with_code(RuleCode::Rf01);
                        if let Some(r) = range {
                            diag = diag.with_range(r);
                        }
                        if !available_aliases.is_empty() {
                            diag = diag.with_help(format!(
                                "available aliases: {}",
                                available_aliases.join(", ")
                            ));
                        }
                        analyzer.emit(diag);
                    }
                    crate::resolve::ColumnValidation::NotFound { column, range } => {
                        let mut diag = Diagnostic::error(format!(
                            "Column '{}' not found in any table in scope",
                            column
                        ))
                        .with_code(RuleCode::Rf01);
                        if let Some(r) = range {
                            diag = diag.with_range(r);
                        }
                        let candidates = scope_column_names(&local_scope);
                        let suggestions = suggest_similar(&column, &candidates, 3);
                        if !suggestions.is_empty() {
                            diag = diag.with_help(did_you_mean(&suggestions));
                        }
                        analyzer.emit(diag);
                    }
                    crate::resolve::ColumnValidation::Ambiguous {
                        column,
                        tables,
                        range,
                    } => {
                        let table_names: Vec<String> =
                            tables.iter().map(|(alias, _)| alias.clone()).collect();
                        let mut diag = Diagnostic::error(format!(
                            "Column '{}' is ambiguous; found in multiple tables",
                            column
                        ))
                        .with_code(RuleCode::Rf02);
                        if let Some(r) = range {
                            diag = diag.with_range(r);
                        }
                        diag = diag.with_help(format!(
                            "qualify with a table name: {}",
                            table_names
                                .iter()
                                .map(|t| format!("{t}.{column}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                        analyzer.emit(diag);
                    }
                }
            }
        }
    }

    // Third pass: validate JSONB access chains
    for node in root.descendants() {
        if let Some(jsonb_access) = JsonbAccessExpr::cast(node.clone())
            && let Some(chain) = jsonb_access.extract_chain()
        {
            let range = jsonb_access.syntax().text_range();
            let local_scope = build_scope_for_node(jsonb_access.syntax(), provider, &analyzer);

            // Validate that the base column is of type JSONB/JSON
            let table_qualifier = chain.table_qualifier.as_deref();
            let column_name = &chain.column;

            // Try to find the column type in scope
            let column_type = local_scope
                .resolve_column(column_name, table_qualifier)
                .ok()
                .and_then(|(_, col)| col.data_type.clone());

            if let Some(ref data_type) = column_type {
                // Check if it's a JSONB/JSON type
                let is_jsonb_type = matches!(data_type, DataType::Jsonb | DataType::Json);

                if !is_jsonb_type {
                    let type_name = data_type.to_string();
                    let op_token = jsonb_access
                        .operator_token()
                        .map(|t| t.text().to_string())
                        .unwrap_or_else(|| "->".to_string());

                    let message = if let Some(table) = table_qualifier {
                        format!(
                            "JSONB operator '{}' cannot be applied to column '{}.{}' of type '{}'",
                            op_token, table, column_name, type_name
                        )
                    } else {
                        format!(
                            "JSONB operator '{}' cannot be applied to column '{}' of type '{}'",
                            op_token, column_name, type_name
                        )
                    };

                    analyzer.error(message, Some(range));
                }
            }

            // Validate the chain structure
            let segments: Vec<(String, bool)> = chain
                .segments
                .iter()
                .map(|s| (s.key.clone(), s.extracts_text))
                .collect();

            let validation = crate::resolve::validate_jsonb_chain(&segments, None);

            for error in validation.errors {
                analyzer.error(error.error_message(), Some(range));
            }

            // Validate JSONB path segments against schema metadata (if available)
            // Get the table name for JSONB field lookup
            let table_name = local_scope
                .resolve_column(column_name, table_qualifier)
                .ok()
                .map(|(table_binding, _)| {
                    // Get the original table name (not alias)
                    match &table_binding.source {
                        TableSource::Table { name, .. } => name.clone(),
                        _ => table_binding.original_name.clone(),
                    }
                });

            if let Some(ref table) = table_name {
                let path_keys: Vec<&str> = chain.path_keys();

                // Validate each path segment
                for (i, key) in path_keys.iter().enumerate() {
                    let parent_path: Vec<&str> = path_keys[..i].to_vec();

                    // Check if this is a numeric index (array access)
                    let is_numeric_index = key.parse::<usize>().is_ok();

                    if is_numeric_index {
                        // Check if the parent field is actually an array
                        if let Some(false) =
                            provider.jsonb_field_is_array(None, table, column_name, &parent_path)
                        {
                            let path_str = if parent_path.is_empty() {
                                column_name.to_string()
                            } else {
                                format!("{}.{}", column_name, parent_path.join("."))
                            };

                            analyzer.error(
                                format!(
                                    "Cannot use array index '{}' on non-array property '{}'",
                                    key, path_str
                                ),
                                Some(range),
                            );
                        }
                    } else {
                        // Check if this field exists at the current path
                        if let Some(false) =
                            provider.jsonb_field_exists(None, table, column_name, &parent_path, key)
                        {
                            let path_str = if parent_path.is_empty() {
                                column_name.to_string()
                            } else {
                                format!("{}.{}", column_name, parent_path.join("."))
                            };

                            analyzer.error(
                                format!("Unknown property '{}' in '{}'", key, path_str),
                                Some(range),
                            );
                        }
                    }
                }
            }
        }
    }

    // Final pass: lint rules (non-fatal quality/safety diagnostics)
    crate::lint::apply_lints(&root, &mut analyzer, options);

    let mut analysis = analyzer.finish(scope, Vec::new());
    // Rules run in registry order, so sort diagnostics into source order for a
    // deterministic, rule-order-independent result. Ties break on the rule
    // code so output is fully stable.
    analysis.diagnostics.sort_by(|a, b| {
        let key = |d: &Diagnostic| {
            let start = d.range.map(|r| u32::from(r.start())).unwrap_or(u32::MAX);
            let end = d.range.map(|r| u32::from(r.end())).unwrap_or(u32::MAX);
            (start, end, d.code.map(|c| c.as_str()).unwrap_or(""))
        };
        key(a).cmp(&key(b))
    });
    // Honour inline `-- noqa` suppression comments.
    crate::noqa::apply(&root, &mut analysis.diagnostics);
    analysis
}

fn build_root_scope(
    root: &banshee_syntax::SyntaxNode,
    provider: &dyn SchemaProvider,
    analyzer: &Analyzer<'_>,
) -> Arc<Scope> {
    root.children()
        .find(|child| is_statement_kind(child.kind()))
        .map(|statement| build_scope_for_statement(statement, statement, provider, analyzer))
        .unwrap_or_else(|| ScopeBuilder::new().build())
}

fn build_scope_for_node(
    node: &banshee_syntax::SyntaxNode,
    provider: &dyn SchemaProvider,
    analyzer: &Analyzer<'_>,
) -> Arc<Scope> {
    nearest_statement(node)
        .map(|statement| build_scope_for_statement(&statement, node, provider, analyzer))
        .unwrap_or_else(|| ScopeBuilder::new().build())
}

fn build_scope_for_statement(
    statement: &banshee_syntax::SyntaxNode,
    context_node: &banshee_syntax::SyntaxNode,
    provider: &dyn SchemaProvider,
    analyzer: &Analyzer<'_>,
) -> Arc<Scope> {
    let visible_ctes = visible_ctes_for_node(context_node, analyzer);
    let cte_map: HashMap<String, CteBinding> = visible_ctes
        .iter()
        .cloned()
        .map(|cte| (cte.name.to_lowercase(), cte))
        .collect();

    let mut builder = ScopeBuilder::new();
    for cte in visible_ctes {
        builder = builder.add_cte_declaration(cte);
    }

    for table_ref in statement.descendants() {
        if table_ref.kind() != SyntaxKind::TABLE_REF {
            continue;
        }
        if nearest_statement(table_ref)
            .as_ref()
            .is_none_or(|owner| owner.text_range() != statement.text_range())
        {
            continue;
        }

        let Some((name, inline_alias)) = extract_table_ref_name_and_alias(table_ref) else {
            continue;
        };

        let (schema, name_only) = split_qualified_table_name(&name);
        let alias = inline_alias.or_else(|| extract_statement_target_alias(table_ref));
        let range = table_ref.text_range();

        if schema.is_none()
            && let Some(cte) = cte_map.get(&name_only.to_lowercase())
        {
            let columns = cte
                .columns
                .iter()
                .enumerate()
                .map(|(ordinal, column)| ColumnBinding::new(column.clone(), ordinal))
                .collect();
            let mut binding = TableBinding::cte(cte.name.clone())
                .with_columns(columns)
                .with_range(range);
            if let Some(alias) = alias {
                binding = binding.with_alias(alias);
            }
            builder = builder.add_table(binding);
            continue;
        }

        if provider.table_exists(schema.as_deref(), &name_only) {
            let binding =
                analyzer.build_table_binding(schema.as_deref(), &name_only, alias.as_deref());
            builder = builder.add_table(binding.with_range(range));
        }
    }

    builder.build()
}

fn visible_ctes_for_node(
    node: &banshee_syntax::SyntaxNode,
    analyzer: &Analyzer<'_>,
) -> Vec<CteBinding> {
    let statements: Vec<_> = node
        .ancestors()
        .filter(|ancestor| is_statement_kind(ancestor.kind()))
        .cloned()
        .collect();
    let mut bindings = Vec::new();

    for statement in statements.iter().rev() {
        let Some(with_clause) = statement
            .children()
            .find_map(|child| WithClause::cast(child.clone()))
        else {
            continue;
        };

        bindings.extend(visible_ctes_in_with_clause(&with_clause, node, analyzer));
    }

    bindings
}

fn visible_ctes_in_with_clause(
    with_clause: &WithClause,
    node: &banshee_syntax::SyntaxNode,
    analyzer: &Analyzer<'_>,
) -> Vec<CteBinding> {
    let mut bindings = Vec::new();
    let recursive = with_clause.is_recursive();
    let current_cte = node.ancestors().find(|ancestor| {
        ancestor.kind() == SyntaxKind::CTE
            && ancestor
                .ancestors()
                .any(|parent| parent.text_range() == with_clause.syntax().text_range())
    });

    for cte in with_clause.ctes() {
        let is_current = current_cte
            .as_ref()
            .is_some_and(|current| current.text_range() == cte.syntax().text_range());

        if is_current {
            if recursive && let Some(binding) = extract_cte_binding(&cte, recursive, analyzer) {
                bindings.push(binding);
            }
            break;
        }

        if let Some(binding) = extract_cte_binding(&cte, recursive, analyzer) {
            bindings.push(binding);
        }
    }

    if current_cte.is_none() {
        bindings.clear();
        for cte in with_clause.ctes() {
            if let Some(binding) = extract_cte_binding(&cte, recursive, analyzer) {
                bindings.push(binding);
            }
        }
    }

    bindings
}

fn nearest_statement(node: &banshee_syntax::SyntaxNode) -> Option<banshee_syntax::SyntaxNode> {
    node.ancestors()
        .find(|ancestor| is_statement_kind(ancestor.kind()))
        .cloned()
}

fn is_statement_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::SELECT_STMT
            | SyntaxKind::UPDATE_STMT
            | SyntaxKind::DELETE_STMT
            | SyntaxKind::INSERT_STMT
    )
}

fn extract_cte_binding(
    cte: &banshee_syntax::ast::Cte,
    is_recursive: bool,
    analyzer: &Analyzer<'_>,
) -> Option<CteBinding> {
    let name = cte.name()?.text().to_string();
    let columns = extract_cte_columns(cte.syntax());

    Some(
        analyzer
            .build_cte_binding(&name, columns, is_recursive)
            .with_range(cte.syntax().text_range()),
    )
}

fn extract_cte_columns(cte: &banshee_syntax::SyntaxNode) -> Vec<String> {
    let mut name_seen = false;
    let mut explicit_columns = Vec::new();
    let mut in_column_list = false;
    let mut found_as = false;

    for child in cte.children_with_tokens() {
        if let Some(token) = child.as_token() {
            match token.kind() {
                SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT => {
                    if !name_seen {
                        name_seen = true;
                    } else if in_column_list && !found_as {
                        explicit_columns.push(token.text().to_string());
                    }
                }
                SyntaxKind::L_PAREN if name_seen && !found_as => {
                    in_column_list = true;
                }
                SyntaxKind::R_PAREN if in_column_list && !found_as => {
                    in_column_list = false;
                }
                SyntaxKind::AS_KW => {
                    found_as = true;
                    in_column_list = false;
                }
                _ => {}
            }
        } else if let Some(node) = child.as_node()
            && found_as
            && explicit_columns.is_empty()
        {
            explicit_columns = infer_cte_output_columns(node);
        }
    }

    explicit_columns
}

fn infer_cte_output_columns(statement: &banshee_syntax::SyntaxNode) -> Vec<String> {
    match statement.kind() {
        SyntaxKind::SELECT_STMT => extract_select_output_columns(statement),
        SyntaxKind::INSERT_STMT | SyntaxKind::UPDATE_STMT | SyntaxKind::DELETE_STMT => {
            extract_returning_output_columns(statement)
        }
        _ => Vec::new(),
    }
}

fn extract_select_output_columns(select: &banshee_syntax::SyntaxNode) -> Vec<String> {
    let item_list = select
        .children()
        .find(|child| child.kind() == SyntaxKind::SELECT_ITEM_LIST)
        .or_else(|| {
            select
                .children()
                .find(|child| child.kind() == SyntaxKind::SELECT_CLAUSE)
                .and_then(|clause| {
                    clause
                        .children()
                        .find(|child| child.kind() == SyntaxKind::SELECT_ITEM_LIST)
                })
        });
    let Some(item_list) = item_list else {
        return Vec::new();
    };

    item_list
        .children()
        .filter(|child| child.kind() == SyntaxKind::SELECT_ITEM)
        .filter_map(extract_select_item_name)
        .collect()
}

fn extract_returning_output_columns(statement: &banshee_syntax::SyntaxNode) -> Vec<String> {
    let Some(returning_clause) = statement
        .children()
        .find(|child| child.kind() == SyntaxKind::RETURNING_CLAUSE)
    else {
        return Vec::new();
    };

    let direct_items: Vec<_> = returning_clause
        .children()
        .filter(|child| child.kind() == SyntaxKind::SELECT_ITEM)
        .filter_map(extract_select_item_name)
        .collect();

    if !direct_items.is_empty() {
        return direct_items;
    }

    returning_clause
        .children()
        .filter_map(extract_select_item_name)
        .collect()
}

fn extract_select_item_name(item: &banshee_syntax::SyntaxNode) -> Option<String> {
    for child in item.children() {
        if child.kind() == SyntaxKind::ALIAS {
            for token in child
                .children_with_tokens()
                .filter_map(|part| part.into_token())
            {
                if matches!(token.kind(), SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT) {
                    return Some(token.text().to_string());
                }
            }
        }
    }

    for child in item.descendants() {
        if child.kind() == SyntaxKind::COLUMN_REF {
            let mut last_ident = None;
            for token in child
                .children_with_tokens()
                .filter_map(|part| part.into_token())
            {
                if matches!(token.kind(), SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT) {
                    last_ident = Some(token.text().to_string());
                }
            }
            if last_ident.is_some() {
                return last_ident;
            }
        }
    }

    for token in item
        .descendants_with_tokens()
        .filter_map(|part| part.into_token())
    {
        if matches!(token.kind(), SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT) {
            return Some(token.text().to_string());
        }
    }

    None
}

fn extract_table_ref_name_and_alias(
    node: &banshee_syntax::SyntaxNode,
) -> Option<(String, Option<String>)> {
    let mut name = String::new();
    let mut alias = None;
    let mut saw_name = false;
    let mut in_alias = false;

    for element in node.children_with_tokens() {
        let token = element.as_token()?;

        match token.kind() {
            SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT => {
                if in_alias {
                    alias = Some(token.text().to_string());
                    break;
                }
                saw_name = true;
                name.push_str(token.text());
            }
            SyntaxKind::DOT if !in_alias => {
                saw_name = true;
                name.push('.');
            }
            SyntaxKind::AS_KW if saw_name => in_alias = true,
            _ if token.kind().is_trivia() && saw_name => in_alias = true,
            _ => {}
        }
    }

    (!name.is_empty()).then_some((name, alias))
}

fn split_qualified_table_name(name: &str) -> (Option<String>, String) {
    let mut parts = name
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if parts.is_empty() {
        return (None, name.to_string());
    }

    let table = parts.pop().unwrap_or(name).to_string();
    let schema = parts.pop().map(str::to_string);
    (schema, table)
}

fn extract_statement_target_alias(node: &banshee_syntax::SyntaxNode) -> Option<String> {
    let parent = node.parent()?;
    if !matches!(
        parent.kind(),
        SyntaxKind::UPDATE_STMT | SyntaxKind::DELETE_STMT | SyntaxKind::INSERT_STMT
    ) {
        return None;
    }

    let is_target = parent
        .children()
        .find(|child| child.kind() == SyntaxKind::TABLE_REF)
        .is_some_and(|target| target.text_range() == node.text_range());
    if !is_target {
        return None;
    }

    let mut after_target = false;
    let mut saw_as = false;

    for element in parent.children_with_tokens() {
        if let Some(child) = element.as_node() {
            if !after_target {
                after_target = child.text_range() == node.text_range();
                continue;
            }

            if matches!(
                child.kind(),
                SyntaxKind::SET_CLAUSE
                    | SyntaxKind::FROM_CLAUSE
                    | SyntaxKind::USING_CLAUSE
                    | SyntaxKind::WHERE_CLAUSE
                    | SyntaxKind::RETURNING_CLAUSE
                    | SyntaxKind::INSERT_COLUMNS
                    | SyntaxKind::VALUES_CLAUSE
                    | SyntaxKind::SELECT_STMT
                    | SyntaxKind::ON_CONFLICT_CLAUSE
            ) {
                break;
            }

            continue;
        }

        let Some(token) = element.as_token() else {
            continue;
        };

        if !after_target || token.kind().is_trivia() {
            continue;
        }

        match token.kind() {
            SyntaxKind::AS_KW => saw_as = true,
            SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT => {
                if saw_as
                    || matches!(
                        parent.kind(),
                        SyntaxKind::UPDATE_STMT | SyntaxKind::DELETE_STMT | SyntaxKind::INSERT_STMT
                    )
                {
                    return Some(token.text().to_string());
                }
                break;
            }
            SyntaxKind::L_PAREN
            | SyntaxKind::VALUES_KW
            | SyntaxKind::SELECT_KW
            | SyntaxKind::DEFAULT_KW
            | SyntaxKind::SET_KW
            | SyntaxKind::FROM_KW
            | SyntaxKind::USING_KW
            | SyntaxKind::WHERE_KW
            | SyntaxKind::RETURNING_KW
            | SyntaxKind::ON_KW => break,
            _ => break,
        }
    }

    None
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct TestProvider {
        tables: HashMap<String, Vec<ColumnInfo>>,
    }

    impl TestProvider {
        fn with_patient_table() -> Self {
            let mut tables = HashMap::new();
            tables.insert(
                "patient".to_string(),
                vec![
                    ColumnInfo {
                        name: "id".to_string(),
                        data_type: DataType::Text,
                        nullable: false,
                        ordinal: 0,
                    },
                    ColumnInfo {
                        name: "resource".to_string(),
                        data_type: DataType::Jsonb,
                        nullable: false,
                        ordinal: 1,
                    },
                ],
            );
            Self { tables }
        }
    }

    impl SchemaProvider for TestProvider {
        fn lookup_table(&self, _schema: Option<&str>, name: &str) -> Option<TableInfo> {
            if self.tables.contains_key(&name.to_lowercase()) {
                Some(TableInfo {
                    schema: None,
                    name: name.to_string(),
                    table_type: TableType::Table,
                })
            } else {
                None
            }
        }

        fn lookup_columns(&self, _schema: Option<&str>, table: &str) -> Vec<ColumnInfo> {
            self.tables
                .get(&table.to_lowercase())
                .cloned()
                .unwrap_or_default()
        }

        fn all_table_names(&self) -> Vec<String> {
            self.tables.keys().cloned().collect()
        }
    }

    fn analyze_with_test_provider(sql: &str) -> Analysis {
        let parse = banshee_parser::parse(sql);
        let provider = TestProvider::with_patient_table();
        analyze_query(&parse, &provider)
    }

    fn analyze_with_test_provider_options(sql: &str, options: &AnalysisOptions) -> Analysis {
        let parse = banshee_parser::parse(sql);
        let provider = TestProvider::with_patient_table();
        analyze_query_with_options(&parse, &provider, options)
    }

    #[test]
    fn test_empty_analysis() {
        let analysis = Analysis::empty();
        assert!(!analysis.has_errors());
        assert_eq!(analysis.errors().count(), 0);
    }

    #[test]
    fn test_diagnostic_creation() {
        let error = Diagnostic::error("test error");
        assert_eq!(error.severity, Severity::Error);
        assert_eq!(error.message, "test error");

        let warning = Diagnostic::warning("test warning");
        assert_eq!(warning.severity, Severity::Warning);
    }

    #[test]
    fn test_analysis_with_errors() {
        let mut analysis = Analysis::empty();
        analysis.diagnostics.push(Diagnostic::error("error 1"));
        analysis.diagnostics.push(Diagnostic::warning("warning 1"));

        assert!(analysis.has_errors());
        assert_eq!(analysis.errors().count(), 1);
        assert_eq!(analysis.warnings().count(), 1);
    }

    #[test]
    fn test_cp01_keyword_case_with_fix() {
        let options =
            AnalysisOptions::new().with_builtin_lint_packs([BuiltinLintPack::Capitalisation]);
        let analysis = analyze_with_test_provider_options("select id from patient", &options);

        let cp01: Vec<_> = analysis
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(RuleCode::Cp01))
            .collect();
        // `select` and `from` are both lower case.
        assert_eq!(cp01.len(), 2, "{:?}", analysis.diagnostics);

        let select_diag = cp01
            .iter()
            .find(|d| d.message.contains("select"))
            .expect("diagnostic for select keyword");
        let fix = select_diag.fixes.first().expect("fix present");
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].new_text, "SELECT");
    }

    #[test]
    fn test_cp01_uppercase_is_clean() {
        let options =
            AnalysisOptions::new().with_builtin_lint_packs([BuiltinLintPack::Capitalisation]);
        let analysis = analyze_with_test_provider_options("SELECT id FROM patient", &options);
        assert!(
            analysis
                .diagnostics
                .iter()
                .all(|d| d.code != Some(RuleCode::Cp01))
        );
    }

    #[test]
    fn test_am05_implicit_cross_join() {
        let options = AnalysisOptions::new().with_builtin_lint_packs([BuiltinLintPack::Core]);
        let analysis =
            analyze_with_test_provider_options("SELECT * FROM patient, orders", &options);
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|d| d.code == Some(RuleCode::Am05)),
            "{:?}",
            analysis.diagnostics
        );
    }

    fn core_diagnostics(sql: &str) -> Vec<Diagnostic> {
        let options = AnalysisOptions::new().with_builtin_lint_packs([BuiltinLintPack::Core]);
        analyze_with_test_provider_options(sql, &options).diagnostics
    }

    #[test]
    fn test_am09_limit_without_order() {
        assert!(
            core_diagnostics("SELECT id FROM patient LIMIT 5")
                .iter()
                .any(|d| d.code == Some(RuleCode::Am09))
        );
        assert!(
            core_diagnostics("SELECT id FROM patient ORDER BY id LIMIT 5")
                .iter()
                .all(|d| d.code != Some(RuleCode::Am09))
        );
    }

    #[test]
    fn test_am02_set_op_modifier() {
        assert!(
            core_diagnostics("SELECT 1 UNION SELECT 2")
                .iter()
                .any(|d| d.code == Some(RuleCode::Am02))
        );
        assert!(
            core_diagnostics("SELECT 1 UNION ALL SELECT 2")
                .iter()
                .all(|d| d.code != Some(RuleCode::Am02))
        );
    }

    #[test]
    fn test_st01_redundant_else_null() {
        let diags = core_diagnostics("SELECT CASE WHEN a THEN 1 ELSE NULL END FROM t");
        let st01 = diags
            .iter()
            .find(|d| d.code == Some(RuleCode::St01))
            .expect("ST01");
        assert_eq!(st01.fixes[0].edits[0].new_text, "");
        // Non-null ELSE is fine.
        assert!(
            core_diagnostics("SELECT CASE WHEN a THEN 1 ELSE 0 END FROM t")
                .iter()
                .all(|d| d.code != Some(RuleCode::St01))
        );
    }

    #[test]
    fn test_al03_unaliased_expression() {
        let diags = core_diagnostics("SELECT count(*), id FROM t");
        assert!(diags.iter().any(|d| d.code == Some(RuleCode::Al03)));
        assert!(
            core_diagnostics("SELECT count(*) AS total FROM t")
                .iter()
                .all(|d| d.code != Some(RuleCode::Al03))
        );
    }

    #[test]
    fn test_al05_unused_alias() {
        assert!(
            core_diagnostics("SELECT id FROM users a")
                .iter()
                .any(|d| d.code == Some(RuleCode::Al05))
        );
        assert!(
            core_diagnostics("SELECT a.id FROM users a")
                .iter()
                .all(|d| d.code != Some(RuleCode::Al05))
        );
    }

    #[test]
    fn test_rf03_mixed_qualification() {
        assert!(
            core_diagnostics("SELECT a.id, name FROM users a")
                .iter()
                .any(|d| d.code == Some(RuleCode::Rf03))
        );
        // Multi-table query → RF03 does not apply.
        assert!(
            core_diagnostics("SELECT a.id, b.name FROM users a JOIN orders b ON a.id = b.id")
                .iter()
                .all(|d| d.code != Some(RuleCode::Rf03))
        );
    }

    #[test]
    fn test_st05_subquery_in_from() {
        assert!(
            core_diagnostics("SELECT * FROM (SELECT id FROM t) x")
                .iter()
                .any(|d| d.code == Some(RuleCode::St05))
        );
        assert!(
            core_diagnostics("SELECT * FROM t")
                .iter()
                .all(|d| d.code != Some(RuleCode::St05))
        );
    }

    #[test]
    fn test_st03_unused_cte() {
        let diags = core_diagnostics("WITH x AS (SELECT 1), y AS (SELECT 2) SELECT * FROM x");
        let st03: Vec<_> = diags
            .iter()
            .filter(|d| d.code == Some(RuleCode::St03))
            .collect();
        assert_eq!(st03.len(), 1, "{diags:?}");
        assert!(st03[0].message.contains("'y'"));
    }

    fn convention_diagnostics(sql: &str) -> Vec<Diagnostic> {
        let options = AnalysisOptions::new().with_builtin_lint_packs([BuiltinLintPack::Convention]);
        analyze_with_test_provider_options(sql, &options).diagnostics
    }

    #[test]
    fn test_cv05_null_comparison_fix() {
        let diags = convention_diagnostics("SELECT 1 FROM t WHERE a = NULL;");
        let cv05 = diags
            .iter()
            .find(|d| d.code == Some(RuleCode::Cv05))
            .expect("CV05");
        assert_eq!(cv05.fixes[0].edits[0].new_text, "IS");
    }

    #[test]
    fn test_cv01_ne_spelling_fix() {
        let diags = convention_diagnostics("SELECT 1 FROM t WHERE a != 1;");
        let cv01 = diags
            .iter()
            .find(|d| d.code == Some(RuleCode::Cv01))
            .expect("CV01");
        assert_eq!(cv01.fixes[0].edits[0].new_text, "<>");
    }

    #[test]
    fn test_cv01_skips_null_comparison() {
        let diags = convention_diagnostics("SELECT 1 FROM t WHERE a != NULL;");
        assert!(diags.iter().all(|d| d.code != Some(RuleCode::Cv01)));
        assert!(diags.iter().any(|d| d.code == Some(RuleCode::Cv05)));
    }

    #[test]
    fn test_cv06_missing_semicolon() {
        let with = convention_diagnostics("SELECT 1 FROM t;");
        assert!(with.iter().all(|d| d.code != Some(RuleCode::Cv06)));
        let without = convention_diagnostics("SELECT 1 FROM t");
        assert!(without.iter().any(|d| d.code == Some(RuleCode::Cv06)));
    }

    #[test]
    fn test_am04_expands_star_with_schema() {
        let options = AnalysisOptions::new().with_builtin_lint_packs([BuiltinLintPack::Core]);
        let analysis = analyze_with_test_provider_options("SELECT * FROM patient", &options);
        let am04 = analysis
            .diagnostics
            .iter()
            .find(|d| d.code == Some(RuleCode::Am04))
            .expect("AM04 diagnostic");
        let fix = am04.fixes.first().expect("AM04 expansion fix present");
        assert!(fix.edits[0].new_text.contains("id"), "{:?}", fix.edits);
    }

    #[test]
    fn test_am04_no_fix_for_multiple_tables() {
        let options = AnalysisOptions::new().with_builtin_lint_packs([BuiltinLintPack::Core]);
        let analysis =
            analyze_with_test_provider_options("SELECT * FROM patient, orders", &options);
        let am04 = analysis
            .diagnostics
            .iter()
            .find(|d| d.code == Some(RuleCode::Am04))
            .expect("AM04 diagnostic");
        assert!(am04.fixes.is_empty());
    }

    #[test]
    fn test_am05_explicit_join_is_clean() {
        let options = AnalysisOptions::new().with_builtin_lint_packs([BuiltinLintPack::Core]);
        let analysis = analyze_with_test_provider_options(
            "SELECT * FROM patient JOIN orders ON patient.id = orders.id",
            &options,
        );
        assert!(
            analysis
                .diagnostics
                .iter()
                .all(|d| d.code != Some(RuleCode::Am05))
        );
    }

    #[test]
    fn test_from_clause_analysis() {
        let tables = vec![
            TableBinding::table(None, "users".to_string()),
            TableBinding::table(None, "orders".to_string()),
        ];

        let scope = analyze_from_clause(tables, vec![], None);

        assert!(scope.resolve_table("users").ok().is_some());
        assert!(scope.resolve_table("orders").ok().is_some());
        assert!(scope.resolve_table("products").is_unresolved());
    }

    #[test]
    fn test_cte_analysis() {
        let ctes = vec![CteBinding::new("my_cte".to_string())];

        let scope = analyze_from_clause(vec![], ctes, None);

        assert!(scope.resolve_cte("my_cte").ok().is_some());
        // CTE is also available as a table
        assert!(scope.resolve_table("my_cte").ok().is_some());
    }

    #[test]
    fn test_expand_star() {
        let scope = ScopeBuilder::new()
            .add_table(
                TableBinding::table(None, "users".to_string()).with_columns(vec![
                    ColumnBinding::new("id".to_string(), 0),
                    ColumnBinding::new("name".to_string(), 1),
                ]),
            )
            .add_table(
                TableBinding::table(None, "orders".to_string())
                    .with_columns(vec![ColumnBinding::new("order_id".to_string(), 0)]),
            )
            .build();

        let columns = expand_star(&scope);
        assert_eq!(columns.len(), 3);
    }

    #[test]
    fn test_expand_table_star() {
        let scope = ScopeBuilder::new()
            .add_table(
                TableBinding::table(None, "users".to_string()).with_columns(vec![
                    ColumnBinding::new("id".to_string(), 0),
                    ColumnBinding::new("name".to_string(), 1),
                ]),
            )
            .build();

        let columns = expand_table_star("users", &scope, 0);
        let cols = columns.ok().unwrap();
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0].name, "id");
        assert_eq!(cols[1].name, "name");
    }

    #[test]
    fn test_analyzer_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Analysis>();
        assert_send_sync::<Diagnostic>();
    }

    #[test]
    fn test_suggest_similar_prefers_prefix_matches() {
        // The user types 'pa' - should suggest 'patient' first, not 'app'
        let tables = vec![
            "app".to_string(),
            "goal".to_string(),
            "flag".to_string(),
            "patient".to_string(),
            "practitioner".to_string(),
        ];

        let suggestions = suggest_similar("pa", &tables, 3);

        // patient and practitioner should come first (prefix matches)
        assert!(!suggestions.is_empty());
        assert!(
            suggestions[0] == "patient" || suggestions[0] == "practitioner",
            "Expected prefix match first, got: {:?}",
            suggestions
        );
    }

    #[test]
    fn test_suggest_similar_levenshtein_fallback() {
        // When no prefix matches, use levenshtein distance
        let tables = vec![
            "users".to_string(),
            "orders".to_string(),
            "products".to_string(),
        ];

        let suggestions = suggest_similar("usrs", &tables, 3);

        // 'users' is closest by levenshtein (distance 1)
        assert_eq!(suggestions.first(), Some(&"users".to_string()));
    }

    #[test]
    fn test_suggest_similar_case_insensitive() {
        let tables = vec!["Patient".to_string(), "Observation".to_string()];

        let suggestions = suggest_similar("pa", &tables, 3);

        assert_eq!(suggestions.first(), Some(&"Patient".to_string()));
    }

    #[test]
    fn test_lint_select_star() {
        let analysis = analyze_with_test_provider("SELECT * FROM patient");
        assert!(
            analysis
                .warnings()
                .any(|d| d.message.contains("Avoid SELECT *")),
            "expected SELECT * warning, got: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn test_lint_update_without_where() {
        let analysis = analyze_with_test_provider("UPDATE patient SET id = '1'");
        assert!(
            analysis
                .warnings()
                .any(|d| d.message.contains("UPDATE without WHERE")),
            "expected UPDATE without WHERE warning, got: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn test_lint_delete_without_where() {
        let analysis = analyze_with_test_provider("DELETE FROM patient");
        assert!(
            analysis
                .warnings()
                .any(|d| d.message.contains("DELETE without WHERE")),
            "expected DELETE without WHERE warning, got: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn test_lint_jsonb_text_compare() {
        let analysis = analyze_with_test_provider(
            "SELECT resource->'id' FROM patient WHERE resource->'id' = 'abc'",
        );
        assert!(
            analysis
                .warnings()
                .any(|d| d.message.contains("Use ->> when comparing JSONB scalar")),
            "expected JSONB text compare warning, got: {:?}",
            analysis.diagnostics
        );

        // The JB01 finding carries an autofix replacing `->` with `->>`.
        let diag = analysis
            .diagnostics
            .iter()
            .find(|d| d.code == Some(RuleCode::Jb01))
            .expect("JB01 diagnostic");
        let fix = diag.fixes.first().expect("JB01 fix present");
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].new_text, "->>");
    }

    #[test]
    fn test_lint_jsonb_text_compare_not_triggered_for_arrow_text() {
        let analysis = analyze_with_test_provider(
            "SELECT resource->>'id' FROM patient WHERE resource->>'id' = 'abc'",
        );
        assert!(
            analysis
                .warnings()
                .all(|d| !d.message.contains("Use ->> when comparing JSONB scalar")),
            "did not expect JSONB text compare warning, got: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn test_lint_pack_core_without_jsonb() {
        let options = AnalysisOptions::new().with_builtin_lint_packs([BuiltinLintPack::Core]);
        let analysis = analyze_with_test_provider_options(
            "SELECT resource->'id' FROM patient WHERE resource->'id' = 'abc'",
            &options,
        );
        assert!(
            analysis
                .warnings()
                .all(|d| !d.message.contains("Use ->> when comparing JSONB scalar")),
            "did not expect JSONB lint from core pack, got: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn test_lint_pack_jsonb_without_core() {
        let options = AnalysisOptions::new().with_builtin_lint_packs([BuiltinLintPack::Jsonb]);
        let analysis = analyze_with_test_provider_options("SELECT * FROM patient", &options);
        assert!(
            analysis
                .warnings()
                .all(|d| !d.message.contains("Avoid SELECT *")),
            "did not expect core lint from jsonb pack, got: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn test_external_lint_pack_hook() {
        struct ExternalPack;
        impl LintRulePack for ExternalPack {
            fn apply(&self, root: &banshee_syntax::SyntaxNode, analyzer: &mut Analyzer<'_>) {
                analyzer.warning("External lint pack executed", Some(root.text_range()));
            }
        }

        let options = AnalysisOptions::new()
            .with_builtin_lint_packs(std::iter::empty())
            .with_external_lint_pack(Arc::new(ExternalPack));
        let analysis = analyze_with_test_provider_options("SELECT id FROM patient", &options);
        assert!(
            analysis
                .warnings()
                .any(|d| d.message.contains("External lint pack executed")),
            "expected warning from external lint pack, got: {:?}",
            analysis.diagnostics
        );
    }
}
