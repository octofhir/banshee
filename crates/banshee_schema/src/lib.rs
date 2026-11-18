//! Postgres schema snapshots, on-disk cache, and snapshot-backed schema
//! providers for the banshee toolchain.
//!
//! The crate has two layers:
//!
//! - **Always available (sync, serde-only):** the [`snapshot`] model, the
//!   `.banshee/` [`cache`], and [`CachedSchemaProvider`]. Embedders can read a
//!   cached schema without pulling in any async runtime.
//! - **Behind the `db` feature:** live Postgres `introspect`ion via `sqlx`.
//!
//! Typical flow: load a fresh cache if present; otherwise (with `db`)
//! introspect the database and persist the result for next time.

pub mod cache;
pub mod provider;
pub mod snapshot;

#[cfg(feature = "db")]
pub mod introspect;

pub use provider::CachedSchemaProvider;
pub use snapshot::{
    ColumnEntry, FunctionEntry, FunctionParameter, FunctionParameterMode, JsonbFieldShape,
    JsonbShape, JsonbType, SchemaSnapshot, TableEntry, TableKind, fingerprint,
};

#[cfg(feature = "db")]
pub use introspect::introspect;
