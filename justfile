# Development task runner for banshee

# Run all tests
test:
    cargo test --all

# Run tests and accept new snapshots
test-update:
    cargo test --all
    cargo insta accept

# Run pre-commit checks (format, lint, test)
check:
    cargo fmt --all -- --check
    cargo clippy --all -- -D warnings
    cargo test --all

# Format code
fmt:
    cargo fmt --all

# Fix clippy warnings
fix:
    cargo clippy --all --fix --allow-dirty

# Run everything (format, test, accept snapshots)
all:
    cargo fmt --all
    cargo test --all
    cargo insta accept

# Quick development cycle: format, test, accept snapshots
dev:
    cargo fmt --all
    cargo test --all || cargo insta accept && cargo test --all

# === CLI ===

# Build the banshee binary (release)
build-banshee:
    cargo build -p banshee --release

# Build with live database introspection (sqlx + tokio)
build-db:
    cargo build -p banshee --release --features db

# Format a SQL string (reads via stdin)
format-sql sql:
    echo "{{sql}}" | cargo run -p banshee --quiet -- format -

# Lint a SQL string (exits 0 even when findings exist, for convenience)
lint-sql sql:
    echo "{{sql}}" | cargo run -p banshee --quiet -- lint - || true

# Apply autofixes to a SQL string
fix-sql sql:
    echo "{{sql}}" | cargo run -p banshee --quiet -- fix - || true

# Dump the CST of a SQL string as JSON
parse-sql sql:
    echo "{{sql}}" | cargo run -p banshee --quiet -- parse --format json - || true

# List built-in lint rules
rules:
    cargo run -p banshee --quiet -- rules

# Run the backward-incompatibility gate over the example migrations directory.
# Breaking changes (0003/0004) are reported as errors and set exit 1; other
# findings stay advisory; the frozen legacy/ file is skipped via exclude-paths.
demo-breaking:
    cargo run -p banshee --quiet -- --config examples/migrations/banshee.toml lint --breaking examples/migrations || true

# Run the language server over stdio (with live schema support)
lsp:
    cargo run -p banshee --features db -- lsp

# === Postgres-backed demo ===

# Start the demo Postgres (seeded with example tables) and wait for readiness
db-up:
    docker compose up -d --wait

# Stop and remove the demo Postgres (and its volume)
db-down:
    docker compose down -v

# Introspect the live schema, cache it under .banshee/, lint a sample (needs Docker)
demo-db: db-up
    DATABASE_URL=postgres://banshee:banshee@localhost:55432/banshee_demo \
        cargo run -p banshee --features db -- --config banshee.example.toml lint examples/demo.sql || true
    @echo
    @echo "--- JSONB key completion after resource-> (sampled from live rows) ---"
    printf "select resource->'' from patient" | \
        DATABASE_URL=postgres://banshee:banshee@localhost:55432/banshee_demo \
        cargo run -p banshee --features db --quiet -- --config banshee.example.toml complete --offset 18 - || true
    @echo
    @echo "Schema (incl. JSONB shapes) cached at .banshee/schema-cache.json"
