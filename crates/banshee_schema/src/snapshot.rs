//! Serializable snapshot of a Postgres schema.
//!
//! A snapshot is what gets cached on disk and what every schema provider reads
//! from. It is deliberately plain data: no database handle, no async, just the
//! shape of the schema at the moment it was fetched.

use serde::{Deserialize, Serialize};

/// A point-in-time view of a database schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaSnapshot {
    /// Fingerprint of the connection + schema this was fetched from, used to
    /// invalidate the cache when the target changes.
    pub fingerprint: String,
    /// Unix timestamp (seconds) when this snapshot was fetched.
    pub fetched_at: u64,
    /// Default schema that was introspected (e.g. `public`).
    pub default_schema: String,
    /// Tables and views.
    pub tables: Vec<TableEntry>,
    /// Functions.
    pub functions: Vec<FunctionEntry>,
}

impl SchemaSnapshot {
    /// Creates an empty snapshot stamped with the given fingerprint and schema.
    pub fn new(fingerprint: impl Into<String>, default_schema: impl Into<String>) -> Self {
        Self {
            fingerprint: fingerprint.into(),
            fetched_at: now_unix(),
            default_schema: default_schema.into(),
            tables: Vec::new(),
            functions: Vec::new(),
        }
    }

    /// Whether this snapshot is still within `ttl_secs` of its fetch time and
    /// matches the expected fingerprint.
    #[must_use]
    pub fn is_fresh(&self, expected_fingerprint: &str, ttl_secs: u64) -> bool {
        self.fingerprint == expected_fingerprint
            && now_unix().saturating_sub(self.fetched_at) <= ttl_secs
    }
}

/// The relational kind of a [`TableEntry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableKind {
    Table,
    View,
    MaterializedView,
    ForeignTable,
}

/// A table or view and its columns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableEntry {
    pub schema: String,
    pub name: String,
    pub kind: TableKind,
    pub columns: Vec<ColumnEntry>,
}

/// A single column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnEntry {
    pub name: String,
    /// Postgres type name as reported by the catalog (e.g. `integer`, `jsonb`).
    pub data_type: String,
    pub nullable: bool,
    pub is_primary_key: bool,
    /// 0-based ordinal position.
    pub ordinal: usize,
    /// Inferred shape for `json`/`jsonb` columns, sampled from live rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jsonb: Option<JsonbShape>,
}

/// A sampled JSONB structure: the set of keys seen at a level and their types.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JsonbShape {
    pub fields: Vec<JsonbFieldShape>,
}

/// One key within a [`JsonbShape`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonbFieldShape {
    pub name: String,
    pub ty: JsonbType,
    /// Nested shape for object/array-of-object values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested: Option<JsonbShape>,
}

/// JSON value kind, mirroring `jsonb_typeof`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsonbType {
    Object,
    Array,
    String,
    Number,
    Boolean,
    Null,
    Unknown,
}

impl JsonbType {
    /// Maps a `jsonb_typeof` result to a [`JsonbType`].
    pub fn from_typeof(s: &str) -> Self {
        match s {
            "object" => JsonbType::Object,
            "array" => JsonbType::Array,
            "string" => JsonbType::String,
            "number" => JsonbType::Number,
            "boolean" => JsonbType::Boolean,
            "null" => JsonbType::Null,
            _ => JsonbType::Unknown,
        }
    }
}

/// A callable function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionEntry {
    pub schema: String,
    pub name: String,
    pub return_type: String,
    /// Input arguments in call order, including INOUT and VARIADIC arguments.
    pub arguments: Vec<FunctionParameter>,
    pub returns_set: bool,
}

/// An argument from the PostgreSQL catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionParameter {
    pub name: Option<String>,
    pub data_type: String,
    pub mode: FunctionParameterMode,
    /// The default expression when the connected role can see it.
    pub default: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionParameterMode {
    In,
    InOut,
    Variadic,
}

/// Seconds since the Unix epoch, saturating to 0 before 1970.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Computes a stable, non-cryptographic fingerprint for a connection target.
///
/// Used only for cache invalidation, not security; collisions merely cause a
/// re-fetch.
#[must_use]
pub fn fingerprint(connection: &str, schema: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    connection.hash(&mut hasher);
    schema.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_respects_ttl_and_fingerprint() {
        let mut snap = SchemaSnapshot::new("abc", "public");
        assert!(snap.is_fresh("abc", 60));
        assert!(!snap.is_fresh("different", 60));
        snap.fetched_at = now_unix().saturating_sub(120);
        assert!(!snap.is_fresh("abc", 60));
    }

    #[test]
    fn fingerprint_is_stable_and_target_sensitive() {
        let a = fingerprint("postgres://h/db", "public");
        let b = fingerprint("postgres://h/db", "public");
        let c = fingerprint("postgres://h/db", "app");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
