-- ============================================================
-- Arcadia - Migracao v3c: políticas de escrita no storage (avatars)
-- Bucket criado via SQL não ganha políticas de objetos → upsert de avatar
-- (trocar foto) falha com "new row violates row-level security policy".
-- Idempotente: cria só o que faltar.
-- ============================================================
do $$
begin
  if not exists (
    select 1 from pg_policies where schemaname = 'storage' and tablename = 'objects' and policyname = 'avatars_insert'
  ) then
    create policy "avatars_insert" on storage.objects
      for insert to authenticated with check (bucket_id = 'avatars');
  end if;

  if not exists (
    select 1 from pg_policies where schemaname = 'storage' and tablename = 'objects' and policyname = 'avatars_update'
  ) then
    create policy "avatars_update" on storage.objects
      for update to authenticated using (bucket_id = 'avatars');
  end if;

  if not exists (
    select 1 from pg_policies where schemaname = 'storage' and tablename = 'objects' and policyname = 'avatars_delete'
  ) then
    create policy "avatars_delete" on storage.objects
      for delete to authenticated using (bucket_id = 'avatars');
  end if;
end $$;
