---
title: CI integration
description: GitHub Action, SARIF code scanning, and pre-commit.
sidebar:
  order: 7
---

## GitHub Action

banshee's repository is itself a composite Action — it installs the binary and
runs it, with findings shown inline on the PR:

```yaml
- uses: octofhir/banshee@v0.2.1
  with:
    command: lint        # or: format
    args: migrations/
```

## SARIF code scanning

The Action writes a SARIF file for upload to GitHub code scanning:

```yaml
- uses: octofhir/banshee@v0.2.1
  with:
    sarif-file: banshee.sarif
- uses: github/codeql-action/upload-sarif@v3
  with:
    sarif_file: banshee.sarif
```

`banshee lint --format sarif` emits SARIF 2.1.0 directly if you'd rather run the
binary yourself.

## Pre-commit

```yaml
repos:
  - repo: https://github.com/octofhir/banshee
    rev: v0.2.1
    hooks:
      - id: banshee-format
      - id: banshee-lint
```

Both hooks expect a `banshee` binary on `PATH`.

## Exit codes

`0` clean · `1` findings or unformatted input · `2` error.
