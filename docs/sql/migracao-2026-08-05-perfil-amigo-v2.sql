-- ============================================================
-- Arcadia - Migracao v2: Perfil de Amigo com metadados das conquistas
-- (title/icon/percent guardados no sync p/ aparecerem no perfil do amigo
--  mesmo quando o jogo NAO esta instalado localmente)
-- Idempotente — pode rodar de novo sem erro. Inclui o v1.
-- ============================================================

-- 1) Colunas de metadados na user_achievements
alter table public.user_achievements
  add column if not exists title  text,
  add column if not exists icon   text,
  add column if not exists percent real;

-- 2) sync_achievements: agora recebe title/icon/percent (primeiro desbloqueio
--    vence no unlocked_at; metadados preenchem quando faltarem)
create or replace function public.sync_achievements(p_items jsonb)
returns setof public.user_achievements
language plpgsql security invoker set search_path = public as $$
begin
  return query
  insert into public.user_achievements (user_id, appid, apiname, unlocked_at, title, icon, percent)
  select
    auth.uid(),
    (i->>'appid'),
    (i->>'apiname'),
    to_timestamp((i->>'unlocked_at')::double precision),
    nullif(i->>'title', ''),
    nullif(i->>'icon', ''),
    nullif((i->>'percent')::text, '')::real
  from jsonb_array_elements(p_items) i
  on conflict (user_id, appid, apiname)
  do update set
    unlocked_at = least(user_achievements.unlocked_at, excluded.unlocked_at),
    title       = coalesce(excluded.title,       user_achievements.title),
    icon        = coalesce(excluded.icon,        user_achievements.icon),
    percent     = coalesce(excluded.percent,     user_achievements.percent),
    updated_at  = now()
  where user_achievements.unlocked_at > excluded.unlocked_at
  returning *;
end $$;

grant execute on function public.sync_achievements(jsonb) to authenticated;

-- 3) friend_achievements: devolve os metadados (só entre amigos aceitos)
create or replace function public.friend_achievements(p_friend uuid)
returns table (appid text, apiname text, unlocked_at timestamptz, updated_at timestamptz, title text, icon text, percent real)
language plpgsql security definer set search_path = public as $$
begin
  if not exists (
    select 1 from public.friendships
    where status = 'accepted'
      and ((user_a = auth.uid() and user_b = p_friend)
        or (user_a = p_friend and user_b = auth.uid()))
  ) then
    return;
  end if;
  return query
    select ua.appid, ua.apiname, ua.unlocked_at, ua.updated_at, ua.title, ua.icon, ua.percent
    from public.user_achievements ua
    where ua.user_id = p_friend
    order by ua.unlocked_at desc
    limit 30;
end $$;

grant execute on function public.friend_achievements(uuid) to authenticated;

-- 4) Policy de remover amigo aceito (se ainda não existir)
do $$
begin
  if not exists (
    select 1 from pg_policies
    where schemaname = 'public' and tablename = 'friendships' and policyname = 'friends_delete_accepted'
  ) then
    create policy friends_delete_accepted on public.friendships
      for delete to authenticated
      using (auth.uid() in (user_a, user_b) and status = 'accepted');
  end if;
end $$;
