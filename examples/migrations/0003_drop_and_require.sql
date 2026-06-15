-- Two backward-incompatible changes in one file.
-- MG05: dropping a column destroys data and breaks readers still using it.
ALTER TABLE public.users DROP COLUMN legacy_ref;

-- MG04: adding a NOT NULL column without a DEFAULT fails on a non-empty table.
ALTER TABLE public.users ADD COLUMN country text NOT NULL;
