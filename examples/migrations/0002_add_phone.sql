-- Safe additive change: nullable column, no default rewrite. Not breaking.
ALTER TABLE public.users ADD COLUMN phone text;
