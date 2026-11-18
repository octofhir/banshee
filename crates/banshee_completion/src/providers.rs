//! Provider traits for completion data sources.
//!
//! Providers supply schema information for completion. They are optional -
//! completion works with just syntax if no providers are available.

use crate::types::{ColumnInfo, FunctionInfo, JsonbSchema, TableInfo};

/// Provider for database schema information.
///
/// Implement this trait to supply table and column information to the
/// completion engine.
pub trait SchemaProvider: Send + Sync {
    /// Returns all tables visible in the database.
    fn tables(&self) -> Vec<TableInfo>;

    /// Returns tables in a specific schema.
    fn tables_in_schema(&self, schema: &str) -> Vec<TableInfo> {
        self.tables()
            .into_iter()
            .filter(|t| t.schema.as_deref() == Some(schema))
            .collect()
    }

    /// Returns tables matching a prefix.
    fn tables_by_prefix(&self, prefix: &str) -> Vec<TableInfo> {
        let prefix_lower = prefix.to_lowercase();
        self.tables()
            .into_iter()
            .filter(|t| t.name.to_lowercase().starts_with(&prefix_lower))
            .collect()
    }

    /// Looks up a table by name.
    fn table(&self, schema: Option<&str>, name: &str) -> Option<TableInfo> {
        let name_lower = name.to_lowercase();
        self.tables().into_iter().find(|t| {
            t.name.to_lowercase() == name_lower
                && (schema.is_none() || t.schema.as_deref() == schema)
        })
    }

    /// Returns all columns for a table.
    fn columns(&self, schema: Option<&str>, table: &str) -> Vec<ColumnInfo>;

    /// Returns columns matching a prefix.
    fn columns_by_prefix(
        &self,
        schema: Option<&str>,
        table: &str,
        prefix: &str,
    ) -> Vec<ColumnInfo> {
        let prefix_lower = prefix.to_lowercase();
        self.columns(schema, table)
            .into_iter()
            .filter(|c| c.name.to_lowercase().starts_with(&prefix_lower))
            .collect()
    }

    /// Returns the JSONB schema for a column, if available.
    fn jsonb_schema(&self, schema: Option<&str>, table: &str, column: &str) -> Option<JsonbSchema>;

    /// Returns all available schemas.
    fn schemas(&self) -> Vec<String> {
        let mut schemas: Vec<String> = self.tables().into_iter().filter_map(|t| t.schema).collect();
        schemas.sort();
        schemas.dedup();
        schemas
    }
}

/// Provider for function information.
///
/// Implement this trait to supply function information to the
/// completion engine.
pub trait FunctionProvider: Send + Sync {
    /// Returns all available functions.
    fn functions(&self) -> Vec<FunctionInfo>;

    /// Returns functions matching a prefix.
    fn functions_by_prefix(&self, prefix: &str) -> Vec<FunctionInfo> {
        let prefix_lower = prefix.to_lowercase();
        self.functions()
            .into_iter()
            .filter(|f| f.name.to_lowercase().starts_with(&prefix_lower))
            .collect()
    }

    /// Looks up a function by name.
    fn function(&self, schema: Option<&str>, name: &str) -> Option<FunctionInfo> {
        let name_lower = name.to_lowercase();
        self.functions().into_iter().find(|f| {
            f.name.to_lowercase() == name_lower
                && (schema.is_none() || f.schema.as_deref() == schema)
        })
    }

    /// Returns the known signatures for an already-normalized function name.
    /// Quoted names are matched exactly, like a lookup in the server catalog.
    fn overloads(&self, schema: Option<&str>, name: &str) -> Vec<FunctionInfo> {
        self.functions()
            .into_iter()
            .filter(|f| f.name == name && (schema.is_none() || f.schema.as_deref() == schema))
            .collect()
    }

    /// Returns functions in a specific schema.
    fn functions_in_schema(&self, schema: &str) -> Vec<FunctionInfo> {
        self.functions()
            .into_iter()
            .filter(|f| f.schema.as_deref() == Some(schema))
            .collect()
    }
}

/// A null schema provider that returns no information.
#[derive(Clone, Debug, Default)]
pub struct NullSchemaProvider;

impl SchemaProvider for NullSchemaProvider {
    fn tables(&self) -> Vec<TableInfo> {
        Vec::new()
    }

    fn columns(&self, _schema: Option<&str>, _table: &str) -> Vec<ColumnInfo> {
        Vec::new()
    }

    fn jsonb_schema(
        &self,
        _schema: Option<&str>,
        _table: &str,
        _column: &str,
    ) -> Option<JsonbSchema> {
        None
    }
}

