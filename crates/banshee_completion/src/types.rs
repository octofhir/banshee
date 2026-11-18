//! Data types for completion.
//!
//! These types are decoupled from LSP - applications transform them
//! to LSP types as needed.

use std::borrow::Cow;

pub(crate) fn display_identifier(name: &str) -> Cow<'_, str> {
    let mut bytes = name.bytes();
    if bytes
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b == b'_')
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'$'))
        && !banshee_syntax::keyword_from_str(name).is_some_and(|kind| kind.is_reserved_keyword())
    {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(format!("\"{}\"", name.replace('"', "\"\"")))
    }
}

/// Information about a database table.
#[must_use]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableInfo {
    /// The schema name, if any.
    pub schema: Option<String>,

    /// The table name.
    pub name: String,

    /// The type of table.
    pub table_type: TableType,

    /// Description/comment for the table.
    pub description: Option<String>,
}

impl TableInfo {
    /// Creates a new table info.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            schema: None,
            name: name.into(),
            table_type: TableType::Table,
            description: None,
        }
    }

    /// Sets the schema.
    pub fn with_schema(mut self, schema: impl Into<String>) -> Self {
        self.schema = Some(schema.into());
        self
    }

    /// Sets the table type.
    pub fn with_type(mut self, table_type: TableType) -> Self {
        self.table_type = table_type;
        self
    }

    /// Sets the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Returns the fully qualified name.
    pub fn qualified_name(&self) -> String {
        match &self.schema {
            Some(s) => format!("{}.{}", s, self.name),
            None => self.name.clone(),
        }
    }
}

/// Type of a database table.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TableType {
    Table,
    View,
    MaterializedView,
    ForeignTable,
    Cte,
}

/// Information about a database column.
#[must_use]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnInfo {
    /// The column name.
    pub name: String,

    /// The data type as a string.
    pub data_type: String,

    /// Whether the column is nullable.
    pub nullable: bool,

    /// Whether this is a primary key column.
    pub is_primary_key: bool,

    /// Whether this column has a default value.
    pub has_default: bool,

    /// Description/comment for the column.
    pub description: Option<String>,

    /// The ordinal position (0-indexed).
    pub ordinal: usize,
}

impl ColumnInfo {
    /// Creates a new column info.
    pub fn new(name: impl Into<String>, data_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            data_type: data_type.into(),
            nullable: true,
            is_primary_key: false,
            has_default: false,
            description: None,
            ordinal: 0,
        }
    }

    /// Sets whether the column is nullable.
    pub fn with_nullable(mut self, nullable: bool) -> Self {
        self.nullable = nullable;
        self
    }

    /// Sets whether this is a primary key.
    pub fn with_primary_key(mut self, is_pk: bool) -> Self {
        self.is_primary_key = is_pk;
        self
    }

    /// Sets the ordinal position.
    pub fn with_ordinal(mut self, ordinal: usize) -> Self {
        self.ordinal = ordinal;
        self
    }

    /// Sets the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Information about a database function.
#[must_use]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionInfo {
    /// The schema name, if any.
    pub schema: Option<String>,

    /// The function name.
    pub name: String,

    /// The function arguments.
    pub args: Vec<FunctionArg>,

    /// The return type.
    pub return_type: String,

    /// Whether this function returns a set (table function).
    pub returns_set: bool,

    /// Description/comment for the function.
    pub description: Option<String>,

    /// Function volatility.
    pub volatility: Volatility,
}

impl FunctionInfo {
    /// Creates a new function info.
    pub fn new(name: impl Into<String>, return_type: impl Into<String>) -> Self {
        Self {
            schema: None,
            name: name.into(),
            args: Vec::new(),
            return_type: return_type.into(),
            returns_set: false,
            description: None,
            volatility: Volatility::Volatile,
        }
    }

    /// Sets the schema.
    pub fn with_schema(mut self, schema: impl Into<String>) -> Self {
        self.schema = Some(schema.into());
        self
    }

    /// Adds an argument.
    pub fn with_arg(mut self, arg: FunctionArg) -> Self {
        self.args.push(arg);
        self
    }

    /// Sets the arguments.
    pub fn with_args(mut self, args: Vec<FunctionArg>) -> Self {
        self.args = args;
        self
    }

    /// Sets whether the function returns a set.
    pub fn with_returns_set(mut self, returns_set: bool) -> Self {
        self.returns_set = returns_set;
        self
    }

    /// Sets the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the volatility.
    pub fn with_volatility(mut self, volatility: Volatility) -> Self {
        self.volatility = volatility;
        self
    }

