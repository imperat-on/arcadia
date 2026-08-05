# Design — Base de Usuários, Amigos e Sincronização de Conquistas

**Data:** 2026-08-05
**Projeto:** Arcadia (`/home/zes/.local/share/arcadia`)
**Status:** Design validado (produzido pelo modelo de planejamento claude-opus-5 + revisão)

---

## 1. Contexto do projeto (explorado no código)

- **Stack:** Electron 33.4.11 (Node 20 interno) + React 18.3.1 + Vite 6 + Tailwind 4 + TypeScript 5.7.2. Node do sistema: v26.4.0, npm 12.0.2.
- **Dados hoje (100% local, JSON):**
  - `achievements.json` — fonte da verdade das conquistas para a UI.
  - `achievements_store.json` — cache de schema scrapado da Steam (`{appid: {at, items: [{apiname, title, desc, icon, icongray, achieved, unlock, percent}]}}`), TTL ~30 dias.
  - `library.json` (biblioteca), `config.json`, `profile_cache.json` (perfis Steam por steamid64).
- **Segurança do Electron:** `contextIsolation: true`, `webSecurity: DISABLED`, `webviewTag: enabled` (webview embutida da Steam). Isso torna o **renderer um lugar inseguro para tokens** — decisão central da arquitetura (ver §3).
- **Ponto único de desbloqueio:** `unlockAchievement()` no main process (main.js) — escreve no achievements, dispara IPC e toast. É o hook perfeito para o sync.
- **i18n:** JSON flat em pt-BR/en-US/es-ES. O pt-BR **já contém** `conquistas.aviso_offline`: "Suas conquistas não serão sincronizadas com a conta." → a UI já antecipava contas.
- **Navegação desktop:** `DesktopView` union em `Sidebar.tsx` (`inicio | biblioteca | lojas | plugins | downloads | fontes | config`) + switch em `DesktopLauncher.tsx`. Existe UI de perfil tanto no modo desktop quanto no PS5 (UserMenu/ProfilePage/ProfileSelect).
- **Testes:** não existe framework (sem vitest/jest). Decisão: usar o **test runner nativo do Node** (zero dependência).
- **Timestamps:** o `.bin` da Steam guarda epoch em **segundos** (10 dígitos); o `at` do achievements_store é em **milissegundos** (13 dígitos). A conversão precisa tratar os dois formatos defensivamente.
- **Nem toda conquista tem `apiname`** — só sincronizamos itens que possuem `apiname` (documentado como restrição).

---

## 2. Perguntas que seriam feitas + premissas assumidas

| Pergunta | Premissa assumida |
|---|---|
| Conta única por máquina ou por perfil local? | Conta única vinculada ao app; perfis locais continuam existindo como hoje (seleção de perfil não é conta online). |
| Quer login com Google/Steam ou só email? | MVP: email + senha apenas. OAuth fica para depois (YAGNI). |
| Deve funcionar offline? | Sim — offline-first: conquistas funcionam localmente e sincronizam quando há conexão. |
| Amigos podem ver conquistas uns dos outros? | **Não no MVP.** A RLS de conquistas é self-only. Extensão futura documentada. |
| Avatar/upload de imagem? | Não no MVP (usa iniciais como hoje). |
| Sincronizar o quê exatamente? | Apenas o estado desbloqueado (achieved + unlocked_at) — metadados (título/ícone) vêm do schema local da Steam. |
| Múltiplas máquinas? | Sim — esse é o objetivo do sync (push/pull/merge). |

---

## 3. Decisões de arquitetura (2-3 abordagens consideradas)

### 3.1 Onde roda o SDK do Supabase?
- **(A) Renderer direto** (supabase-js no React + localStorage p/ sessão): mais simples, menos código, realtime trivial. **Risco:** com `webSecurity: false` e webview da Steam embutida, qualquer script injetado (CSS/temas custom via insertCSS, página da webview) pode ler o token. **Rejeitado.**
- **(B) Main process com bridge IPC** (recomendado): SDK vive no main; renderer chama via `window.launcherAPI.*` (padrão já existente no preload). Token nunca entra no DOM. Realtime chega ao renderer via IPC events. Mais boilerplate, mas alinhado com a postura de segurança do app. **ESCOLHIDO.**
- **(C) Híbrido** (auth no main, queries no renderer com token repassado): o pior dos dois mundos (token transita pelo renderer + split de lógica). **Rejeitado.**

