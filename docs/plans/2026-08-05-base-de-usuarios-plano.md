# Plano de Implementação — Base de Usuários, Amigos e Sync de Conquistas

**Data:** 2026-08-05
**Projeto:** Arcadia (`/home/zes/.local/share/arcadia`)
**Base:** design em `docs/specs/2026-08-05-base-de-usuarios-design.md`
**Stack:** Electron 33 + React 18 + Vite 6 + TS 5.7 + @supabase/supabase-js 2.112.1

> Ordem de execução: cada fase termina com critérios de pronto verificáveis. Fases 0-1 podem andar em paralelo com o dia a dia; 2-5 são sequenciais.

---

## Fase 0 — Preparação (dependências + banco)

**Tarefas:**
- [ ] 0.1 Instalar dependência: `cd app && npm install @supabase/supabase-js@2.112.1` (pin exato; adicionar em `dependencies` no package.json)
- [ ] 0.2 Criar conta/projeto no supabase.com (plano grátis) — anotar `Project URL` e `anon key`
- [ ] 0.3 Colar `docs/sql/schema.sql` no SQL Editor do Supabase (tabelas + RLS + trigger + RPCs) e executar
- [ ] 0.4 Criar `app/electron/supabase/config.js`:
      ```js
      module.exports = {
        url: process.env.SUPABASE_URL || 'https://SEU-PROJETO.supabase.co',
        anonKey: process.env.SUPABASE_ANON_KEY || 'SUA-ANON-KEY',
      }
      ```
- [ ] 0.5 Testar no SQL Editor: `select username_available('teste');` → `true`

**Critérios de pronto:**
- [ ] `npm ls @supabase/supabase-js` mostra 2.112.1
- [ ] Schema aplicado sem erros (3 tabelas + índices + políticas + 3 RPCs + trigger)
- [ ] RPC `username_available` responde

---

## Fase 1 — Cliente + sessão (fundação)

**Arquivos novos:**
- `app/electron/supabase/client.js` — `createClient(url, anonKey)` (singleton)
- `app/electron/supabase/session.js` — persistência em `~/.local/share/arcadia/session.json`

**Tarefas:**
- [ ] 1.1 `session.js`: escrita atômica (tmp + rename, padrão do projeto), permissão 0600, `safeStorage.encryptString` quando `isEncryptionAvailable()`, fallback texto puro com flag `encrypted: false`
- [ ] 1.2 `client.js`: exporta `getClient()` + `getSession()`/`setSession()`/`clearSession()` (via session.js)
- [ ] 1.3 Adicionar `session.json` e fila de sync ao `.gitignore`
- [ ] 1.4 Testes unitários (`node --test`): escrita/leitura/limpeza da sessão, modo criptografado vs fallback

**Critérios de pronto:**
- [ ] `node --test` passa
- [ ] Sessão sobrevive a restart do app (teste manual: gravar, reler)

---

## Fase 2 — Auth (cadastro/login/OTP)

**Arquivos novos:**
- `app/electron/supabase/auth.js`
- `src/components/desktop/AuthDialog.tsx`
- `src/components/account/AccountContext.tsx` (+ `useAccount`)

**Arquivos alterados:**
- `app/electron/main.js` (registrar IPC: `accountSignUp`, `accountSignIn`, `accountSignOut`, `accountVerifyOtp`, `accountResetPassword`, `accountStatus`, `onAuthChanged`)
- `app/electron/preload.js` (expor `accountAPI`)
- `src/global.d.ts` (tipos: `AccountSession`, `AuthResult`, etc.)
- `src/i18n/*.json` (chaves de auth)

**Tarefas:**
- [ ] 2.1 `auth.js`: `signUp(email, password, username)` — checa `username_available` via RPC antes; `signIn`, `signOut`, `verifyOtp(email, token)` (6 dígitos), `resetPassword(email)`, `onAuthStateChange` → persiste/limpa session.json
- [ ] 2.2 IPC handlers em main.js + eventos `auth-changed` para o renderer
- [ ] 2.3 `AccountContext`: estado `{ status: 'logged-out'|'pending'|'logged-in', session }`, expõe `login/signup/logout/verifyOtp`
- [ ] 2.4 `AuthDialog`: abas Entrar / Criar conta (username + email + senha); tela de OTP após cadastro; tela de recuperação
- [ ] 2.5 i18n pt-BR/en/es; abrir diálogo via menu de perfil (desktop) e UserMenu (PS5, mínimo)

**Critérios de pronto:**
- [ ] Cadastro com email real recebe OTP e valida (teste manual com email do usuário)
- [ ] Sessão persiste entre restarts (relogin automático via session.json)
- [ ] Logout limpa sessão

---

## Fase 3 — Amigos

**Arquivos novos:**
- `app/electron/supabase/friends.js`
- `src/components/desktop/FriendsView.tsx`

**Arquivos alterados:**
- `app/electron/main.js` (IPC: `friendsSearch`, `friendsSend`, `friendsAccept`, `friendsCancel`, `friendsList`, `onFriendRequest`; assinatura Realtime)
- `app/electron/preload.js`, `src/global.d.ts`, `src/i18n/*.json`
- `src/components/desktop/Sidebar.tsx` (novo item `amigos` em `DesktopView`)
- `src/components/desktop/DesktopLauncher.tsx` (render do FriendsView)

