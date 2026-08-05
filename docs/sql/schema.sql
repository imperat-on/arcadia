-- ============================================================
-- Arcadia — Schema de Usuários, Amigos e Conquistas (Supabase)
-- Colar no SQL Editor do Supabase e executar (uma vez).
-- Plano: docs/plans/2026-08-05-base-de-usuarios-plano.md (Fase 0)
-- ============================================================

-- ---------- 1) PROFILES ----------
create table public.profiles (
  id         uuid primary key references auth.users(id) on delete cascade,
  username   text not null unique check (username ~ '^[a-z0-9_]{3,20}$'),
  avatar_url text,
  steam_id   text,
  created_at timestamptz not null default now()
);

-- Índice para busca por prefixo (amigos): username like 'q%'
create index on public.profiles (username text_pattern_ops);

-- ---------- 2) FRIENDSHIPS (uma linha canônica por par) ----------
create table public.friendships (
  user_a       uuid not null references public.profiles(id) on delete cascade,
  user_b       uuid not null references public.profiles(id) on delete cascade,
  requester_id uuid not null references public.profiles(id),
  status       text not null default 'pending'
               check (status in ('pending', 'accepted', 'blocked')),
  created_at   timestamptz not null default now(),
  updated_at   timestamptz not null default now(),
  primary key (user_a, user_b),
  check (user_a < user_b)
);

create index on public.friendships (user_a, status);
create index on public.friendships (user_b, status);

-- ---------- 3) USER_ACHIEVEMENTS (append-only; sem DELETE) ----------
create table public.user_achievements (
  user_id     uuid not null references public.profiles(id) on delete cascade,
  appid       text not null,
  apiname     text not null,
  unlocked_at timestamptz not null,
  updated_at  timestamptz not null default now(),
  primary key (user_id, appid, apiname)
);

-- Delta pull: syncs por (user, updated_at)
create index on public.user_achievements (user_id, updated_at);

-- ---------- Trigger: cria profile no signup (com fallback de colisão) ----------
create or replace function public.handle_new_user()
returns trigger
language plpgsql security definer set search_path = public as $$
declare
  base text;
  cand text;
  n int := 0;
begin
  base := coalesce(
    nullif(trim(new.raw_user_meta_data->>'username'), ''),
    'player' || substr(new.id::text, 1, 8)
  );
  loop
    cand := base || case when n = 0 then '' else '_' || n::text end;
    exit when not exists (select 1 from public.profiles where username = cand);
    n := n + 1;
  end loop;
  insert into public.profiles (id, username) values (new.id, cand);
  return new;
end $$;

drop trigger if exists on_auth_user_created on auth.users;
create trigger on_auth_user_created
  after insert on auth.users
  for each row execute function public.handle_new_user();

-- ---------- RLS: PROFILES ----------
alter table public.profiles enable row level security;

create policy profiles_select on public.profiles
  for select to authenticated using (true);
create policy profiles_insert on public.profiles
  for insert to authenticated with check (auth.uid() = id);
create policy profiles_update on public.profiles
  for update to authenticated using (auth.uid() = id);

-- ---------- RLS: FRIENDSHIPS ----------
alter table public.friendships enable row level security;

create policy friends_select on public.friendships
  for select to authenticated
  using (auth.uid() in (user_a, user_b));
create policy friends_insert on public.friendships
  for insert to authenticated
  with check (auth.uid() = requester_id and status = 'pending');
create policy friends_update on public.friendships
  for update to authenticated
  using (auth.uid() in (user_a, user_b));
-- Cancelar pedido (só o requester, só enquanto pending)
create policy friends_delete on public.friendships
  for delete to authenticated
  using (auth.uid() = requester_id and status = 'pending');

-- ---------- RLS: USER_ACHIEVEMENTS (self-only, append-only) ----------
alter table public.user_achievements enable row level security;

create policy ach_select on public.user_achievements
  for select to authenticated using (auth.uid() = user_id);
create policy ach_insert on public.user_achievements
  for insert to authenticated with check (auth.uid() = user_id);
create policy ach_update on public.user_achievements
  for update to authenticated using (auth.uid() = user_id);
-- sem política de delete (append-only)

-- ---------- RPC: PUSH (primeiro desbloqueio vence) ----------
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
    to_timestamp((i->>'unlocked_at')::double precision)
  from jsonb_array_elements(p_items) i
  on conflict (user_id, appid, apiname)
  do update set
    unlocked_at = least(user_achievements.unlocked_at, excluded.unlocked_at),
    updated_at  = now()
  where user_achievements.unlocked_at > excluded.unlocked_at
  returning *;
end $$;

grant execute on function public.sync_achievements(jsonb) to authenticated;

-- ---------- RPC: PULL (delta desde o último sync) ----------
create or replace function public.pull_achievements(p_since timestamptz)
returns setof public.user_achievements
language plpgsql security invoker set search_path = public as $$
begin
  return query
  select *
  from public.user_achievements
  where user_id = auth.uid()
    and (p_since is null or updated_at > p_since)
  order by updated_at;
end $$;

grant execute on function public.pull_achievements(timestamptz) to authenticated;

-- ---------- RPC: disponibilidade de username ----------
create or replace function public.username_available(p_username text)
returns boolean
language sql security invoker set search_path = public as $$
  select not exists (select 1 from public.profiles where username = p_username);
$$;

grant execute on function public.username_available(text) to authenticated;
