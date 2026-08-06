-- ============================================================
-- Arcadia - Migracao v6: SEGURANCA (auditoria 2026-08-05)
-- A-01 login_check, A-02 storage dono, A-03 visibilidade perfis,
-- A-04a/b friend_achievements (blocks+privacidade), A-05a/b clamps,
-- A-22/23 friendships, A-24 username_available, A-19 trigger, A-20 revokes
-- Idempotente. Colar no SQL Editor e executar UMA vez.
-- ============================================================

-- ============================================================
-- A-01: login_check SUBSTITUI login_email (não vaza email ao anon)
-- ============================================================
drop function if exists public.login_email(text);

create table if not exists public.login_attempts (
  username     text primary key,
  attempts     int  not null default 1,
  window_start timestamptz not null default now()
);

create or replace function public.login_check(p_username text, p_password text)
returns jsonb
language plpgsql security definer set search_path = public, extensions as $$
declare
  u   auth.users%rowtype;
  rec public.login_attempts%rowtype;
begin
  select * into u from auth.users
  where raw_user_meta_data->>'username' = lower(btrim(p_username))
  limit 1;
  if u.id is null then
    return jsonb_build_object('ok', false, 'error', 'usuario_nao_existe');
  end if;

  -- Throttle: 5 falhas em 10 minutos por conta (bloqueio temporário)
  select * into rec from public.login_attempts where username = u.id::text for update;
  if rec.username is not null
     and rec.window_start > now() - interval '10 minutes'
     and rec.attempts >= 5 then
    return jsonb_build_object('ok', false, 'error', 'muitas_tentativas');
  end if;

  if u.encrypted_password is null or u.encrypted_password = '' or u.encrypted_password = '!' then
    return jsonb_build_object('ok', false, 'error', 'sem_senha');
  end if;

  if crypt(p_password, u.encrypted_password) <> u.encrypted_password then
    insert into public.login_attempts (username, attempts, window_start)
    values (u.id::text, 1, now())
    on conflict (username) do update set
      attempts = case
        when public.login_attempts.window_start < now() - interval '10 minutes' then 1
        else public.login_attempts.attempts + 1
      end,
      window_start = case
        when public.login_attempts.window_start < now() - interval '10 minutes' then now()
        else public.login_attempts.window_start
      end;
    return jsonb_build_object('ok', false, 'error', 'senha_errada');
  end if;

  delete from public.login_attempts where username = u.id::text;
  return jsonb_build_object('ok', true, 'email', u.email);
end $$;

revoke all on function public.login_check(text, text) from public;
grant execute on function public.login_check(text, text) to anon, authenticated;

-- ============================================================
-- A-02: Storage avatars — só o DONO do uid no prefixo do path
-- (o app passa a subir em avatars/<uid>/<arquivo>)
-- ============================================================
drop policy if exists avatars_select on storage.objects;
drop policy if exists avatars_insert on storage.objects;
drop policy if exists avatars_update on storage.objects;
drop policy if exists avatars_delete on storage.objects;

create policy avatars_select on storage.objects
  for select to authenticated using (bucket_id = 'avatars');

create policy avatars_insert on storage.objects
  for insert to authenticated
  with check (bucket_id = 'avatars' and (storage.foldername(name))[1] = auth.uid()::text);

create policy avatars_update on storage.objects
  for update to authenticated
  using (bucket_id = 'avatars' and (storage.foldername(name))[1] = auth.uid()::text)
  with check (bucket_id = 'avatars' and (storage.foldername(name))[1] = auth.uid()::text);

create policy avatars_delete on storage.objects
  for delete to authenticated
  using (bucket_id = 'avatars' and (storage.foldername(name))[1] = auth.uid()::text);

-- ============================================================
-- A-03: profiles — visibilidade (public/friends/private) + localização opt-in
-- ============================================================
alter table public.profiles add column if not exists profile_visibility text not null default 'public'
  check (profile_visibility in ('public', 'friends', 'private'));
alter table public.profiles add column if not exists show_location boolean not null default false;

drop policy if exists profiles_select on public.profiles;
create policy profiles_select on public.profiles
  for select to authenticated
  using (
    id = auth.uid()
    or profile_visibility = 'public'
    or exists (
      select 1 from public.friendships f
      where f.status = 'accepted'
        and ((f.user_a = auth.uid() and f.user_b = public.profiles.id)
          or (f.user_a = public.profiles.id and f.user_b = auth.uid()))
    )
  );

