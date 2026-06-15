---
title: Backward-compatible migrations
description: Enforce a no-breaking-changes policy across your migration history with banshee lint --breaking.
sidebar:
  order: 3
---

A schema migration is **backward-incompatible** when it breaks code or data that
the currently-deployed application still depends on: dropping a column a release
still reads, renaming a table, tightening a column to `NOT NULL` on a table that
already has rows. These changes turn a routine deploy into an outage.

**Policy: migrations must be backward-compatible.** Land the schema change and
the application change as separate, ordered deploys (expand → migrate →
contract) so that old and new code both work against the schema at every step.

banshee enforces this with a dedicated gate.

## The `--breaking` gate

`banshee lint --breaking <path>` lints every SQL file under `<path>` with the
full rule set, but **only backward-incompatible changes fail the run** (exit
code `1`, reported as errors). Every other finding — locking hazards, type
preferences, style — is still printed, but stays advisory and does not affect
the exit code.

```sh
banshee lint --breaking migrations/
```

One invocation therefore does both jobs: it surfaces the whole migration-safety
pack *and* gates the merge on breaking changes — no need for a second run.

The rules it enforces:

| Rule | Breaking change |
|------|-----------------|
| [MG04](/banshee/rules/mg04/) | `ADD COLUMN NOT NULL` without `DEFAULT` (fails on a non-empty table) |
| [MG05](/banshee/rules/mg05/) | `DROP COLUMN` |
| [MG06](/banshee/rules/mg06/) | `ALTER COLUMN … TYPE` |
| [MG07](/banshee/rules/mg07/) | `RENAME` of a table or column |
| [MG08](/banshee/rules/mg08/) | `TRUNCATE … CASCADE` |
| [MG16](/banshee/rules/mg16/) | `DROP TABLE` |
| [MG17](/banshee/rules/mg17/) | `ALTER COLUMN … DROP NOT NULL` |
| [MG18](/banshee/rules/mg18/) | `DROP DATABASE` |

Each finding links to a rule page with the safe, multi-step rewrite. Run
`banshee explain MG05` for the same guidance on the command line.

## Excluding legacy files

Existing migrations that predate the policy should not block new work. List them
under `exclude-paths` in `banshee.toml` — the globs are matched against each
input path and matching files are skipped entirely:

```toml
[lint]
exclude-paths = [
  "migrations/2019*.sql",   # frozen: pre-policy
  "migrations/legacy/**",
]
```

Add files to this list only when they are already merged. New migrations should
pass the gate, not be exempted from it.

## Wiring it into CI

`--breaking` exits non-zero when it finds a breaking change, so it drops
straight into a pipeline. With the GitHub Actions formatter, findings annotate
the diff:

```yaml
- uses: octofhir/banshee@v0.2.2
  with:
    command: lint
    args: --breaking --format github migrations/
```

See [CI integration](/banshee/guides/ci/) for SARIF code-scanning and the full
Action reference.

## Pre-commit hook

Catch breaking migrations before they are committed:

```yaml
# .pre-commit-config.yaml
- repo: local
  hooks:
    - id: banshee-breaking
      name: no backward-incompatible migrations
      entry: banshee lint --breaking
      language: system
      files: ^migrations/.*\.sql$
```

## What the gate does *not* flag

The gate is context-aware to avoid false alarms on changes that are not actually
breaking:

- A table created in the **same migration file** has no deployed clients or rows
  yet, so adding a `NOT NULL` column to it, or dropping it/its columns, is not
  flagged (handy for temp scaffolding).
- An `ADD COLUMN NOT NULL` that is `GENERATED` (identity or computed) supplies
  its own value for every row, so it is safe.
- `RENAME CONSTRAINT` is not flagged — constraint names are not referenced by
  client code.
- An `ALTER COLUMN … TYPE` that is a **provable safe widening** — determined from
  the column's type history across the whole migration directory — is not
  flagged: `int → bigint`, `real → double precision`, `varchar(n) → text` or
  `varchar(m)` with `m ≥ n`. Narrowings (`bigint → int`, `varchar(100) →
  varchar(50)`) still fail, and a column whose type is unknown or changed more
  than once is treated conservatively (flagged).

These never hide a real breaking change: when banshee cannot prove a change is
safe, it flags it.

## Advisory vs blocking

`--breaking` already runs the rest of the [migration-safety
pack](/banshee/rules/) (`MG01`–`MG24`) — locking hazards, non-idempotent
statements, type preferences — and prints those findings; they just don't fail
the run. To make any of them blocking too, raise its severity to `error` in
`banshee.toml`:

```toml
[lint.rules.MG01]
severity = "error"   # also block CREATE INDEX without CONCURRENTLY
```

With `--breaking`, the run fails on **any** error-severity finding, so
configured errors and the built-in breaking rules gate together in one pass.
