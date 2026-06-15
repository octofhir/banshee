-- Pre-policy migration: contains a breaking change (MG16) but is frozen.
-- Excluded via `exclude-paths` in this directory's banshee.toml, so the gate
-- never flags it.
DROP TABLE old_accounts;
