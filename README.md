# banshee

[![CI](https://github.com/octofhir/banshee/actions/workflows/ci.yml/badge.svg)](https://github.com/octofhir/banshee/actions/workflows/ci.yml)

A PostgreSQL SQL parser, formatter, linter, and language server, written in Rust.

📖 **[Docs](https://octofhir.github.io/banshee/)** · **[Rule reference](https://octofhir.github.io/banshee/rules/)**

banshee builds a **lossless concrete syntax tree** with error recovery, so it keeps
working on incomplete or invalid SQL — the use case editors actually need. On
top of the tree it resolves names, formats, lints (with autofixes), and serves
an LSP. When a database connection is configured it introspects the live schema
to make completion and reference checks accurate.

Scope is deliberately **PostgreSQL only**. There is no dialect-abstraction
layer; the grammar, JSONB/JSONPath support, and lint rules target Postgres.

## Why banshee

- **One binary, three jobs.** Format, lint (with autofixes), and an LSP — not a
  formatter *or* a linter you wire together.
- **Keeps working on broken SQL.** A lossless, error-recovering CST means
  editors get diagnostics on half-typed statements, not a hard parse failure.
- **Correct by construction.** Every statement is cross-checked against
  PostgreSQL's own parser (`libpg_query`) in a differential test.
- **Postgres-native.** JSONB/JSONPath, plus a squawk-class migration-safety
  pack (MG01–MG30) that generic SQL tools don't have, with a
  `lint --breaking` gate for backward-incompatible changes.
- **Fast.** Compiled Rust, no Python or Perl runtime to spin up per file:

  | formatter (400-stmt, 60 KB Postgres file) | median | vs banshee |
  |-------------------------------------------|--------|------------|
  | **banshee**                               | ~11 ms | 1×         |
  | sqlfluff 4.2 (`format --dialect postgres`)| ~257 ms| ~24× slower |
  | pgFormatter 5.10 (`pg_format`)            | ~1170 ms| ~105× slower |

  Linting shows the same gap (~9 ms vs sqlfluff's ~250 ms). Measured on Apple
  Silicon, median of 20 runs including process startup; reproduce with
  [`scripts/bench.sh`](scripts/bench.sh). Numbers vary by machine and corpus,
  but the order of magnitude is the point.

## Install

Prebuilt binary (Linux/macOS), verifies its SHA-256:

```sh
curl -fsSL https://raw.githubusercontent.com/octofhir/banshee/main/install.sh | sh
```

With a Rust toolchain:

```sh
cargo binstall banshee     # fetch the prebuilt release archive
cargo install banshee      # or build from source via crates.io
```

Windows users can grab the `.zip` from [Releases](https://github.com/octofhir/banshee/releases/latest).

From a checkout:

```sh
cargo build --release -p banshee                  # parser, formatter, linter, LSP
cargo build --release -p banshee --features db    # + live schema introspection
```

The binary is `target/release/banshee`. The `db` feature pulls in `sqlx`/`tokio`;
leave it off and the binary stays async-free, using a cached schema if present.

## CLI

```
banshee format [--write|--check] [files…|-]
banshee lint   [--format human|json|github|sarif] [--statistics] [--breaking] [files…|-]
banshee fix    [--diff] [files…|-]
banshee parse  [--format tree|json] [files…|-]
banshee rules [--format human|json] [--group <category|prefix>]   # list lint rules
banshee explain <CODE>        # detailed rule description with examples
banshee init                  # scaffold a banshee.toml
banshee lsp                   # language server over stdio
```

Inputs are files, directories (walked for `*.sql`), or stdin (`-`). Exit codes:
`0` clean, `1` findings / unformatted, `2` error.

```sh
echo "select id,name from users where active=true" | banshee format -
#  SELECT id, name
#    FROM users
#   WHERE active = true

echo "select * from a, b" | banshee lint -
#  warning[AM04]: Avoid SELECT *; list columns explicitly
#  warning[AM05]: Implicit cross join; use an explicit JOIN clause
#  …
```

Human output is rendered rustc-style (caret underlines, `help:` hints, colors on
a TTY). `--format json|github|sarif` is for CI and code-scanning.

`banshee lint --breaking migrations/` is the migration gate: it lints with the
full rule set but only **backward-incompatible** changes (drop/rename/retype a
column or table, add a `NOT NULL` column without a default, …) fail the run as
errors — every other finding stays advisory. One run reports everything and
blocks on breaking changes. Skip legacy files with `exclude-paths` in
`banshee.toml`. See the
[backward-compatible migrations guide](https://octofhir.github.io/banshee/guides/backward-compatible-migrations/).

## Configuration

Configuration lives in `banshee.toml`, discovered by walking up from the input.
The on-disk schema is decoupled from the engine's internal types.

```toml
[format]
style = "sqlstyle"        # sqlstyle | pgformatter | compact
# keyword-case = "upper"  # plus per-knob overrides (indent, commas, width…)

[lint]
enabled = true
# exclude = ["AM04"]
# exclude-paths = ["migrations/2019*.sql"]   # skip legacy files entirely
# [lint.rules.CP01]
# severity = "warning"

# Per-rule options (a rule reads its own keys):
# [lint.rules.CV04]
# prefer = "star"            # star | 1 | 0
# [lint.rules.CV11]
# prefer = "consistent"      # consistent | shorthand | functional
# [lint.rules.CV09]
# blocked = ["old_status", "sysdate"]   # words to forbid (off until set)
# [lint.rules.CP01]
# policy = "upper"           # upper | lower — keyword case
# [lint.rules.CP02]
# policy = "lower"           # lower | upper — identifier case

[database]
url-env = "DATABASE_URL"  # read the connection string from this env var
schema  = "public"
```

The formatter ships two engines: `sqlstyle` (river alignment, sqlstyle.guide)
and `pgformatter`. `style` selects the engine; shared knobs map across both.

## Linting

| Code | Fixable | Description |
|------|---------|-------------|
| AL01 | yes     | Table alias should be introduced with `AS` |
| AL02 | yes     | Column/expression alias should be introduced with `AS` |
| AL03 | —       | Complex select expression should be aliased with `AS` |
| AL04 | —       | Duplicate table alias in one `FROM` |
| AL05 | —       | Table alias declared but never used |
| AL08 | —       | Duplicate column alias in a `SELECT` list |
| AM01 | —       | `DISTINCT` is redundant with `GROUP BY` |
| AM02 | —       | Set operators (`UNION`/`EXCEPT`/`INTERSECT`) should state `ALL`/`DISTINCT` |
| AM03 | —       | `ORDER BY` mixes explicit and implicit sort directions |
| AM04 | yes\*   | Avoid `SELECT *`; expand to columns (\*fix needs schema) |
| AM05 | —       | Implicit cross join; use an explicit `JOIN` |
| AM07 | —       | Set-operation branches select different column counts |
| AM09 | —       | `LIMIT`/`OFFSET` without `ORDER BY` is non-deterministic |
| ST01 | yes     | Redundant `ELSE NULL` in `CASE` |
| ST03 | —       | CTE defined but never used |
| ST05 | —       | Subquery in `FROM`/`JOIN`; prefer a CTE |
| ST07 | —       | Avoid `NATURAL JOIN`; state the join columns |
| ST08 | —       | `DISTINCT ON` without `ORDER BY` is non-deterministic |
| SF01 | —       | `UPDATE` without `WHERE` affects all rows |
| SF02 | —       | `DELETE` without `WHERE` affects all rows |
| SF03 | —       | `INSERT` without an explicit column list |
| JB01 | yes     | Use `->>` when comparing a JSONB value to text |
| CV01 | yes     | Use `<>` instead of `!=` for inequality |
| CV05 | yes     | Compare with NULL using `IS NULL` / `IS NOT NULL` |
| CV06 | yes     | Statements should end with a semicolon |
| CV08 | —       | Prefer `LEFT JOIN` over `RIGHT JOIN` |
| CV09 | —       | Use of a configured blocked word (off until set) |
| CV10 | yes     | `LIKE` without a wildcard is just `=` |
| CV11 | —       | Inconsistent cast style (`::` vs `CAST`) in a statement |
| CV13 | yes     | `IN` with a single value is just `=` |
| CP01 | yes     | Keywords should be upper case |
| CP02 | yes     | Unquoted identifiers should be lower case |
| RF01 | —       | Unknown table/column/alias (needs schema) |
| RF02 | —       | Ambiguous column; qualify it (needs schema) |
| RF03 | —       | Inconsistent column qualification in a single-table query |
| RF06 | yes     | Identifier quoted unnecessarily |
| MG01 | yes     | `CREATE INDEX` without `CONCURRENTLY` locks the table |
| MG02 | yes     | `ADD CONSTRAINT` (FK/CHECK) without `NOT VALID` validates under a lock |
| MG03 | —       | `ADD COLUMN` with a volatile `DEFAULT` rewrites the table |
| MG04 | —       | `ADD COLUMN NOT NULL` without a `DEFAULT` fails on a non-empty table |
| MG05 | —       | `DROP COLUMN` destroys data and breaks dependents |
| MG06 | —       | `ALTER COLUMN … TYPE` rewrites the table under a lock |
| MG07 | —       | `RENAME` breaks code that refers to the old name |
| MG08 | —       | `TRUNCATE … CASCADE` empties dependent tables too |
| MG09 | yes     | Prefer `text` to `char(n)`/`varchar(n)` |
| MG10 | yes     | Prefer `timestamptz` to `timestamp` |
| MG11 | —       | Prefer `bigint` over a narrower integer for a primary key |
| MG12 | yes     | `DROP INDEX` without `CONCURRENTLY` locks the table |
| MG13 | —       | `ADD PRIMARY KEY`/`UNIQUE` builds its index under a lock |
| MG14 | —       | `ALTER COLUMN … SET NOT NULL` scans the table under a lock |
| MG15 | —       | Prefer `GENERATED … AS IDENTITY` over `serial` |
| MG16 | —       | `DROP TABLE` destroys the table and its dependents |
| MG17 | —       | `ALTER COLUMN … DROP NOT NULL` lets nulls into the column |
| MG18 | —       | `DROP DATABASE` destroys the whole database |
| MG19 | —       | `CREATE INDEX CONCURRENTLY` cannot run inside a transaction |
| MG20 | —       | Transaction opened but never committed or rolled back |
| MG21 | —       | `BEGIN`/`START` issued inside an open transaction |
| MG22 | —       | `CREATE`/`DROP` without `IF [NOT] EXISTS` is not idempotent |
| MG23 | —       | `CREATE TABLE` name is not schema-qualified |
| MG24 | —       | Identifier exceeds Postgres's 63-byte limit |
| MG25 | —       | `REINDEX` without `CONCURRENTLY` locks the index |
| MG26 | —       | `VACUUM FULL`/`CLUSTER` rewrite the table under a lock |
| MG27 | —       | Lock-taking migration without a statement/lock timeout |
| MG28 | —       | `CREATE DOMAIN` with a constraint validates under a lock |
| MG29 | —       | `ALTER DOMAIN ADD CONSTRAINT` validates under a lock |
| MG30 | —       | `DETACH PARTITION` without `CONCURRENTLY` locks the parent |

Prefixes: `AL` aliasing, `AM` ambiguity, `ST` structure, `SF` safety, `JB`
JSONB, `CV` convention, `CP` capitalisation, `RF` references, `MG` migration
safety.

`banshee fix` applies every available autofix; `banshee explain <CODE>` prints the
rationale and a before/after example. Reference checks (RF\*) only run when a
schema is available, so they never produce false positives offline.

### Suppressing diagnostics inline

A `noqa` comment silences findings without touching `banshee.toml`. It uses the
familiar `-- noqa` convention, so existing suppressions carry over:

```sql
SELECT * FROM t;              -- noqa               suppress everything on this line
SELECT * FROM t;              -- noqa: AM04         suppress only AM04 on this line
SELECT * FROM t;              -- noqa: AM,RF01      a group prefix or explicit codes

-- range form: suppress until a matching enable (or end of file)
SELECT * FROM a;              -- noqa: disable=AM04
SELECT * FROM b;              --   (AM04 stays silent here)
SELECT * FROM c;              -- noqa: enable=AM04
```

Use `disable=all` / `enable=all` to gate every rule. Suppression is applied by
the engine, so the LSP honours it too. An alphabetic token (`AM`) matches every
code in that family; a full code (`AM04`) matches only itself.

## Templating

Application SQL often contains parameter placeholders that are not valid
PostgreSQL on their own (`:name`, `?`, `%(name)s`). Enable the templater to
substitute them before parsing so the statement still lints; findings map back
to the original text and the placeholders survive `banshee fix` untouched.

```toml
[templater]
style = "colon"        # colon (:name / :1) | question-mark (?) | percent (%s, %(name)s)
```

Native Postgres parameters (`$1`, `$name`) are already valid and are never
touched, and placeholders inside strings, dollar-quotes and comments are left
alone. See [`examples/templater.sql`](examples/templater.sql).

## Schema-aware features

With `[database]` configured and a `db` build, banshee introspects
`information_schema`/`pg_catalog` for tables, columns, primary keys, functions,
and **samples JSONB columns** to infer their shape. The result is cached at
`.banshee/schema-cache.json` (keyed by connection + schema, with a TTL), so only
the first run touches the database.

The cached schema drives:
- column / table completion,
- JSONB key completion (`resource->'…'`),
- `RF01`/`RF02` reference checks and "did you mean?" suggestions,
- `AM04`'s `SELECT *` expansion.

A `.banshee/` cache is plain JSON and can be read without the `db` feature, so
embedders stay async-free.

## Language server

`banshee lsp` speaks LSP over stdio. Implemented:

- diagnostics (parse errors + lint findings, with quick-fix code actions),
- completion (keywords, schema columns/tables, JSONB paths),
- hover (column type and source table),
- document formatting and range formatting,
- document symbols (outline),
- semantic tokens (highlighting),
- signature help.

### Editor setup — VS Code

A local extension lives in [`editors/code`](editors/code). Build `banshee`, put it
on your `PATH` (or set `banshee.path`), then from `editors/code` run
`npm install && npm run compile` and press **F5** in VS Code to launch it. See
[`editors/code/README.md`](editors/code/README.md) for details. It is not
published to the Marketplace.

## CI integration

This repo is itself a GitHub Action — it installs `banshee` and runs it, with
findings shown inline on the PR (`github` format by default):

```yaml
- uses: octofhir/banshee@v0.2.2
  with:
    command: lint        # or: format
    args: migrations/
```

For code scanning, emit SARIF and upload it; the Action writes the file for you:

```yaml
- uses: octofhir/banshee@v0.2.2
  with:
    sarif-file: banshee.sarif
- uses: github/codeql-action/upload-sarif@v3
  with:
    sarif_file: banshee.sarif
```

`banshee lint --format sarif` emits SARIF 2.1.0 directly if you'd rather run the
binary yourself. For pre-commit, this repo ships hooks (`banshee-format`,
`banshee-lint`) — see `.pre-commit-hooks.yaml`.

## Library

The workspace is split into focused crates:

| Crate | Purpose |
|-------|---------|
| `banshee_lexer` | Tokenizer |
| `banshee_parser` | Recovering parser → lossless CST |
| `banshee_syntax` | Syntax kinds, AST, `Parse` container |
| `banshee_hir` | Name resolution, scopes, lint rules |
| `banshee_completion` | Completion engine and provider traits |
| `banshee_format` | Formatter (two engines) and edit diffing |
| `banshee_config` | `banshee.toml` schema (serde) |
| `banshee_schema` | Schema snapshot, `.banshee/` cache, live introspection |
| `banshee_lsp` | Language server |
| `banshee` | Facade crate and CLI |

```rust
let parse = banshee::parser::parse("SELECT id, name FROM users");
assert!(parse.errors().is_empty());

let formatted = banshee_format::format_sqlstyle("select id,name from users");
println!("{formatted}");
```

Runnable examples:

```sh
cargo run -p banshee --example parse_and_format
cargo run -p banshee --example completion
cargo run -p banshee --example custom_provider
```

## Development

```sh
just test          # cargo test --all
just check         # fmt + clippy + test
just demo-db       # spin up Postgres, introspect, lint a sample (needs Docker)
```

`just demo-db` brings up a seeded Postgres via `docker-compose.yml`, introspects
it (writing `.banshee/schema-cache.json`), and runs schema-aware lint plus JSONB
key completion against it end to end.

The project tracks the latest stable Rust toolchain; there is no pinned MSRV.

## Stability

Pre-1.0: the public API of the engine crates may change between minor versions.
The intended public surface is what each crate re-exports from its `lib.rs`
(notably `Parse`/`SyntaxNode` in `banshee_syntax`, `parse` in `banshee_parser`,
`analyze_query*` / `SchemaProvider` / `Diagnostic` / `RuleCode` in `banshee_hir`,
the provider traits in `banshee_completion`, and `format*` in `banshee_format`).
Items reachable but not part of that surface may change without notice.

**Embedding the engine.** banshee is built to be driven by other tools without a
fork. `banshee_hir` exposes a committed extension contract — `SchemaProvider`
(feed schema from any source, e.g. FHIR `StructureDefinition` rather than live
introspection), `LintRulePack` (add domain-specific rules), `AnalysisOptions` /
`BuiltinLintPack`, and `analyze_query_with_options` — and `banshee_completion`
exposes its `SchemaProvider` / `FunctionProvider` traits. These will not change
incompatibly within a minor version.

## License

MIT
