-- ============================================================
-- Arcadia - Migracao v3: bucket publico de avatares
-- (necessario para o upload de avatar e ele aparecer pros amigos)
-- ============================================================
insert into storage.buckets (id, name, public, file_size_limit, allowed_mime_types)
values (
  'avatars',
  'avatars',
  true,
  2097152,
  array['image/png', 'image/jpeg', 'image/webp']
)
on conflict (id) do nothing;
