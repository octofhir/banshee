---
title: Library (crates)
description: Embed banshee's engine — the workspace crates and their stable surface.
---

banshee is a workspace of focused crates, all published on crates.io.

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
let parse = banshee_parser::parse("SELECT id, name FROM users");
assert!(parse.errors().is_empty());

let formatted = banshee_format::format_sqlstyle("select id,name from users");
println!("{formatted}");
```

Use `banshee_format::try_format` or `banshee_format::pg_format::try_format`
to handle formatting errors. The helpers that return `String` return the
original SQL when formatting fails.

## Embedding the engine

banshee is built to be driven by other tools without a fork. `banshee_hir`
exposes a committed extension contract:

- `SchemaProvider` — supply schema (tables, columns, JSONB shape) from any
  source, e.g. a FHIR `StructureDefinition` instead of live introspection.
- `LintRulePack` — add domain-specific lint rules.
- `AnalysisOptions` / `BuiltinLintPack` — select built-in packs and inject
  external packs via `external_lint_packs`.
- `analyze_query_with_options` — the entry point that takes both.

`banshee_completion` likewise exposes its `SchemaProvider` / `FunctionProvider`
traits. These will not change incompatibly within a minor version.

## Stability

Pre-1.0: the public API may change between minor versions. The intended surface
is what each crate re-exports from its `lib.rs`, plus the extension contract
above.