### 3.2 Onde guardar anon key e sessão?
- **Anon key** do Supabase **não é segredo** (é publicável por design; a segurança vem da RLS). Mesmo assim, fora do bundle do renderer: arquivo versionado `app/electron/supabase/config.js` com override por env vars (`SUPABASE_URL`, `SUPABASE_ANON_KEY`).
- **Tokens de sessão (access/refresh) SÃO segredo**: nunca em `config.json` (usuário pode editar/compartilhar). Ficam em `~/.local/share/arcadia/session.json` com permissão 0600, escrita atômica (padrão do projeto), fora do git. Criptografia com `safeStorage` do Electron (keychain do sistema) quando disponível, fallback texto puro (documentado).

### 3.3 Modelo de dados de amizade
- **(A) Uma linha canônica por par** (user_a < user_b ordenados, CHECK constraint, requester_id + status): sem duplicatas por construção, consultas simétricas simples. **ESCOLHIDO.**
- (B) Duas linhas espelhadas (uma por direção): consultas triviais mas risco de divergência/duplicata. Rejeitado.
- (C) Tabelas separadas `friend_requests` + `friends`: mais "puro" mas mais RLS e joins. Rejeitado (YAGNI no MVP).

### 3.4 Estratégia de merge de conquistas
- Desbloqueio é **monótono** (não existe "re-desbloquear" nem deleção legítima). O único conflito possível é o timestamp.
- **Regra: quem desbloqueou PRIMEIRO vence** (`earliest unlock_at wins`) — representa a verdade do primeiro desbloqueio.
- Implementação: **função RPC** (`sync_achievements`) com `INSERT ... ON CONFLICT DO UPDATE SET unlocked_at = LEAST(...)` — o upsert do SDK JS não suporta expressões no DO UPDATE; o RPC resolve no servidor de forma atômica.

---

## 4. Design do sistema

### 4.1 Arquitetura geral

```
┌───────────────────────── Electron ─────────────────────────┐
│  Renderer (React)                                          │
│   AuthDialog · FriendsView · SyncStatus · AchievementsPanel│
│        │ window.launcherAPI.* (contextBridge/preload)      │
│  Main process                                              │
│   supabase/client.js ─ session.js (safeStorage)            │
│   auth.js · friends.js · sync.js (fila + merge)            │
│   IPC handlers (main.js) + eventos (onAuthChanged, etc.)   │
└──────────────────────────┬─────────────────────────────────┘
                           │ HTTPS
                    ┌──────▼──────┐
                    │  Supabase   │  Postgres + Auth + Realtime (plano grátis)
                    └─────────────┘
```

- SDK **@supabase/supabase-js 2.112.1** (pin exato, convenção do projeto).
- Sessão persistida em `session.json` (0600, atômico, safeStorage).
- Sync disparado em: boot do app, login, desbloqueio (hook `unlockAchievement`), e manual (botão).
- Padrão de retorno IPC do projeto: `{ ok: boolean, error?: string }`.

### 4.2 Schema SQL (Supabase/Postgres)

```sql
-- 1) PROFILES (criado por trigger no signup)
create table public.profiles (
  id          uuid primary key references auth.users(id) on delete cascade,
  username    text not null unique check (username ~ '^[a-z0-9_]{3,20}$'),
  avatar_url  text,
  steam_id    text,
  created_at  timestamptz not null default now()
);
create index on public.profiles (username text_pattern_ops); -- busca prefixo

-- 2) FRIENDSHIPS (uma linha canônica por par)
create table public.friendships (
  user_a       uuid not null references public.profiles(id) on delete cascade,
  user_b       uuid not null references public.profiles(id) on delete cascade,
  requester_id uuid not null references public.profiles(id),
  status       text not null default 'pending' check (status in ('pending','accepted','blocked')),
  created_at   timestamptz not null default now(),
  updated_at   timestamptz not null default now(),
  primary key (user_a, user_b),
  check (user_a < user_b)
);
create index on public.friendships (user_b, status);
create index on public.friendships (user_a, status);

-- 3) USER_ACHIEVEMENTS (append-only; sem DELETE)
create table public.user_achievements (
  user_id    uuid not null references public.profiles(id) on delete cascade,
  appid      text not null,
  apiname    text not null,
  unlocked_at timestamptz not null,
  updated_at timestamptz not null default now(),
  primary key (user_id, appid, apiname)
);
create index on public.user_achievements (user_id, updated_at); -- delta pull

-- Trigger: perfil automático no signup (com fallback de username em colisão)
create function public.handle_new_user() returns trigger
language plpgsql security definer set search_path = public as $$
declare base text; cand text; n int := 0;
begin
  base := coalesce(nullif(trim(new.raw_user_meta_data->>'username'), ''), 'player' || substr(new.id::text, 1, 8));
  loop
    cand := base || case when n = 0 then '' else '_' || n::text end;
    exit when not exists (select 1 from public.profiles where username = cand);
    n := n + 1;
  end loop;
  insert into public.profiles (id, username) values (new.id, cand);
  return new;
end $$;
create trigger on_auth_user_created after insert on auth.users
  for each row execute function public.handle_new_user();
```