-- View "segura": cidade/país só quando o dono optou por mostrar
create or replace view public.profiles_safe as
select id, username, display_name, avatar_url, steam_id, summary, showcase, created_at,
       case when show_location then city else null end as city,
       case when show_location then country else null end as country
from public.profiles;

revoke all on public.profiles_safe from public;
grant select on public.profiles_safe to authenticated;

-- ============================================================
-- A-04a: tabela blocks (pronta pra feature de bloqueio)
-- A-04a/b: friend_achievements respeita bloqueios + privacidade
-- ============================================================
create table if not exists public.blocks (
  blocker_id uuid not null references public.profiles(id) on delete cascade,
  blocked_id uuid not null references public.profiles(id) on delete cascade,
  created_at timestamptz not null default now(),
  primary key (blocker_id, blocked_id),
  check (blocker_id <> blocked_id)
);

create or replace function public.friend_achievements(p_friend uuid)
returns table (appid text, apiname text, unlocked_at timestamptz, updated_at timestamptz, title text, icon text, percent real)
language plpgsql security definer set search_path = public as $$
begin
  if p_friend = auth.uid() then
    return; -- nem as próprias conquistas via essa RPC
  end if;
  if not exists (
    select 1 from public.friendships
    where status = 'accepted'
      and ((user_a = auth.uid() and user_b = p_friend)
        or (user_a = p_friend and user_b = auth.uid()))
  ) then
    return;
  end if;
  if exists (
    select 1 from public.blocks
    where (blocker_id = p_friend and blocked_id = auth.uid())
       or (blocker_id = auth.uid() and blocked_id = p_friend)
  ) then
    return; -- bloqueado em qualquer direção
  end if;
  if exists (
    select 1 from public.profiles p
    where p.id = p_friend and p.profile_visibility = 'private'
  ) then
    return; -- perfil privado: conquistas não vazam nem pra amigos
  end if;
  return query
    select ua.appid, ua.apiname, ua.unlocked_at, ua.updated_at, ua.title, ua.icon, ua.percent
    from public.user_achievements ua
    where ua.user_id = p_friend
    order by ua.unlocked_at desc
    limit 30;
end $$;

revoke all on function public.friend_achievements(uuid) from public;
grant execute on function public.friend_achievements(uuid) to authenticated;

-- ============================================================
-- A-05a: sync_achievements — sem backdating (unlocked_at <= now) + sem nulo
-- ============================================================
create or replace function public.sync_achievements(p_items jsonb)
returns setof public.user_achievements
language plpgsql security invoker set search_path = public as $$
begin
  return query
  insert into public.user_achievements (user_id, appid, apiname, unlocked_at)
  select
    auth.uid(),
    (i->>'appid'),
    (i->>'apiname'),
    least(
      to_timestamp(
        case when (i->>'unlocked_at') ~ '^[0-9]+(\.[0-9]+)?$'
             then (i->>'unlocked_at')::double precision end
      ),
      now()
    )
  from jsonb_array_elements(p_items) i
  where (i->>'appid') is not null and (i->>'apiname') is not null
    and (i->>'unlocked_at') ~ '^[0-9]+(\.[0-9]+)?$'
  on conflict (user_id, appid, apiname)
  do update set
    unlocked_at = least(user_achievements.unlocked_at, excluded.unlocked_at),
    updated_at  = now()
  where user_achievements.unlocked_at > excluded.unlocked_at
  returning *;
end $$;

revoke all on function public.sync_achievements(jsonb) from public;
grant execute on function public.sync_achievements(jsonb) to authenticated;

