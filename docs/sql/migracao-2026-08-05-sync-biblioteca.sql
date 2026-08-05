-- ============================================================
-- Arcadia - Migracao v5: sync de BIBLIOTECA (jogos custom) + HORAS por conta
-- Cada conta tem a própria coleção no servidor; jogos seguem entre máquinas.
-- Idempotente. RODAR UMA VEZ no SQL Editor.
-- ============================================================

-- ---------- Tabelas ----------
create table if not exists public.user_library (
  user_id    uuid not null references auth.users(id) on delete cascade,
  appid      text not null,
  title      text not null default '',
  platform   text not null default 'windows',
  added_at   timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  primary key (user_id, appid)
);

create table if not exists public.user_playtime (
  user_id    uuid not null references auth.users(id) on delete cascade,
  appid      text not null,
  minutes    int not null default 0,
  updated_at timestamptz not null default now(),
  primary key (user_id, appid)
);

-- ---------- RLS (só o dono vê/edita) ----------
alter table public.user_library enable row level security;
alter table public.user_playtime enable row level security;

do $$
begin
  if not exists (select 1 from pg_policies where schemaname='public' and tablename='user_library' and policyname='ulib_select') then
    create policy ulib_select on public.user_library for select to authenticated using (auth.uid() = user_id);
  end if;
  if not exists (select 1 from pg_policies where schemaname='public' and tablename='user_library' and policyname='ulib_insert') then
    create policy ulib_insert on public.user_library for insert to authenticated with check (auth.uid() = user_id);
  end if;
  if not exists (select 1 from pg_policies where schemaname='public' and tablename='user_library' and policyname='ulib_update') then
    create policy ulib_update on public.user_library for update to authenticated using (auth.uid() = user_id);
  end if;
  if not exists (select 1 from pg_policies where schemaname='public' and tablename='user_library' and policyname='ulib_delete') then
    create policy ulib_delete on public.user_library for delete to authenticated using (auth.uid() = user_id);
  end if;

  if not exists (select 1 from pg_policies where schemaname='public' and tablename='user_playtime' and policyname='upt_select') then
    create policy upt_select on public.user_playtime for select to authenticated using (auth.uid() = user_id);
  end if;
  if not exists (select 1 from pg_policies where schemaname='public' and tablename='user_playtime' and policyname='upt_insert') then
    create policy upt_insert on public.user_playtime for insert to authenticated with check (auth.uid() = user_id);
  end if;
  if not exists (select 1 from pg_policies where schemaname='public' and tablename='user_playtime' and policyname='upt_update') then
    create policy upt_update on public.user_playtime for update to authenticated using (auth.uid() = user_id);
  end if;
  if not exists (select 1 from pg_policies where schemaname='public' and tablename='user_playtime' and policyname='upt_delete') then
    create policy upt_delete on public.user_playtime for delete to authenticated using (auth.uid() = user_id);
  end if;
end $$;

-- ---------- RPC: PUSH (jogos upsert/removidos + delta de horas) ----------
create or replace function public.push_library(p_lib jsonb, p_playtime jsonb)
returns void
language plpgsql security invoker set search_path = public as $$
begin
  if p_lib is not null and jsonb_array_length(p_lib) > 0 then
    -- adiciona/renomeia
    insert into public.user_library (user_id, appid, title, platform, updated_at)
    select auth.uid(), (i->>'appid'), (i->>'title'), coalesce(i->>'platform', 'windows'), now()
    from jsonb_array_elements(p_lib) i
    where coalesce((i->>'removed')::boolean, false) is not true
    on conflict (user_id, appid)
    do update set title = excluded.title, platform = excluded.platform, updated_at = now();
    -- remove (tombstone)
    delete from public.user_library
    where user_id = auth.uid()
      and appid in (
        select i->>'appid' from jsonb_array_elements(p_lib) i
        where coalesce((i->>'removed')::boolean, false) is true
      );
  end if;

  if p_playtime is not null and jsonb_array_length(p_playtime) > 0 then
    -- horas: ACUMULA delta (cada máquina manda o que jogou desde o último push)
    insert into public.user_playtime (user_id, appid, minutes, updated_at)
    select auth.uid(), (i->>'appid'), greatest(0, (i->>'minutes')::int), now()
    from jsonb_array_elements(p_playtime) i
    on conflict (user_id, appid)
    do update set minutes = public.user_playtime.minutes + excluded.minutes, updated_at = now()
    where excluded.minutes > 0;
  end if;
end $$;

grant execute on function public.push_library(jsonb, jsonb) to authenticated;

-- ---------- RPC: PULL (biblioteca + horas da conta) ----------
create or replace function public.pull_library()
returns table (appid text, title text, platform text, minutes int)
language plpgsql security invoker set search_path = public as $$
begin
  return query
    select l.appid, l.title, l.platform, coalesce(p.minutes, 0) as minutes
    from public.user_library l
    left join public.user_playtime p on p.user_id = l.user_id and p.appid = l.appid
    where l.user_id = auth.uid()
    order by l.added_at;
end $$;

grant execute on function public.pull_library() to authenticated;