    /// Returns the function signature as a string.
    pub fn signature(&self) -> String {
        let args: Vec<_> = self.args.iter().map(FunctionArg::label).collect();
        let name = if self.schema.is_some() {
            display_identifier(&self.name)
        } else {
            Cow::Borrowed(self.name.as_str())
        };
        format!("{name}({}) -> {}", args.join(", "), self.return_type)
    }
}

/// A function argument.
#[must_use]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionArg {
    /// The argument name, if any.
    pub name: Option<String>,

    /// The data type.
    pub data_type: String,

    /// The argument mode.
    pub mode: ArgMode,

    /// Default value expression, if any.
    pub default: Option<String>,
}

impl FunctionArg {
    /// Creates a new function argument.
    pub fn new(data_type: impl Into<String>) -> Self {
        Self {
            name: None,
            data_type: data_type.into(),
            mode: ArgMode::In,
            default: None,
        }
    }

    /// Sets the argument name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the argument mode.
    pub fn with_mode(mut self, mode: ArgMode) -> Self {
        self.mode = mode;
        self
    }

    /// Sets the default value.
    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default = Some(default.into());
        self
    }

    /// Returns the argument as displayed in signature help.
    pub fn label(&self) -> String {
        let mut label = String::new();
        label.push_str(match self.mode {
            ArgMode::In => "",
            ArgMode::Out => "OUT ",
            ArgMode::InOut => "INOUT ",
            ArgMode::Variadic => "VARIADIC ",
        });
        if let Some(name) = &self.name {
            label.push_str(&display_identifier(name));
            label.push(' ');
        }
        label.push_str(&self.data_type);
        if let Some(default) = &self.default {
            label.push_str(" = ");
            label.push_str(default);
        }
        label
    }
}

/// Function argument mode.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArgMode {
    In,
    Out,
    InOut,
    Variadic,
}

/// Function volatility category.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Volatility {
    Immutable,
    Stable,
    Volatile,
}

/// Schema information for a JSONB column.
#[must_use]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonbSchema {
    /// Known fields at the top level.
    pub fields: Vec<JsonbField>,
}

impl JsonbSchema {
    /// Creates a new empty JSONB schema.
    pub fn new() -> Self {
        Self { fields: Vec::new() }
    }

    /// Adds a field to the schema.
    pub fn with_field(mut self, field: JsonbField) -> Self {
        self.fields.push(field);
        self
    }

    /// Finds a field by name.
    pub fn find_field(&self, name: &str) -> Option<&JsonbField> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Returns fields at a given path.
    /// Numeric segments (array indices like "0", "1") are skipped as they don't affect field structure.
    pub fn fields_at_path(&self, path: &[String]) -> Vec<&JsonbField> {
        if path.is_empty() {
            return self.fields.iter().collect();
        }

        let mut current_fields = &self.fields;
        for segment in path {
            // Skip numeric segments (array indices like "0", "1")
            if segment.parse::<i32>().is_ok() {
                continue;
            }

            if let Some(field) = current_fields.iter().find(|f| &f.name == segment) {
                if let Some(ref nested) = field.nested {
                    current_fields = &nested.fields;
                } else {
                    return Vec::new();
                }
            } else {
                return Vec::new();
            }
        }
        current_fields.iter().collect()
    }
}

impl Default for JsonbSchema {
    fn default() -> Self {
        Self::new()
    }
}

/// A field in a JSONB schema.
#[must_use]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonbField {
    /// The field name.
    pub name: String,

    /// The expected type of this field.
    pub field_type: JsonbFieldType,

    /// Optional semantic/domain namespace for editor hints (e.g. "FHIR").
    pub semantic_namespace: Option<String>,

    /// Optional semantic/domain type for editor hints (e.g. "HumanName[]").
    pub semantic_type: Option<String>,

    /// Nested schema for object fields.
    pub nested: Option<Box<JsonbSchema>>,

    /// Description of the field.
    pub description: Option<String>,
}

impl JsonbField {
    /// Creates a new JSONB field.
    pub fn new(name: impl Into<String>, field_type: JsonbFieldType) -> Self {
        Self {
            name: name.into(),
            field_type,
            semantic_namespace: None,
            semantic_type: None,
            nested: None,
            description: None,
        }
    }

    /// Sets semantic/domain namespace metadata.
    pub fn with_semantic_namespace(mut self, semantic_namespace: impl Into<String>) -> Self {
        self.semantic_namespace = Some(semantic_namespace.into());
        self
    }

    /// Sets semantic/domain type metadata.
    pub fn with_semantic_type(mut self, semantic_type: impl Into<String>) -> Self {
        self.semantic_type = Some(semantic_type.into());
        self
    }

