-- ============================================================
-- v7 — RPCs restritos: anon NÃO executa (menor privilégio)
-- 2026-08-06 — auditoria de vazamento (F1): o servidor estava
-- regrantando execute ao anon apesar dos revokes da v6.
-- Idempotente: pode rodar quantas vezes quiser.
-- ============================================================

-- Conquistas (push/pull) — só usuário logado
revoke all on function public.sync_achievements(jsonb) from public, anon;
grant execute on function public.sync_achievements(jsonb) to authenticated;

revoke all on function public.pull_achievements(timestamptz) from public, anon;
grant execute on function public.pull_achievements(timestamptz) to authenticated;

-- Biblioteca/horas (push/pull) — só usuário logado
revoke all on function public.push_library(jsonb, jsonb) from public, anon;
grant execute on function public.push_library(jsonb, jsonb) to authenticated;

revoke all on function public.pull_library() from public, anon;
grant execute on function public.pull_library() to authenticated;

-- Conquistas de amigo — só usuário logado
revoke all on function public.friend_achievements(uuid) from public, anon;
grant execute on function public.friend_achievements(uuid) to authenticated;

-- View de perfil seguro — só usuário logado
revoke all on public.profiles_safe from public, anon;
grant select on public.profiles_safe to authenticated;

-- ============================================================
-- CONFERÊNCIA: RPCs que o ANON PRECISA continuam liberados
-- (cadastro instantâneo e login por username, sem verificação)
-- ============================================================
grant execute on function public.login_check(text, text) to anon, authenticated;
grant execute on function public.username_available(text) to anon, authenticated;
