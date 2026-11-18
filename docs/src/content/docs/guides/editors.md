---
title: Editor setup
description: Use banshee as a language server in your editor.
sidebar:
  order: 6
---

`banshee lsp` speaks LSP over stdio: diagnostics with quick-fix code actions,
completion (keywords, schema columns/tables, JSONB paths), hover, document and
range formatting, document symbols, semantic tokens, and signature help.

For a local SQL file, `banshee lsp` finds the nearest `banshee.toml` by walking
up from the file's directory. Each discovered config uses the schema cache in
its own directory. An explicit `--config` applies to every document in the
session. Buffers without a local file path use the launch directory.

With discovery enabled, the server reads settings when a SQL document opens
or changes. Reopen the SQL document to pick up config changes without editing
it. A malformed config appears as a diagnostic on the affected document.

## VS Code

A local extension lives in
[`editors/code`](https://github.com/octofhir/banshee/tree/main/editors/code).
Put `banshee` on your `PATH` (or set `banshee.path`), then from `editors/code`
run `npm install && npm run compile` and press **F5** to launch it.

## Neovim (built-in LSP)

```lua
vim.lsp.start({
  name = 'banshee',
  cmd = { 'banshee', 'lsp' },
  root_dir = vim.fs.dirname(vim.fs.find({ 'banshee.toml', '.git' }, { upward = true })[1]),
})
```

Any editor that speaks LSP can launch `banshee lsp` the same way.

## Embedding in a Rust server

Use the `banshee_lsp` crate to create a `Session` for each client. During LSP
initialization, advertise `server_capabilities()` and pass the client's
capabilities to `Session::new`. Feed decoded messages to `handle_message` in
arrival order and send any returned message to that client. The host manages
the transport, including JSON-RPC 2.0 framing, and drops the session at
shutdown. A session runs synchronous analysis and can be moved to a blocking
worker.

The session accepts incremental UTF-16 ranges as well as full text snapshots.
Changes in one notification apply in order, with analysis and diagnostics
produced for the resulting revision. After an invalid range, send a newer
full snapshot or reopen the document to resume synchronization.

The settings resolver receives the full document URI on open or change,
including virtual documents with no local file. It can share an in-memory
schema provider between a client's documents through `DocumentSettings`.
Provide a `CachedSchemaProvider` or implement `DocumentSchema` through the
analysis, completion, and function provider traits.
