//! Live Postgres schema introspection (requires the `db` feature).
//!
//! Reads tables, columns, primary keys and function signatures from the
//! PostgreSQL catalogs and assembles a [`SchemaSnapshot`]. The result
//! is plain data and can be cached and reused without a database connection.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use sqlx::Row;
use sqlx::postgres::PgPoolOptions;

use crate::snapshot::{
    ColumnEntry, FunctionEntry, JsonbFieldShape, JsonbShape, JsonbType, SchemaSnapshot, TableEntry,
    TableKind, fingerprint,
};

/// Rows sampled per JSONB level, and how deep to recurse.
const JSONB_SAMPLE_ROWS: u32 = 200;
const JSONB_MAX_DEPTH: u32 = 3;
const JSONB_MAX_FIELDS: usize = 64;

/// Connects to `url`, introspects `schema`, and returns a snapshot.
///
/// `pool_size` bounds connections; `connect_timeout` caps the initial connect.
pub async fn introspect(
    url: &str,
    schema: &str,
    pool_size: u32,
    connect_timeout: Duration,
) -> Result<SchemaSnapshot, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(pool_size.max(1))
        .acquire_timeout(connect_timeout)
        .connect(url)
        .await?;

    let mut snapshot = SchemaSnapshot::new(fingerprint(url, schema), schema);

    let primary_keys = fetch_primary_keys(&pool, schema).await?;
    let mut tables = fetch_tables(&pool, schema).await?;
    attach_columns(&pool, schema, &mut tables, &primary_keys).await?;
    sample_jsonb_columns(&pool, schema, &mut tables).await;
    snapshot.tables = tables.into_values().collect();
    snapshot.tables.sort_by(|a, b| a.name.cmp(&b.name));
    snapshot.functions = fetch_functions(&pool, schema).await?;

    pool.close().await;
    Ok(snapshot)
}

fn table_kind(raw: &str) -> TableKind {
    match raw {
        "VIEW" => TableKind::View,
        "FOREIGN TABLE" | "FOREIGN" => TableKind::ForeignTable,
        _ => TableKind::Table,
    }
}

async fn fetch_tables(
    pool: &sqlx::PgPool,
    schema: &str,
) -> Result<HashMap<String, TableEntry>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT table_name, table_type \
         FROM information_schema.tables \
         WHERE table_schema = $1",
    )
    .bind(schema)
    .fetch_all(pool)
    .await?;

    let mut tables = HashMap::new();
    for row in rows {
        let name: String = row.get("table_name");
        let kind: String = row.get("table_type");
        tables.insert(
            name.clone(),
            TableEntry {
                schema: schema.to_string(),
                name,
                kind: table_kind(&kind),
                columns: Vec::new(),
            },
        );
    }
    Ok(tables)
}

async fn attach_columns(
    pool: &sqlx::PgPool,
    schema: &str,
    tables: &mut HashMap<String, TableEntry>,
    primary_keys: &HashMap<String, HashSet<String>>,
) -> Result<(), sqlx::Error> {
    let rows = sqlx::query(
        "SELECT table_name, column_name, data_type, is_nullable, ordinal_position::int AS ord \
         FROM information_schema.columns \
         WHERE table_schema = $1 \
         ORDER BY table_name, ordinal_position",
    )
    .bind(schema)
    .fetch_all(pool)
    .await?;

    for row in rows {
        let table_name: String = row.get("table_name");
        let Some(table) = tables.get_mut(&table_name) else {
            continue;
        };
        let name: String = row.get("column_name");
        let data_type: String = row.get("data_type");
        let is_nullable: String = row.get("is_nullable");
        let ord: i32 = row.get("ord");
        let is_pk = primary_keys
            .get(&table_name)
            .is_some_and(|cols| cols.contains(&name));

        table.columns.push(ColumnEntry {
            name,
            data_type,
            nullable: is_nullable.eq_ignore_ascii_case("YES"),
            is_primary_key: is_pk,
            ordinal: ord.saturating_sub(1).max(0) as usize,
            jsonb: None,
        });
    }
    Ok(())
}