**Tarefas:**
- [ ] 3.1 `friends.js`: `search(query)` — `username ILIKE 'q%'`, exclui self; `send(toUserId)` — insert com par canônico ordenado + requester_id; `accept(friendId)` — update pending→accepted (RLS garante addressee); `cancel(friendId)`; `list()` — aceitos + pendentes recebidos/enviados
- [ ] 3.2 Realtime: `supabase.channel('friends').on('postgres_changes', { table: 'friendships', filter: 'user_id=eq.<me>' })` no main → forward IPC `friend-request`
- [ ] 3.3 FriendsView: busca com debounce, resultados, botões enviar/aceitar/cancelar, lista de amigos, badge de pedidos
- [ ] 3.4 Integração Sidebar + DesktopLauncher + contador de pedidos no sidebar
- [ ] 3.5 Teste unitário: par canônico (ordenação user_a < user_b)

**Critérios de pronto:**
- [ ] Teste manual com 2 contas (email diferente): busca → pedido → notificação (Realtime) → aceite → aparece na lista
- [ ] Duplicatas impossíveis (PK + CHECK)

---

## Fase 4 — Sync de conquistas

**Arquivos novos:**
- `app/electron/supabase/sync.js`

**Arquivos alterados:**
- `app/electron/main.js` — hook no `unlockAchievement()` (enfileira sem bloquear), chamadas de sync no boot/login, IPC `syncNow`, `syncStatus`, evento `sync-state`
- `app/electron/preload.js`, `src/global.d.ts`, `src/i18n/*.json`
- `src/components/desktop/AchievementsPanel.tsx` — banner dinâmico (conectado/sincronizando/offline) usando `conquistas.aviso_offline` existente
- `src/components/desktop/SyncStatusIndicator.tsx` (novo) — status + botão "Sincronizar agora"

**Tarefas:**
- [ ] 4.1 `sync.js`: fila local (JSON atômico, ex: `sync_queue.json`); `normalizeTs(v)` (10 dígitos seg → 13 ms → Date); `pushDelta()` — chama RPC `sync_achievements` com lote e drena fila; `pullDelta()` — RPC `pull_achievements(lastPullAt)`, aplica no achievements.json (desbloqueado no server e não local → achieved+timestamp; local anterior → mantém); `reconcile()` no login/boot
- [ ] 4.2 Hook no `unlockAchievement`: `sync.enqueue(...)` em try/catch, sem await no caminho do launch
- [ ] 4.3 `syncNow` (manual) + evento `sync-state` para o renderer
- [ ] 4.4 Retry com backoff (offline → 5min máx; 429 → backoff exponencial; RLS error → descarta + loga)
- [ ] 4.5 Testes unitários: merge earliest-wins, normalização 10/13 dígitos, fila com client fake (sem rede)

**Critérios de pronto:**
- [ ] Desbloquear conquista offline → fila; religar internet → sync automático (teste com 2 máquinas: máquina B recebe a conquista da máquina A após sync)
- [ ] Reconcile no login não duplica nem regride conquistas
- [ ] `node --test` verde

---

## Fase 5 — Verificação final

**Tarefas:**
- [ ] 5.1 `npm run build` (Vite) sem erros de tipo
- [ ] 5.2 Teste de regressão: launcher abre, biblioteca/carrossel/conquistas locais funcionam sem login
- [ ] 5.3 Teste em 2 máquinas (ou 2 perfis): conta A desbloqueia → conta B sincroniza
- [ ] 5.4 Fluxo completo de amigos (busca, pedido, notificação, aceite)
- [ ] 5.5 Logout + login de volta não perde fila nem conquistas locais
- [ ] 5.6 (Opcional) Testar RLS: usuário B tentando ler conquistas do A via SQL retorna vazio

**Critérios de pronto:**
- [ ] Todos os testes acima passam
- [ ] `git status` limpo exceto arquivos esperados; `session.json`/fila fora do git

---

## Resumo de arquivos por tipo

| Tipo | Arquivos |
|---|---|
| Novo (main) | `app/electron/supabase/{config,client,session,auth,friends,sync}.js` |
| Novo (renderer) | `AuthDialog.tsx`, `FriendsView.tsx`, `SyncStatusIndicator.tsx`, `AccountContext.tsx` |
| Alterado (main) | `main.js`, `preload.js` |
| Alterado (renderer) | `Sidebar.tsx`, `DesktopLauncher.tsx`, `AchievementsPanel.tsx`, `UserMenu.tsx`, `global.d.ts`, 3× i18n |
| SQL | `docs/sql/schema.sql` |
| Config | `app/package.json`, `.gitignore` |
| Testes | `node --test` (suíte em `app/test/`) |

## Riscos de execução

- **Relay/instabilidade de rede:** a fila local garante que nada se perde; retry com backoff.
- **Atraso do email OTP (provedor):** orientar usuário a checar spam; reenviar OTP.
- **safeStorage indisponível (Linux sem keyring):** fallback texto puro documentado; sessão ainda funcional.
- **Free tier do Supabase (pausa de projeto ocioso):** reativar com 1 clique no dashboard.