**Políticas RLS:**

```sql
alter table public.profiles enable row level security;
create policy profiles_select on public.profiles for select to authenticated using (true);
create policy profiles_insert on public.profiles for insert to authenticated with check (auth.uid() = id);
create policy profiles_update on public.profiles for update to authenticated using (auth.uid() = id);

alter table public.friendships enable row level security;
create policy friends_select on public.friendships for select to authenticated
  using (auth.uid() in (user_a, user_b));
create policy friends_insert on public.friendships for insert to authenticated
  with check (auth.uid() = requester_id and status = 'pending');
create policy friends_update on public.friendships for update to authenticated
  using (auth.uid() in (user_a, user_b));
create policy friends_delete on public.friendships for delete to authenticated
  using (auth.uid() = requester_id and status = 'pending'); -- cancelar pedido

alter table public.user_achievements enable row level security;
create policy ach_select on public.user_achievements for select to authenticated using (auth.uid() = user_id);
create policy ach_insert on public.user_achievements for insert to authenticated with check (auth.uid() = user_id);
create policy ach_update on public.user_achievements for update to authenticated using (auth.uid() = user_id);
-- sem política de delete: append-only
```

**Funções RPC (invoker security, grant a authenticated):**

```sql
-- Push: upsert com "primeiro desbloqueio vence"; RETURNING só linhas que mudaram
create or replace function public.sync_achievements(p_items jsonb)
returns setof public.user_achievements
language plpgsql security invoker set search_path = public as $$
begin
  return query
  insert into public.user_achievements (user_id, appid, apiname, unlocked_at)
  select auth.uid(), (i->>'appid'), (i->>'apiname'),
         to_timestamp((i->>'unlocked_at')::double precision)
  from jsonb_array_elements(p_items) i
  on conflict (user_id, appid, apiname)
  do update set unlocked_at = least(user_achievements.unlocked_at, excluded.unlocked_at),
                updated_at = now()
  where user_achievements.unlocked_at > excluded.unlocked_at
  returning *;
end $$;

-- Pull: delta desde o último sync
create or replace function public.pull_achievements(p_since timestamptz)
returns setof public.user_achievements
language plpgsql security invoker set search_path = public as $$
begin
  return query
  select * from public.user_achievements
  where user_id = auth.uid()
    and (p_since is null or updated_at > p_since)
  order by updated_at;
end $$;

-- Disponibilidade de username (pré-checagem no signup)
create or replace function public.username_available(p_username text)
returns boolean language sql security invoker set search_path = public as $$
  select not exists (select 1 from public.profiles where username = p_username);
$$;
```

### 4.3 Auth (fluxo)

- **Cadastro:** email + senha + username. App chama `username_available` antes; `signUp` do Supabase com metadata `{username}`; trigger cria profile (com fallback de colisão).
- **Confirmação de email:** **OTP de 6 dígitos digitado no app** (fluxo `verifyOtp`) em vez de magic-link/deep-link — funciona 100% offline do app (não depende de registrar protocolo `arcadia://`), melhor para desktop.
- **Recuperação de senha:** `resetPasswordForEmail` com o mesmo padrão OTP.
- **Sessão:** `onAuthStateChange` no main; persistida em `session.json`; `signOut` limpa sessão **sem apagar a fila de sync pendente**.
- **IPC exposto:** `accountSignUp`, `accountSignIn`, `accountSignOut`, `accountVerifyOtp`, `accountResetPassword`, `accountStatus`, `onAuthChanged`.

### 4.4 Sync de conquistas (offline-first)

- **Push (delta):** ao desbloquear (hook `unlockAchievement`), enfileira `{appid, apiname, unlocked_at}` na fila local (JSON atômico). Em segundo plano (sem bloquear o launch do jogo): chama `sync_achievements` (RPC) e drena a fila.
- **Pull (delta):** guarda `lastPullAt`; chama `pull_achievements(p_since)`; aplica localmente: desbloqueado no servidor e não local → marca achieved + timestamp; já local com `unlocked_at` anterior → mantém local (próximo push corrige o servidor).
- **Reconcile completo no login e no boot.**
- **Conflitos:** regra única "earliest wins" no servidor (`least()` no upsert) e mesma regra no merge local. Sem deleção.
- **Restrição:** itens sem `apiname` não sincronizam.
- **Normalização de timestamp:** função defensiva aceita 10 dígitos (segundos, formato .bin da Steam) ou 13 (ms, formato `at` do store).

### 4.5 Amigos