    /// Sets semantic/domain namespace and type metadata together.
    pub fn with_semantic_hint(
        mut self,
        semantic_namespace: impl Into<String>,
        semantic_type: impl Into<String>,
    ) -> Self {
        self.semantic_namespace = Some(semantic_namespace.into());
        self.semantic_type = Some(semantic_type.into());
        self
    }

    /// Sets nested schema for object fields.
    pub fn with_nested(mut self, schema: JsonbSchema) -> Self {
        self.nested = Some(Box::new(schema));
        self
    }

    /// Sets the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Type of a JSONB field.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JsonbFieldType {
    Object,
    Array,
    String,
    Number,
    Boolean,
    Null,
    Unknown,
}

/// A completion item returned by the completion engine.
#[must_use]
#[derive(Clone, Debug)]
pub struct CompletionItem {
    /// The kind of completion item.
    pub kind: CompletionItemKind,

    /// The label shown to the user.
    pub label: String,

    /// Additional detail shown after the label.
    pub detail: Option<String>,

    /// Documentation for the item.
    pub documentation: Option<String>,

    /// Text to insert (if different from label).
    pub insert_text: Option<String>,

    /// Sort key for ordering (if different from label).
    pub sort_key: Option<String>,

    /// Filter text for fuzzy matching (if different from label).
    pub filter_text: Option<String>,

    /// Application-specific data.
    pub data: CompletionData,
}

impl CompletionItem {
    /// Creates a new completion item.
    pub fn new(kind: CompletionItemKind, label: impl Into<String>) -> Self {
        Self {
            kind,
            label: label.into(),
            detail: None,
            documentation: None,
            insert_text: None,
            sort_key: None,
            filter_text: None,
            data: CompletionData::None,
        }
    }

    /// Sets the detail.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Sets the documentation.
    pub fn with_documentation(mut self, doc: impl Into<String>) -> Self {
        self.documentation = Some(doc.into());
        self
    }

    /// Sets the insert text.
    pub fn with_insert_text(mut self, text: impl Into<String>) -> Self {
        self.insert_text = Some(text.into());
        self
    }

    /// Sets the sort key.
    pub fn with_sort_key(mut self, key: impl Into<String>) -> Self {
        self.sort_key = Some(key.into());
        self
    }

    /// Sets the filter text.
    pub fn with_filter_text(mut self, text: impl Into<String>) -> Self {
        self.filter_text = Some(text.into());
        self
    }

    /// Sets the data.
    pub fn with_data(mut self, data: CompletionData) -> Self {
        self.data = data;
        self
    }

    /// Returns the text to use for filtering.
    pub fn effective_filter_text(&self) -> &str {
        self.filter_text.as_deref().unwrap_or(&self.label)
    }

    /// Returns the text to use for sorting.
    pub fn effective_sort_key(&self) -> &str {
        self.sort_key.as_deref().unwrap_or(&self.label)
    }
}

/// Kind of a completion item.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompletionItemKind {
    Table,
    View,
    Column,
    Function,
    Keyword,
    Alias,
    Schema,
    JsonbPath,
    Snippet,
    Type,
    Operator,
}

/// Application-specific data attached to a completion item.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum CompletionData {
    /// No additional data.
    None,

    /// Table reference data.
    Table {
        schema: Option<String>,
        name: String,
    },

    /// Column reference data.
    Column {
        table: Option<String>,
        name: String,
        data_type: Option<String>,
    },

    /// Function reference data.
    Function {
        schema: Option<String>,
        name: String,
        signature: Option<String>,
    },

    /// JSONB path data.
    JsonbPath {
        base_column: String,
        path: Vec<String>,
    },
}

/// Result of a completion request.
#[must_use]
#[derive(Clone, Debug)]
pub struct CompletionResult {
    /// The detected completion context.
    pub context: CompletionContext,

    /// The completion items.
    pub items: Vec<CompletionItem>,

    /// Whether the result is incomplete (more items available).
    pub is_incomplete: bool,
}

impl CompletionResult {
    /// Creates a new completion result.
    pub fn new(context: CompletionContext) -> Self {
        Self {
            context,
            items: Vec::new(),
            is_incomplete: false,
        }
    }

    /// Adds items to the result.
    pub fn with_items(mut self, items: Vec<CompletionItem>) -> Self {
        self.items = items;
        self
    }

    /// Marks the result as incomplete.
    pub fn incomplete(mut self) -> Self {
        self.is_incomplete = true;
        self
    }
}

