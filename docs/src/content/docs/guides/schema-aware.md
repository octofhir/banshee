---
title: Schema-aware features
description: Live Postgres introspection drives completion and reference checks.
sidebar:
  order: 4
---

With `[database]` configured and a `db`-feature build, banshee introspects
`information_schema` and `pg_catalog` for tables, columns, primary keys and
function signatures. It also samples JSONB columns to infer their shape. The
result is cached at `.banshee/schema-cache.json` until its TTL expires or the
configured connection or schema changes.

The cached schema drives:

- column / table completion,
- JSONB key completion (`resource->'…'`),
- function completion and editor signature help for overloads,
- `RF01` / `RF02` reference checks and "did you mean?" suggestions,
- `AM04`'s `SELECT *` expansion.

Reference checks (`RF*`) only run when a schema is available, so they never
produce false positives offline. The `.banshee/` cache is plain JSON and can be
read without the `db` feature, so embedders stay async-free.

Run `banshee schema refresh` after changing functions to update the cache.
Signature help selects an overload using argument counts and names. It
displays default expressions when the connected role has access to them.
