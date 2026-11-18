//! A schema provider backed by a [`SchemaSnapshot`].
//!
//! Implements both the analyzer's `banshee_hir::SchemaProvider` and the completion
//! engine's `banshee_completion::providers::SchemaProvider` / `FunctionProvider`,
//! so a single cached snapshot drives linting, name resolution and completion.

use banshee_completion::providers::{FunctionProvider, SchemaProvider as CompletionSchemaProvider};
use banshee_completion::types::{
    ArgMode, ColumnInfo as CompletionColumn, FunctionArg, FunctionInfo, JsonbField, JsonbFieldType,
    JsonbSchema, TableInfo as CompletionTable,
};
use banshee_hir::{
    ColumnInfo as HirColumn, DataType, SchemaProvider as HirSchemaProvider, TableInfo as HirTable,
    TableType as HirTableType,
};

use crate::snapshot::{
    FunctionEntry, FunctionParameterMode, JsonbShape, JsonbType, SchemaSnapshot, TableEntry,
    TableKind,
};

/// A schema provider reading from an in-memory snapshot.
#[derive(Debug, Clone)]
pub struct CachedSchemaProvider {
    snapshot: SchemaSnapshot,
}

impl CachedSchemaProvider {
    /// Wraps a snapshot.
    pub fn new(snapshot: SchemaSnapshot) -> Self {
        Self { snapshot }
    }

    /// The underlying snapshot.
    pub fn snapshot(&self) -> &SchemaSnapshot {
        &self.snapshot
    }

    /// Finds a table, honoring the snapshot's default schema when `schema` is
    /// `None`. Table matching is case-insensitive (Postgres folds unquoted
    /// identifiers to lower case).
    fn find_table(&self, schema: Option<&str>, name: &str) -> Option<&TableEntry> {
        let want_schema = schema.unwrap_or(&self.snapshot.default_schema);
        self.snapshot.tables.iter().find(|t| {
            t.name.eq_ignore_ascii_case(name) && t.schema.eq_ignore_ascii_case(want_schema)
        })
    }
}

fn hir_table_type(kind: TableKind) -> HirTableType {
    match kind {
        TableKind::Table => HirTableType::Table,
        TableKind::View => HirTableType::View,
        TableKind::MaterializedView => HirTableType::MaterializedView,
        TableKind::ForeignTable => HirTableType::ForeignTable,
    }
}

// ---- analyzer (banshee_hir) provider -----------------------------------------

impl HirSchemaProvider for CachedSchemaProvider {
    fn lookup_table(&self, schema: Option<&str>, name: &str) -> Option<HirTable> {
        self.find_table(schema, name).map(|t| HirTable {
            schema: Some(t.schema.clone()),
            name: t.name.clone(),
            table_type: hir_table_type(t.kind),
        })
    }

    fn lookup_columns(&self, schema: Option<&str>, table: &str) -> Vec<HirColumn> {
        let Some(t) = self.find_table(schema, table) else {
            return Vec::new();
        };
        t.columns
            .iter()
            .map(|c| HirColumn {
                name: c.name.clone(),
                data_type: DataType::Custom(c.data_type.clone()),
                nullable: c.nullable,
                ordinal: c.ordinal,
            })
            .collect()
    }

    fn schema_exists(&self, schema: &str) -> bool {
        self.snapshot
            .tables
            .iter()
            .any(|t| t.schema.eq_ignore_ascii_case(schema))
    }

    fn all_table_names(&self) -> Vec<String> {
        self.snapshot
            .tables
            .iter()
            .map(|t| t.name.clone())
            .collect()
    }

    fn all_schema_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .snapshot
            .tables
            .iter()
            .map(|t| t.schema.clone())
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

// ---- completion provider ---------------------------------------------------

impl CompletionSchemaProvider for CachedSchemaProvider {
    fn tables(&self) -> Vec<CompletionTable> {
        self.snapshot
            .tables
            .iter()
            .map(|t| CompletionTable::new(t.name.clone()).with_schema(t.schema.clone()))
            .collect()
    }

    fn columns(&self, schema: Option<&str>, table: &str) -> Vec<CompletionColumn> {
        let Some(t) = self.find_table(schema, table) else {
            return Vec::new();
        };
        t.columns
            .iter()
            .map(|c| {
                CompletionColumn::new(c.name.clone(), c.data_type.clone())
                    .with_nullable(c.nullable)
                    .with_primary_key(c.is_primary_key)
                    .with_ordinal(c.ordinal)
            })
            .collect()
    }

    fn jsonb_schema(&self, schema: Option<&str>, table: &str, column: &str) -> Option<JsonbSchema> {
        let entry = self.find_table(schema, table)?;
        let col = entry
            .columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(column))?;
        col.jsonb.as_ref().map(build_jsonb_schema)
    }
}

fn jsonb_field_type(ty: JsonbType) -> JsonbFieldType {
    match ty {
        JsonbType::Object => JsonbFieldType::Object,
        JsonbType::Array => JsonbFieldType::Array,
        JsonbType::String => JsonbFieldType::String,
        JsonbType::Number => JsonbFieldType::Number,
        JsonbType::Boolean => JsonbFieldType::Boolean,
        JsonbType::Null => JsonbFieldType::Null,
        JsonbType::Unknown => JsonbFieldType::Unknown,
    }
}

