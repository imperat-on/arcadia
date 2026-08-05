-- ============================================================
-- Arcadia - Migracao v3d: políticas COMPLETAS do storage avatars
-- Upsert (trocar avatar) falhava: ON CONFLICT precisa LER a linha
-- existente → exige política SELECT (a UPDATE sozinha não basta).
-- Idempotente: drop + recria as 4 políticas.
-- ============================================================
do $$
begin
  drop policy if exists "avatars_select" on storage.objects;
  drop policy if exists "avatars_insert" on storage.objects;
  drop policy if exists "avatars_update" on storage.objects;
  drop policy if exists "avatars_delete" on storage.objects;

  create policy "avatars_select" on storage.objects
    for select to authenticated using (bucket_id = 'avatars');

  create policy "avatars_insert" on storage.objects
    for insert to authenticated with check (bucket_id = 'avatars');

  create policy "avatars_update" on storage.objects
    for update to authenticated using (bucket_id = 'avatars') with check (bucket_id = 'avatars');

  create policy "avatars_delete" on storage.objects
    for delete to authenticated using (bucket_id = 'avatars');
end $$;
