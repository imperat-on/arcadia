-- ============================================================
-- Arcadia - Migracao: Perfil de Amigo (2026-08-05)
-- Colar no SQL Editor e executar (uma vez). Idempotente.
-- ============================================================

-- 1) Conquistas publicas do amigo (so entre amigos aceitos)
create or replace function public.friend_achievements(p_friend uuid)
returns table (appid text, apiname text, unlocked_at timestamptz, updated_at timestamptz)
language plpgsql security definer set search_path = public as $$
begin
  -- Só devolve se os dois forem amigos aceitos (qualquer direção)
  if not exists (
    select 1 from public.friendships
    where status = 'accepted'
      and ((user_a = auth.uid() and user_b = p_friend)
        or (user_a = p_friend and user_b = auth.uid()))
  ) then
    return;
  end if;
  return query
    select ua.appid, ua.apiname, ua.unlocked_at, ua.updated_at
    from public.user_achievements ua
    where ua.user_id = p_friend
    order by ua.unlocked_at desc
    limit 30;
end $$;

grant execute on function public.friend_achievements(uuid) to authenticated;

-- 2) Remover amigo aceito (qualquer membro do par)
create policy friends_delete_accepted on public.friendships
  for delete to authenticated
  using (auth.uid() in (user_a, user_b) and status = 'accepted');
