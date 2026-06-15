-- MG07: renaming a column breaks every query and client using the old name.
ALTER TABLE public.users RENAME COLUMN email TO email_address;
