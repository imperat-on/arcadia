-- ============================================================
-- Arcadia - Migracao v3+v3b: bucket de avatares (PNG/JPG/WEBP/GIF, 5MB)
-- Idempotente: cria se faltar e atualiza os limites. RODAR UMA VEZ.
-- ============================================================
insert into storage.buckets (id, name, public, file_size_limit, allowed_mime_types)
values (
  'avatars',
  'avatars',
  true,
  5242880,
  array['image/png', 'image/jpeg', 'image/webp', 'image/gif']
)
on conflict (id) do nothing;

update storage.buckets
set file_size_limit    = 5242880,
    allowed_mime_types = array['image/png', 'image/jpeg', 'image/webp', 'image/gif']
where id = 'avatars';
