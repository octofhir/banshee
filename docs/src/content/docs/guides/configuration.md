---
title: Configuration
description: The banshee.toml schema — formatter, linter, database and templater.
sidebar:
  order: 1
---

Configuration lives in `banshee.toml`, discovered by walking up from the input
(or pass `--config <file>`).

## Formatter

```toml
[format]
style = "sqlstyle"        # sqlstyle (river alignment) | pgformatter | compact
# keyword-case      = "upper"     # upper | lower | preserve
# identifier-case   = "preserve"  # lower | preserve
# indent-unit       = "space"     # space | tab
# indent-width      = 4
# max-width         = 100
# comma-style       = "trailing"  # trailing | leading
# river-alignment        = true
# newline-before-logical = true   # break before AND/OR in WHERE
```

`style` selects one of two engines; the shared knobs map across both.

## Linter

```toml
[lint]
enabled = true
# exclude = ["AM04"]
# exclude-paths = ["migrations/2019*.sql", "migrations/legacy/**"]
#                          # globs of files to skip entirely (e.g. legacy
#                          # migrations); also honoured by `lint --breaking`

# Per-rule options (each rule reads its own keys):
# [lint.rules.CV04]
# prefer = "star"          # star | 1 | 0  — count(*) vs count(1)/count(0)
# [lint.rules.CV11]
# prefer = "consistent"    # consistent | shorthand | functional
# [lint.rules.CV09]
# blocked = ["old_status", "sysdate"]
# [lint.rules.CP01]
# policy = "upper"         # keyword case
# [lint.rules.MG01]
# enabled = false          # allow CREATE INDEX without CONCURRENTLY
```

See the [rule reference](/banshee/rules/) for every code and its options.

## Database (schema-aware features)

```toml
[database]
url-env = "DATABASE_URL"  # read the connection string from this env var
schema  = "public"
```

See [Schema-aware features](/banshee/guides/schema-aware/).

## Templater

```toml
[templater]
style = "colon"   # colon (:name / :1) | question-mark (?) | percent (%s, %(name)s)
```

See [Templating](/banshee/guides/templating/).
