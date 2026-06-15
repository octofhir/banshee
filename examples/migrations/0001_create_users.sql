-- Baseline schema: schema-qualified, idempotent, durable types. Clean.
CREATE TABLE IF NOT EXISTS public.users (
    id         bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    email      text NOT NULL,
    legacy_ref text,
    created_at timestamptz NOT NULL DEFAULT now()
);
