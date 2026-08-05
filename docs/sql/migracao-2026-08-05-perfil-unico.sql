-- ============================================================
-- Arcadia - Migracao v4: Perfil Único (display_name + campos do perfil)
-- Idempotente. Necessaria para o perfil sincronizar entre maquinas.
-- ============================================================
alter table public.profiles
  add column if not exists display_name text,
  add column if not exists summary      text,
  add column if not exists country      text,
  add column if not exists city         text,
  add column if not exists showcase     jsonb not null default '[]'::jsonb;
