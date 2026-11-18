---
title: CLI reference
description: Every banshee subcommand and flag.
---

```text
banshee format [--write|--check] [files…|-]
banshee lint   [--format human|json|github|sarif] [--statistics] [--breaking] [files…|-]
banshee fix    [--diff] [--unsafe-fixes] [files…|-]
banshee parse  [--format tree|json] [files…|-]
banshee complete --offset BYTE [--limit N] [file|-]
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

- **`format`** — print formatted SQL from one input. Use `--write` to rewrite
  files in place or `--check` to check files without writing. `--check` exits
  `1` if a file needs formatting. A formatting error exits `2` before any SQL
  is printed or files are rewritten. `--stdin-filename PATH` supplies the path
  for configuration discovery and diagnostics when reading stdin.
- **`lint`** — report findings. `--format` selects `human` / `json` / `github`
  (PR annotations) / `sarif` (code scanning); `--statistics` prints a per-rule
  count breakdown. `--breaking` runs the full pack but fails only on
  backward-incompatible changes (raised to errors); other findings stay
  advisory — the migration gate in one run; see
  [backward-compatible migrations](/banshee/guides/backward-compatible-migrations/).
- **`fix`** — apply capitalization fixes. `--unsafe-fixes` includes rewrites
  that can change behavior, and `--diff` previews the selected fixes.
- **`parse`** — print the concrete syntax tree (`--format tree|json`).
- **`complete`** — print suggestions at a UTF-8 byte offset. `--limit`
  overrides the configured maximum and accepts `0` for an unlimited list.
- **`rules`** — list lint rules; `--group` filters by category or prefix.
- **`explain <CODE>`** — print a rule's rationale and a before/after example.
- **`init`** — scaffold a `banshee.toml`.
- **`lsp`** — run the language server over stdio.

## Exit codes

`0` clean · `1` findings or unformatted input · `2` error.
