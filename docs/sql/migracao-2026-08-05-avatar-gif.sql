-- ============================================================
-- Arcadia - Migracao v3b: avatar GIF + limite 5MB
-- Roda DEPOIS da v3 (bucket avatars já existe) — atualiza os limites.
-- ============================================================
update storage.buckets
set file_size_limit   = 5242880,
    allowed_mime_types = array['image/png', 'image/jpeg', 'image/webp', 'image/gif']
where id = 'avatars';