- **Busca:** `select ... where username like 'prefixo%'` (case-insensitive via lowercase do input; índice `text_pattern_ops`). Exclui o próprio usuário e já-amigos/pendentes.
- **Pedido:** insert `friendships` com `requester_id = auth.uid()`, status `pending`, par canônico ordenado.
- **Aceite:** update do addressee quando status = pending → accepted.
- **Cancelar:** delete do requester enquanto pending.
- **Notificação:** **Realtime** do Supabase (`postgres_changes` filtrando `user_id` na tabela friendships) no main process → forward via IPC (`onFriendRequest`). Fallback: poll ao abrir a tela de amigos.
- **Lista:** duas queries simples (amigos aceitos; pedidos recebidos/pendentes) via join canônico.

### 4.6 Error handling

| Erro | Tratamento |
|---|---|
| Offline | Enfileira tudo; indicador de "offline" na UI; drena na volta da conexão |
| Rate limit (429) | Backoff exponencial até 5 min, máximo de retries |
| Erro de RLS/permissão | Descarta o item (é bug de programação, não transitório) e loga |
| Falha de auth (token expirado) | Limpa sessão, mantém fila, pede login |
| Falha de rede no stream/API | Retry com backoff; nunca perde a fila |

### 4.7 Testes

- Test runner nativo do Node (`node --test`), script `npm test` — zero dependência.
- Unidades: par canônico (user_a < user_b), merge "earliest wins", normalização de timestamp (10/13 dígitos), operações da fila (com client fake, sem rede).
- RLS: script SQL manual opcional para teste no Supabase (não automatizado no MVP).

### 4.8 YAGNI (o que NÃO fazer agora)

- Nada de ver conquistas de amigos (RLS self-only; extensão futura documentada).
- Sem OAuth (Google/Steam), sem upload de avatar, sem chat, sem leaderboard, sem web client.
- Sem dependência `electron-store` (usar padrão atômico do projeto).
- Sem magic-link/deep-link de confirmação (OTP resolve).

---

## 5. Arquivos a criar/alterar

**Main process (novos):**
- `app/electron/supabase/config.js` — URL + anon key (versionado, overrides por env)
- `app/electron/supabase/client.js` — criação do client
- `app/electron/supabase/session.js` — session.json (0600, atômico, safeStorage)
- `app/electron/supabase/auth.js` — signup/login/OTP/recovery/onAuthStateChange
- `app/electron/supabase/friends.js` — busca/pedido/aceite/cancelar/realtime
- `app/electron/supabase/sync.js` — fila, push/pull, merge, normalização de timestamp

**Main process (alterar):**
- `app/electron/main.js` — registrar IPC handlers (account/friends/sync), hook no `unlockAchievement`, eventos
- `app/electron/preload.js` — expor `accountAPI` seguindo o padrão launcherAPI

**SQL:**
- `docs/sql/schema.sql` — tabelas + RLS + RPCs + trigger (para colar no Supabase SQL Editor)

**Renderer (novos):**
- `src/components/desktop/AuthDialog.tsx` — login/cadastro/OTP/recuperação
- `src/components/desktop/FriendsView.tsx` — busca, pedidos, lista
- `src/components/account/AccountContext.tsx` (+ hook `useAccount`)
- `src/components/desktop/SyncStatusIndicator.tsx` — status online/offline + botão sync

**Renderer (alterar):**
- `src/components/desktop/Sidebar.tsx` — novo item "Amigos" no `DesktopView`
- `src/components/desktop/DesktopLauncher.tsx` — render do FriendsView + AuthDialog
- `src/components/desktop/AchievementsPanel.tsx` — banner dinâmico (conectado/sincronizado vs offline)
- `src/components/ps5-launcher/UserMenu.tsx` — entrada de login/logout
- `src/global.d.ts` — tipos do accountAPI + entidades (AccountSession, FriendRequest, etc.)
- `src/i18n/pt-BR.json`, `en-US.json`, `es-ES.json` — chaves de conta/amigos/sync

**Config:**
- `app/package.json` — `@supabase/supabase-js@2.112.1` (pin)
- `.gitignore` — `session.json`, fila de sync

---

## 6. Riscos e mitigação

| Risco | Mitigação |
|---|---|
| Relay/provedor de rede instável (fora do escopo do app) | Padrão de retry + fila local; nada se perde |
| Steam `.bin` com formatos de timestamp variados | Normalização defensiva 10/13 dígitos + testes |
| Conquistas antigas sem `apiname` | Restrição documentada: não sincroniza |
| Usuário com sessão expirada no meio de sync | Limpa sessão, mantém fila, re-login |
| Custo do Supabase free estourar | Limites documentados (500MB, 50k MAU, 5GB bw); schema mínimo (só ids+timestamps) |
| `webSecurity: false` no renderer | Token nunca sai do main process; RLS como camada final de segurança |