async fn fetch_primary_keys(
    pool: &sqlx::PgPool,
    schema: &str,
) -> Result<HashMap<String, HashSet<String>>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT tc.table_name, kcu.column_name \
         FROM information_schema.table_constraints tc \
         JOIN information_schema.key_column_usage kcu \
           ON tc.constraint_name = kcu.constraint_name \
          AND tc.table_schema = kcu.table_schema \
         WHERE tc.constraint_type = 'PRIMARY KEY' AND tc.table_schema = $1",
    )
    .bind(schema)
    .fetch_all(pool)
    .await?;

    let mut map: HashMap<String, HashSet<String>> = HashMap::new();
    for row in rows {
        let table: String = row.get("table_name");
        let column: String = row.get("column_name");
        map.entry(table).or_default().insert(column);
    }
    Ok(map)
}

async fn fetch_functions(
    pool: &sqlx::PgPool,
    schema: &str,
) -> Result<Vec<FunctionEntry>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT p.proname::text AS name,
               pg_catalog.pg_get_function_result(p.oid) AS return_type,
               p.proretset AS returns_set,
               COALESCE((
                   SELECT pg_catalog.jsonb_agg(
                       pg_catalog.jsonb_build_object(
                           'name', NULLIF(p.proargnames[a.position::int], ''),
                           'data_type', pg_catalog.format_type(a.type_oid, NULL),
                           'mode', CASE p.proargmodes[a.position::int]
                               WHEN 'b' THEN 'in_out'
                               WHEN 'v' THEN 'variadic'
                               ELSE 'in'
                           END,
                           'default', CASE WHEN pg_catalog.pg_has_role(p.proowner, 'USAGE')
                               THEN pg_catalog.pg_get_function_arg_default(p.oid, a.position::int)
                           END
                       ) ORDER BY a.position
                   )
                   FROM pg_catalog.unnest(COALESCE(p.proallargtypes, p.proargtypes::oid[]))
                       WITH ORDINALITY AS a(type_oid, position)
                   WHERE COALESCE(p.proargmodes[a.position::int], 'i') IN ('i', 'b', 'v')
               ), '[]'::jsonb)::text AS arguments
        FROM pg_catalog.pg_proc p
        JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
        WHERE n.nspname = $1
          AND p.prokind <> 'p'
          AND (pg_catalog.pg_has_role(p.proowner, 'USAGE')
               OR pg_catalog.has_function_privilege(p.oid, 'EXECUTE'))
        ORDER BY p.proname, p.pronargs, p.oid
        "#,
    )
    .bind(schema)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let arguments = serde_json::from_str(row.try_get("arguments")?)
                .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
            Ok(FunctionEntry {
                schema: schema.to_owned(),
                name: row.try_get("name")?,
                return_type: row.try_get("return_type")?,
                arguments,
                returns_set: row.try_get("returns_set")?,
            })
        })
        .collect()
}

// === JSONB shape sampling ====================================================

fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn quote_lit(s: &str) -> String {
    s.replace('\'', "''")
}

/// Samples the structure of every json/jsonb column. Best-effort: sampling
/// errors (permissions, odd data) are swallowed so introspection still
/// succeeds without shapes.
async fn sample_jsonb_columns(
    pool: &sqlx::PgPool,
    schema: &str,
    tables: &mut HashMap<String, TableEntry>,
) {
    for table in tables.values_mut() {
        for col in table.columns.iter_mut() {
            let dt = col.data_type.to_ascii_lowercase();
            if dt != "jsonb" && dt != "json" {
                continue;
            }
            let value_expr = quote_ident(&col.name);
            col.jsonb = build_shape(
                pool,
                schema,
                &table.name,
                &value_expr,
                false,
                JSONB_MAX_DEPTH,
            )
            .await;
        }
    }
}