/// The context in which completion was triggered.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum CompletionContext {
    /// At the start of a statement.
    Statement,

    /// In a SELECT column list.
    SelectColumn {
        /// Tables available in scope.
        tables: Vec<String>,
    },

    /// Expecting a table name.
    TableName {
        /// Schema prefix, if any.
        schema: Option<String>,
    },

    /// A column reference.
    ColumnRef {
        /// Table qualifier, if specified.
        table: Option<String>,
        /// Tables available in scope.
        tables: Vec<String>,
    },

    /// In a JOIN condition.
    JoinCondition {
        /// Left table in the join.
        left_table: String,
        /// Right table in the join.
        right_table: String,
    },

    /// In a WHERE clause condition.
    WhereCondition {
        /// Tables available in scope.
        tables: Vec<String>,
    },

    /// JSONB field access.
    JsonbField {
        /// The table name (if qualified, e.g., `patient` in `patient.resource`).
        table: Option<String>,
        /// The base column name.
        column: String,
        /// The current access path.
        path: Vec<String>,
    },

    /// JSONPath expression.
    JsonPath {
        /// Current path being typed.
        current_path: String,
    },

    /// Function argument.
    FunctionArg {
        /// The function name.
        function: String,
        /// The argument index (0-based).
        arg_index: usize,
    },

    /// Type name expected.
    TypeName,

    /// Keyword expected.
    Keyword {
        /// Expected keyword kinds.
        expected: Vec<String>,
    },

    /// Inside OVER clause, before any keywords.
    WindowClause {
        /// Tables in scope for column references.
        tables: Vec<String>,
    },

    /// After PARTITION BY or ORDER BY inside OVER.
    WindowPartition {
        /// Tables in scope for column references.
        tables: Vec<String>,
    },

    /// After ORDER BY column, expecting sort direction.
    WindowOrderDirection,

    /// Inside frame specification (ROWS/RANGE BETWEEN).
    WindowFrame,

    /// Unknown context (fallback to keyword completion).
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_info() {
        let table = TableInfo::new("users")
            .with_schema("public")
            .with_type(TableType::Table)
            .with_description("User accounts");

        assert_eq!(table.qualified_name(), "public.users");
        assert_eq!(table.description.as_deref(), Some("User accounts"));
    }

    #[test]
    fn test_column_info() {
        let col = ColumnInfo::new("id", "integer")
            .with_nullable(false)
            .with_primary_key(true)
            .with_ordinal(0);

        assert_eq!(col.name, "id");
        assert!(!col.nullable);
        assert!(col.is_primary_key);
    }

    #[test]
    fn test_function_signature() {
        let func = FunctionInfo::new("jsonb_extract_path", "jsonb")
            .with_arg(FunctionArg::new("jsonb").with_name("from_json"))
            .with_arg(
                FunctionArg::new("text")
                    .with_name("path")
                    .with_mode(ArgMode::Variadic),
            );

        let sig = func.signature();
        assert!(sig.contains("jsonb_extract_path"));
        assert!(sig.contains("from_json"));
    }

    #[test]
    fn test_jsonb_schema() {
        let schema = JsonbSchema::new()
            .with_field(JsonbField::new("name", JsonbFieldType::String))
            .with_field(
                JsonbField::new("address", JsonbFieldType::Object).with_nested(
                    JsonbSchema::new()
                        .with_field(JsonbField::new("street", JsonbFieldType::String))
                        .with_field(JsonbField::new("city", JsonbFieldType::String)),
                ),
            );

        assert!(schema.find_field("name").is_some());
        assert!(schema.find_field("address").is_some());

        let address_fields = schema.fields_at_path(&["address".to_string()]);
        assert_eq!(address_fields.len(), 2);
    }

    #[test]
    fn test_completion_item() {
        let item = CompletionItem::new(CompletionItemKind::Table, "users")
            .with_detail("public.users")
            .with_insert_text("users")
            .with_sort_key("0_users");

        assert_eq!(item.kind, CompletionItemKind::Table);
        assert_eq!(item.label, "users");
        assert_eq!(item.effective_sort_key(), "0_users");
    }

    #[test]
    fn test_types_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<TableInfo>();
        assert_send_sync::<TableType>();
        assert_send_sync::<ColumnInfo>();
        assert_send_sync::<FunctionInfo>();
        assert_send_sync::<FunctionArg>();
        assert_send_sync::<ArgMode>();
        assert_send_sync::<Volatility>();
        assert_send_sync::<JsonbSchema>();
        assert_send_sync::<JsonbField>();
        assert_send_sync::<JsonbFieldType>();
        assert_send_sync::<CompletionItem>();
        assert_send_sync::<CompletionItemKind>();
        assert_send_sync::<CompletionData>();
        assert_send_sync::<CompletionResult>();
        assert_send_sync::<CompletionContext>();
    }
}
