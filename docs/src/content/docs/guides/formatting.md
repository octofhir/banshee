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

Stdout mode accepts one input. Use `--check` or `--write` for a batch of files.

For editor buffers passed on stdin, pass the intended filename to discover the
nearest `banshee.toml` and include the filename in errors.

```sh
banshee format --stdin-filename migrations/draft.sql -
```

Indentation, comma placement, and keyword case apply to both engines.
See [Configuration](/banshee/guides/configuration/) for settings.
For application SQL with named or positional template parameters, see
[Templating](/banshee/guides/templating/).

Function names and forms such as `array(...)`, `array[...]` and
`coalesce(...)` retain their original spelling by default. The pgFormatter
preset's `function-case` setting can change unquoted function names explicitly.

The formatter checks that output preserves SQL tokens, statement separators,
comments and significant whitespace. Regression tests compare PostgreSQL parse
trees and check that a second formatting pass yields identical output.
They cover all three presets, including leading commas with comments and
CRLF line endings. Some statements use a simpler layout to preserve every token.
The CLI reports an input it cannot safely format on stderr and exits with
code `2` before writing any formatted SQL. Formatting the whole editor
document also requires valid input. Scripts containing `COPY FROM STDIN`
data are excluded from formatting.

Format Selection expands the selection to the top-level SQL statements it
touches. If the document has syntax errors, the editor can still format
valid selected statements with clear boundaries. Statements containing
errors or with ambiguous boundaries are skipped.

The `pgformatter.no-comment` option explicitly requests comment removal.

## Keep hand-formatted SQL

Use `-- fmt: off` and `-- fmt: on` on their own lines to keep a block exactly
as written. An unmatched `-- fmt: off` keeps everything through the end of
the file. Use `-- fmt: skip` on its own line before a statement or as a
trailing comment on the same line as the statement.

```sql
-- fmt: off
INSERT INTO grade (code, label) VALUES
  ('A', 'Excellent'),
  ('B', 'Good');
-- fmt: on

SELECT 1 AS a, 2 AS b; -- fmt: skip
```

The CLI and editor protect complete top-level statements when a directive
falls inside one. Format Selection respects directives outside the
selection. Directives and comments inside protected SQL are kept even when
`pgformatter.no-comment` is enabled.