/// Recursively samples a JSONB shape. `as_array` selects element sampling for
/// array values; otherwise object keys are sampled.
fn build_shape<'a>(
    pool: &'a sqlx::PgPool,
    schema: &'a str,
    table: &'a str,
    value_expr: &'a str,
    as_array: bool,
    depth: u32,
) -> Pin<Box<dyn Future<Output = Option<JsonbShape>> + Send + 'a>> {
    Box::pin(async move {
        if depth == 0 {
            return None;
        }
        let fields = if as_array {
            sample_array_fields(pool, schema, table, value_expr).await
        } else {
            sample_object_fields(pool, schema, table, value_expr).await
        };
        if fields.is_empty() {
            return None;
        }

        let mut out = Vec::new();
        for (name, ty) in fields.into_iter().take(JSONB_MAX_FIELDS) {
            // Only recurse into object keys; array element fields are sampled
            // one level deep (their own object keys) but not beyond.
            let nested = if !as_array && depth > 1 {
                let child_expr = format!("{value_expr}->'{}'", quote_lit(&name));
                match ty {
                    JsonbType::Object => {
                        build_shape(pool, schema, table, &child_expr, false, depth - 1).await
                    }
                    JsonbType::Array => {
                        build_shape(pool, schema, table, &child_expr, true, depth - 1).await
                    }
                    _ => None,
                }
            } else {
                None
            };
            out.push(JsonbFieldShape { name, ty, nested });
        }
        Some(JsonbShape { fields: out })
    })
}

async fn sample_object_fields(
    pool: &sqlx::PgPool,
    schema: &str,
    table: &str,
    value_expr: &str,
) -> Vec<(String, JsonbType)> {
    let sql = format!(
        "SELECT e.key AS k, jsonb_typeof(e.value) AS ty \
         FROM (SELECT {ve} AS v FROM {sch}.{tbl} WHERE {ve} IS NOT NULL LIMIT {lim}) src, \
              LATERAL jsonb_each(src.v) e \
         WHERE jsonb_typeof(src.v) = 'object'",
        ve = value_expr,
        sch = quote_ident(schema),
        tbl = quote_ident(table),
        lim = JSONB_SAMPLE_ROWS,
    );
    collect_fields(pool, &sql).await
}

async fn sample_array_fields(
    pool: &sqlx::PgPool,
    schema: &str,
    table: &str,
    value_expr: &str,
) -> Vec<(String, JsonbType)> {
    let sql = format!(
        "SELECT e.key AS k, jsonb_typeof(e.value) AS ty \
         FROM (SELECT {ve} AS v FROM {sch}.{tbl} WHERE {ve} IS NOT NULL LIMIT {lim}) src, \
              LATERAL jsonb_array_elements(src.v) el, \
              LATERAL jsonb_each(el) e \
         WHERE jsonb_typeof(src.v) = 'array' AND jsonb_typeof(el) = 'object'",
        ve = value_expr,
        sch = quote_ident(schema),
        tbl = quote_ident(table),
        lim = JSONB_SAMPLE_ROWS,
    );
    collect_fields(pool, &sql).await
}

/// Runs a sampling query and reduces (key, type) rows to one type per key
/// (the most frequently observed), in sorted key order for determinism.
async fn collect_fields(pool: &sqlx::PgPool, sql: &str) -> Vec<(String, JsonbType)> {
    let Ok(rows) = sqlx::query(sql).fetch_all(pool).await else {
        return Vec::new();
    };
    // key -> (type -> count)
    let mut counts: BTreeMap<String, HashMap<JsonbType, u32>> = BTreeMap::new();
    for row in rows {
        let key: String = row.get("k");
        let ty: Option<String> = row.try_get("ty").ok();
        let ty = ty
            .map(|t| JsonbType::from_typeof(&t))
            .unwrap_or(JsonbType::Null);
        *counts.entry(key).or_default().entry(ty).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .map(|(key, types)| {
            let ty = types
                .into_iter()
                .max_by_key(|(_, c)| *c)
                .map(|(t, _)| t)
                .unwrap_or(JsonbType::Unknown);
            (key, ty)
        })
        .collect()
}