/// Converts a sampled [`JsonbShape`] into the completion engine's schema.
fn build_jsonb_schema(shape: &JsonbShape) -> JsonbSchema {
    let mut schema = JsonbSchema::new();
    for field in &shape.fields {
        let mut f = JsonbField::new(field.name.clone(), jsonb_field_type(field.ty));
        if let Some(nested) = &field.nested {
            f.nested = Some(Box::new(build_jsonb_schema(nested)));
        }
        schema = schema.with_field(f);
    }
    schema
}

impl FunctionProvider for CachedSchemaProvider {
    fn functions(&self) -> Vec<FunctionInfo> {
        self.snapshot.functions.iter().map(function_info).collect()
    }

    fn overloads(&self, schema: Option<&str>, name: &str) -> Vec<FunctionInfo> {
        let schema = schema.unwrap_or(&self.snapshot.default_schema);
        self.snapshot
            .functions
            .iter()
            .filter(|f| f.schema == schema && f.name == name)
            .map(function_info)
            .collect()
    }
}

fn function_info(function: &FunctionEntry) -> FunctionInfo {
    let args = function
        .arguments
        .iter()
        .map(|arg| FunctionArg {
            name: arg.name.clone(),
            data_type: arg.data_type.clone(),
            mode: match arg.mode {
                FunctionParameterMode::In => ArgMode::In,
                FunctionParameterMode::InOut => ArgMode::InOut,
                FunctionParameterMode::Variadic => ArgMode::Variadic,
            },
            default: arg.default.clone(),
        })
        .collect();
    FunctionInfo::new(&function.name, &function.return_type)
        .with_schema(&function.schema)
        .with_args(args)
        .with_returns_set(function.returns_set)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{ColumnEntry, JsonbFieldShape, JsonbShape};

    fn sample() -> CachedSchemaProvider {
        let mut snap = SchemaSnapshot::new("fp", "public");
        snap.tables.push(TableEntry {
            schema: "public".into(),
            name: "patient".into(),
            kind: TableKind::Table,
            columns: vec![
                ColumnEntry {
                    name: "id".into(),
                    data_type: "integer".into(),
                    nullable: false,
                    is_primary_key: true,
                    ordinal: 0,
                    jsonb: None,
                },
                ColumnEntry {
                    name: "resource".into(),
                    data_type: "jsonb".into(),
                    nullable: true,
                    is_primary_key: false,
                    ordinal: 1,
                    jsonb: Some(JsonbShape {
                        fields: vec![
                            JsonbFieldShape {
                                name: "resourceType".into(),
                                ty: JsonbType::String,
                                nested: None,
                            },
                            JsonbFieldShape {
                                name: "name".into(),
                                ty: JsonbType::Array,
                                nested: Some(JsonbShape {
                                    fields: vec![JsonbFieldShape {
                                        name: "given".into(),
                                        ty: JsonbType::Array,
                                        nested: None,
                                    }],
                                }),
                            },
                        ],
                    }),
                },
            ],
        });
        snap.functions.push(FunctionEntry {
            schema: "public".into(),
            name: "now".into(),
            return_type: "timestamptz".into(),
            arguments: Vec::new(),
            returns_set: false,
        });
        CachedSchemaProvider::new(snap)
    }

    #[test]
    fn hir_lookup_resolves_table_and_columns() {
        let p = sample();
        assert!(HirSchemaProvider::lookup_table(&p, None, "patient").is_some());
        // Unqualified lookup uses default schema; case-insensitive.
        assert!(HirSchemaProvider::lookup_table(&p, None, "PATIENT").is_some());
        let cols = HirSchemaProvider::lookup_columns(&p, None, "patient");
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0].name, "id");
        assert!(!cols[0].nullable);
    }

    #[test]
    fn completion_tables_and_functions() {
        let p = sample();
        assert_eq!(CompletionSchemaProvider::tables(&p).len(), 1);
        let cols = CompletionSchemaProvider::columns(&p, None, "patient");
        assert!(cols.iter().any(|c| c.name == "id" && c.is_primary_key));
        assert_eq!(FunctionProvider::functions(&p).len(), 1);
    }

    #[test]
    fn jsonb_schema_exposes_sampled_shape() {
        let p = sample();
        let schema = CompletionSchemaProvider::jsonb_schema(&p, None, "patient", "resource")
            .expect("jsonb shape present");
        assert!(schema.find_field("resourceType").is_some());
        let name = schema.find_field("name").expect("name field");
        let nested = name.nested.as_ref().expect("nested shape");
        assert!(nested.find_field("given").is_some());
        // A non-jsonb column has no shape.
        assert!(CompletionSchemaProvider::jsonb_schema(&p, None, "patient", "id").is_none());
    }

    #[test]
    fn unknown_table_yields_no_columns() {
        let p = sample();
        assert!(HirSchemaProvider::lookup_columns(&p, None, "ghost").is_empty());
        assert!(!HirSchemaProvider::schema_exists(&p, "nonexistent"));
    }
}
