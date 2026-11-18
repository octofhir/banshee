---
title: Linting & suppression
description: Run lints, apply fixes, and silence findings inline with noqa.
sidebar:
  order: 2
---

`banshee lint` reports findings in `human` (default), `json`, `github` (PR
annotations), or `sarif` (code scanning).

`banshee fix` applies capitalization fixes by default and preserves function
names and call-like constructs. Use `--unsafe-fixes` to include rewrites that
can change SQL behavior. Use `--diff` to preview the selected fixes.

Editor quick fixes use the same SQL and placeholder checks as the CLI.
They require parseable SQL and skip scripts containing `COPY FROM STDIN`.
Structural fixes are labelled as potentially changing SQL behavior. Editors
that support versioned edits apply each fix to the document revision it was
checked against.

Browse all rules in the [rule reference](/banshee/rules/), or run
`banshee explain <CODE>`.

## Suppressing diagnostics inline

A `-- noqa` comment silences findings without touching `banshee.toml`:

```sql
SELECT * FROM t;        -- noqa             suppress everything on this line
SELECT * FROM t;        -- noqa: AM04       suppress only AM04
SELECT * FROM t;        -- noqa: AM,RF01    a group prefix or explicit codes
```

Range form suppresses until a matching enable (or end of file):

```sql
SELECT * FROM a;        -- noqa: disable=AM04
SELECT * FROM b;        --   (AM04 stays silent here)
SELECT * FROM c;        -- noqa: enable=AM04
```

Use `disable=all` / `enable=all` to gate every rule. An alphabetic token (`AM`)
matches every code in that family; a full code (`AM04`) matches only itself.
Suppression is applied by the engine, so the LSP honours it too.
The formatter preserves the directive's line and any SQL statements that
share it. This keeps the same findings suppressed after formatting, including
when `pgformatter.no-comment` is enabled.