-- ============================================================
-- A-05b: push_library — horas clampadas em [0, 999999]
-- ============================================================
create or replace function public.push_library(p_lib jsonb, p_playtime jsonb)
returns void
language plpgsql security invoker set search_path = public as $$
begin
  if p_lib is not null and jsonb_array_length(p_lib) > 0 then
    insert into public.user_library (user_id, appid, title, platform, updated_at)
    select auth.uid(), (i->>'appid'), (i->>'title'), coalesce(i->>'platform', 'windows'), now()
    from jsonb_array_elements(p_lib) i
    where coalesce((i->>'removed')::boolean, false) is not true
    on conflict (user_id, appid)
    do update set title = excluded.title, platform = excluded.platform, updated_at = now();

    delete from public.user_library
    where user_id = auth.uid()
      and appid in (
        select i->>'appid' from jsonb_array_elements(p_lib) i
        where coalesce((i->>'removed')::boolean, false) is true
      );
  end if;

  if p_playtime is not null and jsonb_array_length(p_playtime) > 0 then
    insert into public.user_playtime (user_id, appid, minutes, updated_at)
    select auth.uid(), (i->>'appid'), greatest(0, least((i->>'minutes')::int, 999999)), now()
    from jsonb_array_elements(p_playtime) i
    where (i->>'appid') is not null and (i->>'minutes') ~ '^-?[0-9]+$'
    on conflict (user_id, appid)
    do update set
      minutes = greatest(0, least(public.user_playtime.minutes + excluded.minutes, 999999)),
      updated_at = now()
    where excluded.minutes > 0;
  end if;
end $$;

revoke all on function public.push_library(jsonb, jsonb) from public;
grant execute on function public.push_library(jsonb, jsonb) to authenticated;

-- ============================================================
-- A-22/A-23: friendships — par imutável, requester não auto-aceita,
-- insert exige que o requester seja do par
-- ============================================================
drop policy if exists friends_update on public.friendships;
create policy friends_update on public.friendships
  for update to authenticated
  using (auth.uid() in (user_a, user_b))
  with check (auth.uid() in (user_a, user_b) and status in ('accepted', 'blocked'));

drop policy if exists friends_insert on public.friendships;
create policy friends_insert on public.friendships
  for insert to authenticated
  with check (auth.uid() = requester_id and auth.uid() in (user_a, user_b) and status = 'pending');

create or replace function public.friendships_update_guard()
returns trigger
language plpgsql security definer set search_path = public as $$
begin
  if new.user_a <> old.user_a or new.user_b <> old.user_b then
    raise exception 'par de amizade imutável';
  end if;
  if old.status = 'pending' and new.status = 'accepted' and auth.uid() = new.requester_id then
    raise exception 'quem enviou o pedido não pode aceitar';
  end if;
  if new.status not in ('pending', 'accepted', 'blocked') then
    raise exception 'status inválido';
  end if;
  return new;
end $$;

drop trigger if exists friendships_update_guard on public.friendships;
create trigger friendships_update_guard
  before update on public.friendships
  for each row execute function public.friendships_update_guard();

-- ============================================================
-- A-24: username_available funcionando pra anon (definer) + reservados
-- ============================================================
create table if not exists public.reserved_usernames (
  username text primary key
);

insert into public.reserved_usernames (username) values
  ('admin'), ('moderator'), ('support'), ('staff'), ('arcadia'), ('system'),
  ('official'), ('bot'), ('null'), ('undefined'), ('root'), ('test'), ('teste')
on conflict do nothing;

create or replace function public.username_available(p_username text)
returns boolean
language sql security definer set search_path = public as $$
  select not exists (
    select 1 from public.profiles where username = lower(btrim(p_username))
    union all
    select 1 from public.reserved_usernames where username = lower(btrim(p_username))
  );
$$;

revoke all on function public.username_available(text) from public;
grant execute on function public.username_available(text) to anon, authenticated;

-- ============================================================
-- A-19: trigger handle_new_user valida formato (fallback player+id)
-- ============================================================
create or replace function public.handle_new_user()
returns trigger
language plpgsql security definer set search_path = public as $$
declare
  base text;
  cand text;
  n int := 0;
begin
  base := lower(btrim(coalesce(
    nullif(trim(new.raw_user_meta_data->>'username'), ''),
    'player' || substr(new.id::text, 1, 8)
  )));
  if base !~ '^[a-z0-9_]{3,20}$' then
    base := 'player' || substr(new.id::text, 1, 8);
  end if;
  loop
    cand := base || case when n = 0 then '' else '_' || n::text end;
    exit when not exists (select 1 from public.profiles where username = cand)
              and not exists (select 1 from public.reserved_usernames where username = cand);
    n := n + 1;
  end loop;
  insert into public.profiles (id, username) values (new.id, cand);
  return new;
end $$;

drop trigger if exists on_auth_user_created on auth.users;
create trigger on_auth_user_created
  after insert on auth.users
  for each row execute function public.handle_new_user();

-- ============================================================
-- A-20: revogar EXECUTE público das demais funções
-- ============================================================
revoke all on function public.pull_achievements(timestamptz) from public;
revoke all on function public.pull_library() from public;
