---
title: CLI reference
description: Every banshee subcommand and flag.
---

```text
banshee format [--write|--check] [files…|-]
banshee lint   [--format human|json|github|sarif] [--statistics] [--breaking] [files…|-]
banshee fix    [--diff] [files…|-]
banshee parse  [--format tree|json] [files…|-]
banshee rules  [--format human|json] [--group <category|prefix>]
banshee explain <CODE>
banshee init
banshee lsp
```

Inputs are files, directories (walked for `*.sql`), or stdin (`-`).

## Global flags

- `--config <FILE>` — use this `banshee.toml` instead of discovery.
- `--no-color` — disable colored output (also honored via `NO_COLOR`).

## Commands

- **`format`** — format SQL. `--write` rewrites files in place; `--check` exits
  `1` if anything is unformatted.
- **`lint`** — report findings. `--format` selects `human` / `json` / `github`
  (PR annotations) / `sarif` (code scanning); `--statistics` prints a per-rule
  count breakdown. `--breaking` runs the full pack but fails only on
  backward-incompatible changes (raised to errors); other findings stay
  advisory — the migration gate in one run; see
  [backward-compatible migrations](/banshee/guides/backward-compatible-migrations/).
- **`fix`** — apply every available autofix. `--diff` previews instead of writing.
- **`parse`** — print the concrete syntax tree (`--format tree|json`).
- **`rules`** — list lint rules; `--group` filters by category or prefix.
- **`explain <CODE>`** — print a rule's rationale and a before/after example.
- **`init`** — scaffold a `banshee.toml`.
- **`lsp`** — run the language server over stdio.

## Exit codes

`0` clean · `1` findings or unformatted input · `2` error.
