# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project tracks
the latest stable Rust toolchain.

## [0.2.0] - 2026-06-15

### Added

- **Inline suppression** (`banshee_hir`): `-- noqa` comments suppress diagnostics
  per line or across `disable=`/`enable=` ranges (codes, alphabetic group
  prefixes, or `all`); applied in the engine so the CLI and LSP both honour it.
- **Lint rules**: AL01/AL02 (explicit `AS` aliasing, fixable), AM03 (consistent
  `ORDER BY` directions), CV08 (prefer `LEFT JOIN`), CV10 (`LIKE` without a
  wildcard, fixable), ST07 (avoid `NATURAL JOIN`), ST08 (`DISTINCT ON` without
  `ORDER BY`), SF03 (`INSERT` without a column list), RF06 (unnecessary
  identifier quotes, fixable), AL04 (duplicate table alias), AL08 (duplicate
  column alias), AM01 (`DISTINCT` redundant with `GROUP BY`), CV04
  (`count(1)`/`count(0)` → `count(*)`, fixable), AM07 (set-operation branch
  column-count mismatch), CV11 (inconsistent `::`/`CAST` style), CV09
  (configurable blocked words) — 34 lint codes total.
- **Configurable rules**: per-rule options in `banshee.toml` (`[lint.rules.<CODE>]`)
  now reach the engine. `CV04` takes `prefer = star|1|0`, `CV11` takes
  `prefer = consistent|shorthand|functional`, `CV09` takes a `blocked` word
  list, `CP01`/`CP02` take `policy = upper|lower`, and `AM02` takes
  `prefer = all|distinct`.
- **Lint rule** CV13: `IN` with a single value is equivalent to `=` (fixable) —
  35 lint codes total.
- **Migration-safety lint pack** (`MG`, squawk-class): MG01 `CREATE INDEX`
  without `CONCURRENTLY` (fixable), MG02 `ADD CONSTRAINT` (FK/CHECK) without
  `NOT VALID` (fixable), MG03 `ADD COLUMN` with a volatile `DEFAULT`, MG04
  `ADD COLUMN NOT NULL` without a `DEFAULT`, MG05 `DROP COLUMN`, MG06
  `ALTER COLUMN … TYPE`, MG07 `RENAME`, MG08 `TRUNCATE … CASCADE`, MG09 prefer
  `text` over `char(n)`/`varchar(n)` (fixable), MG10 prefer `timestamptz` over
  `timestamp` (fixable), MG11 prefer `bigint` for primary keys, MG12
  `DROP INDEX` without `CONCURRENTLY` (fixable), MG13 `ADD PRIMARY KEY`/`UNIQUE`
  built under a lock, MG14 `ALTER COLUMN … SET NOT NULL`, MG15 prefer
  `GENERATED … AS IDENTITY` over `serial`, MG16 `DROP TABLE`. MG01/MG12 are
  suppressed inside an explicit transaction block, where `CONCURRENTLY` is not
  valid.
- **Full squawk parity** in the migration pack: MG17 `ALTER COLUMN … DROP
  NOT NULL`, MG18 `DROP DATABASE`, MG19 `CREATE INDEX CONCURRENTLY` inside a
  transaction, MG20 uncommitted transaction, MG21 transaction nesting, MG22
  non-idempotent statement (missing `IF [NOT] EXISTS`), MG23 unqualified
  `CREATE TABLE` name, MG24 over-long identifier — 24 migration codes.
- **Backward-incompatibility gate**: `banshee lint --breaking <path>` lints a
  whole migrations directory with the full rule set but fails only on
  backward-incompatible changes (MG04/MG05/MG06/MG07/MG08/MG16/MG17/MG18, raised
  to errors); all other findings stay advisory, so one run both reports the pack
  and gates the merge. The run fails on any error-severity finding, so rules
  promoted to `error` in config gate alongside it. Legacy files are skipped with
  the new `[lint] exclude-paths` glob list.
- **DDL typed AST and analysis** (`banshee_syntax`, `banshee_hir`): typed accessors
  for the DDL and command statements; analysis resolves tables and columns
  defined by `CREATE TABLE`/`CREATE VIEW` in the same source and no longer
  reports a migration's own DDL targets as missing.
- **CLI**: `banshee rules` gains `--format json` and `--group <category|prefix>`;
  `banshee lint` gains `--statistics` (per-rule count breakdown).
- **Placeholder templating** (`banshee_templater`): substitute `:name`, `?` and
  `%(name)s` placeholders (configurable via `[templater] style`) so app SQL
  parses and lints; diagnostics map back to the source and fixes that would
  edit a placeholder are dropped. Native `$1`/`$name` and placeholders in
  strings/comments are left untouched.
- **Rule registry** (`banshee_hir`): a `Rule` trait with a `builtin_rules()`
  registry replaces hardcoded dispatch; diagnostics are emitted in source order.
- **Declarative rule fixtures**: `tests/rule_cases/*.yaml` drive pass/fail/fix
  assertions per rule code.
- **Distribution**: rolling pre-release of the `banshee` binary (built with the
  `db` feature) for Linux/macOS/Windows on every push to `main`.

- **Configuration** (`banshee_config`): unified `banshee.toml` schema covering the
  formatter, linter, completion and database connection, with walk-up
  discovery and an env-var-based connection string.
- **Lint engine** (`banshee_hir`): 16 rules across aliasing (AL03, AL05),
  ambiguity (AM02, AM04, AM05, AM09), structure (ST01, ST03, ST05), safety
  (SF01, SF02), JSONB (JB01), convention (CV01, CV05, CV06), capitalisation
  (CP01, CP02) and references (RF01, RF02, RF03). Nine are auto-fixable.
  Diagnostics carry a typed `RuleCode`, an optional `help` line, and fixes.
- **Schema introspection** (`banshee_schema`): live Postgres introspection behind
  the `db` feature (tables, columns, primary keys, functions, and sampled JSONB
  column shapes), cached to `.banshee/schema-cache.json`; a sync, serde-only
  provider reads the cache without async dependencies.
- **Language server** (`banshee_lsp`): diagnostics with quick-fix code actions,
  completion (including JSONB key completion), hover, document & range
  formatting, document symbols, semantic tokens, and signature help.
- **CLI** (`banshee`): `format`, `lint`, `fix`, `parse`, `rules`, `explain`,
  `complete`, `schema`, `init`, and `lsp` subcommands. rustc-style diagnostic
  rendering with "did you mean?" hints; `--format human|json|github|sarif`;
  `NO_COLOR`/`--no-color`, `--quiet`, and parallel multi-file processing.
- **Editor**: a local VS Code extension under `editors/code` that runs
  `banshee lsp`.
- **CI**: GitHub Actions running fmt, clippy (`-D warnings`) and the test suite,
  plus a `db`-feature job.

### Fixed

- Parser no longer panics on the `!=` operator (a `NE` static-text mismatch).
- pgFormatter engine preserved parentheses (`count(*)`) and implicit-alias
  spacing (`users a`).
- Reference checks (RF\*) are suppressed without a live schema, so offline
  linting produces no false positives.
