---
title: Formatting
description: Two formatter engines — sqlstyle and pgFormatter.
sidebar:
  order: 3
---

`banshee format` ships two engines, selected by `[format] style`:

- **`sqlstyle`** — river alignment per the sqlstyle.guide (leading keywords
  right-aligned into a "river").
- **`pgformatter`** — a pgFormatter-style layout, including DDL.
- **`compact`** — minimal, single-line-leaning output.

```sh
banshee format --write .     # rewrite files in place
banshee format --check .     # exit 1 if anything is unformatted (for CI)
banshee format -             # read stdin, write stdout
```

Shared knobs (indent, width, comma style, keyword case, …) map across both
engines — see [Configuration](/banshee/guides/configuration/).

The formatter checks that output preserves SQL tokens, statement separators,
comments and significant whitespace. PostgreSQL regression tests exercise all
three presets. Some statements use a simpler layout to preserve every token.
The CLI reports an input it cannot safely format on stderr and exits with
code `2` before writing any formatted SQL. In editor formatting, an input
that cannot be safely formatted yields no edits. This includes scripts with
`COPY FROM STDIN` data.

The `pgformatter.no-comment` option explicitly requests comment removal.