/// A null function provider that returns no information.
#[derive(Clone, Debug, Default)]
pub struct NullFunctionProvider;

impl FunctionProvider for NullFunctionProvider {
    fn functions(&self) -> Vec<FunctionInfo> {
        Vec::new()
    }
}

/// A schema provider backed by in-memory data.
#[derive(Clone, Debug, Default)]
pub struct MemorySchemaProvider {
    tables: Vec<TableInfo>,
    columns: Vec<(Option<String>, String, Vec<ColumnInfo>)>,
    jsonb_schemas: Vec<(Option<String>, String, String, JsonbSchema)>,
}

impl MemorySchemaProvider {
    /// Creates a new empty memory schema provider.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a table to the provider.
    pub fn add_table(mut self, table: TableInfo) -> Self {
        self.tables.push(table);
        self
    }

    /// Adds columns for a table.
    pub fn add_columns(
        mut self,
        schema: Option<String>,
        table: impl Into<String>,
        columns: Vec<ColumnInfo>,
    ) -> Self {
        self.columns.push((schema, table.into(), columns));
        self
    }

    /// Adds a JSONB schema for a column.
    pub fn add_jsonb_schema(
        mut self,
        schema: Option<String>,
        table: impl Into<String>,
        column: impl Into<String>,
        jsonb_schema: JsonbSchema,
    ) -> Self {
        self.jsonb_schemas
            .push((schema, table.into(), column.into(), jsonb_schema));
        self
    }
}

impl SchemaProvider for MemorySchemaProvider {
    fn tables(&self) -> Vec<TableInfo> {
        self.tables.clone()
    }

    fn columns(&self, schema: Option<&str>, table: &str) -> Vec<ColumnInfo> {
        let table_lower = table.to_lowercase();
        for (s, t, cols) in &self.columns {
            if t.to_lowercase() == table_lower && s.as_deref() == schema {
                return cols.clone();
            }
        }
        Vec::new()
    }

    fn jsonb_schema(&self, schema: Option<&str>, table: &str, column: &str) -> Option<JsonbSchema> {
        let table_lower = table.to_lowercase();
        let column_lower = column.to_lowercase();
        for (s, t, c, js) in &self.jsonb_schemas {
            if t.to_lowercase() == table_lower
                && c.to_lowercase() == column_lower
                && s.as_deref() == schema
            {
                return Some(js.clone());
            }
        }
        None
    }
}

/// A function provider backed by in-memory data.
#[derive(Clone, Debug, Default)]
pub struct MemoryFunctionProvider {
    functions: Vec<FunctionInfo>,
}

impl MemoryFunctionProvider {
    /// Creates a new empty memory function provider.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a function to the provider.
    pub fn add_function(mut self, function: FunctionInfo) -> Self {
        self.functions.push(function);
        self
    }
}

impl FunctionProvider for MemoryFunctionProvider {
    fn functions(&self) -> Vec<FunctionInfo> {
        self.functions.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_null_providers() {
        let schema = NullSchemaProvider;
        assert!(schema.tables().is_empty());
        assert!(schema.columns(None, "users").is_empty());
        assert!(schema.jsonb_schema(None, "users", "data").is_none());

        let funcs = NullFunctionProvider;
        assert!(funcs.functions().is_empty());
    }

    #[test]
    fn test_memory_schema_provider() {
        let provider = MemorySchemaProvider::new()
            .add_table(TableInfo::new("users").with_schema("public"))
            .add_table(TableInfo::new("orders").with_schema("public"))
            .add_columns(
                Some("public".to_string()),
                "users",
                vec![
                    ColumnInfo::new("id", "integer"),
                    ColumnInfo::new("name", "text"),
                ],
            );

        assert_eq!(provider.tables().len(), 2);
        assert_eq!(provider.tables_in_schema("public").len(), 2);
        assert_eq!(provider.tables_by_prefix("use").len(), 1);

        let columns = provider.columns(Some("public"), "users");
        assert_eq!(columns.len(), 2);
    }

    #[test]
    fn test_memory_function_provider() {
        let provider = MemoryFunctionProvider::new()
            .add_function(FunctionInfo::new("jsonb_extract_path", "jsonb"))
            .add_function(FunctionInfo::new("jsonb_build_object", "jsonb"));

        assert_eq!(provider.functions().len(), 2);
        assert_eq!(provider.functions_by_prefix("jsonb_e").len(), 1);
        assert!(provider.function(None, "jsonb_extract_path").is_some());
    }

    #[test]
    fn test_providers_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<NullSchemaProvider>();
        assert_send_sync::<NullFunctionProvider>();
        assert_send_sync::<MemorySchemaProvider>();
        assert_send_sync::<MemoryFunctionProvider>();
    }
}
