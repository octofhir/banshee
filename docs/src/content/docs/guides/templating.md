---
title: Templating
description: Format and analyze application SQL with parameter placeholders in the CLI and editor.
sidebar:
  order: 5
---

Application SQL often contains placeholders that aren't valid PostgreSQL on their
own (`:name`, `?`, `%(name)s`). Enable the templater for `banshee format`,
`banshee lint`, `banshee fix`, and `banshee lsp`. The language server
reports findings and highlights parameters at their positions in the
original document. Fixes that overlap a substituted placeholder are skipped.

```toml
[templater]
style = "colon"   # colon (:name / :1) | question-mark (?) | percent (%s, %(name)s)
```

PostgreSQL positional parameters such as `$1` are preserved, as are placeholders
inside quoted identifiers, string literals, and comments.

CLI formatting, linting, and fixes prefer the original text when Banshee's
PostgreSQL parser accepts it. Otherwise they validate SQL with the configured
placeholders substituted. Formatting and fixes preserve parameter spelling.

With `question-mark`, a bare `?` in a value position is a parameter.
Infix `?` and operators such as `?|`, `?&`, and `@?` keep their PostgreSQL
meaning. For example, `WHERE data ? ?` tests for a key supplied as a parameter.
[PostgreSQL JSON functions and operators](https://www.postgresql.org/docs/current/functions-json.html)
