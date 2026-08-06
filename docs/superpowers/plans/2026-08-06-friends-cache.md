# Cache de Amigos + Queries Paralelas — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Aba amigos/perfil abrem instantâneo (cache local + background) e as 2 queries do friends.list rodam em paralelo (~1.4s → ~0.6s).

**Architecture:** Cache `contas/<user>/friends_cache.json` no main (TTL 60s, atômico, 600) — list() serve cache + atualiza em background via evento `friends:changed`; ações invalidam o cache; Promise.all nas queries. Renderer: FriendsContext escuta o evento; ProfilePage console usa useFriends().

**Tech Stack:** Node/Electron (main), React context (renderer), Supabase client.

## Global Constraints

- Escrita atômica (tmp + rename) e `chmod 600` nos caches (padrão do projeto)
- Caminhos de conta via `caminhoConta` (padrão sync_state)
- Evento novo `friends:changed`; contrato `friendsList({forcar?})` retorna `{ok, data, deCache}`
- `npm test` (34/34), `tsc --noEmit`, `npm run build` verdes; verificação ad-hoc `/tmp/hermes-verify-*.js` executada e removida

---

### Task 1: friends.js — cache + Promise.all + invalidação + onAtualizado

**Files:**
- Modify: `app/electron/supabase/friends.js`

- [ ] **Step 1: Ler o friends.js completo** (247 linhas) e mapear `list`, `send`, `accept`, `cancel`, `requireUserId`, exports.
- [ ] **Step 2: Adicionar imports** (`fs`, `path`, `caminhoConta` de `./conta`).
- [ ] **Step 3: Implementar `buscarFresco(me, cacheFile)`** — Promise.all das 2 queries (friendships + profiles `.in()`), monta `{friends, incoming, outgoing}`, grava cache atômico 600, retorna data.
- [ ] **Step 4: Implementar `list({ forcar })`** — cache TTL 60s; cache válido+!forcar → retorna `{ok, data, deCache:true}` + background (buscarFresco → onAtualizado cb); senão síncrono `{ok, data, deCache:false}`.
- [ ] **Step 5: Ações invalidam o cache** — `send`/`accept`/`cancel`/demais mutações: `fs.rm(cacheFile, {force:true})`.
- [ ] **Step 6: Exportar `onAtualizado` setter + `list` com assinatura nova.**
- [ ] **Step 7: `node --check` + testes.**

### Task 2: ipc.js — forcar + friends:changed

**Files:**
- Modify: `app/electron/supabase/ipc.js`

- [ ] **Step 1: Handler `friends:list` repassa `{ forcar }`** do payload.
- [ ] **Step 2: `friends.onAtualizado((data) => broadcast("friends:changed", data))`.**
- [ ] **Step 3: `node --check`.**

### Task 3: preload + tipos

**Files:**
- Modify: `app/electron/preload.js`, `app/src/global.d.ts`

- [ ] **Step 1: preload: `onFriendsChanged(cb)`** com cleanup.
- [ ] **Step 2: global.d.ts: tipar `onFriendsChanged` e `friendsList(opts?)**.
- [ ] **Step 3: `node --check` preload.**

### Task 4: FriendsContext — evento + forcar

**Files:**
- Modify: `app/src/components/account/FriendsContext.tsx`

- [ ] **Step 1: `refresh(forcar?)` repassa flag.**
- [ ] **Step 2: useEffect escuta `onFriendsChanged` → setData.**
- [ ] **Step 3: `onFriendRequest` → `refresh(true)`; login → `refresh(false)`.**
- [ ] **Step 4: tsc.**

### Task 5: ProfilePage (console) — useFriends()

**Files:**
- Modify: `app/src/components/ps5-launcher/ProfilePage.tsx`

- [ ] **Step 1: Remover `friendsList()` do useEffect e o state `friends`; usar `useFriends()`** → `data?.friends ?? []`.
- [ ] **Step 2: tsc.**

### Task 6: Verificação final + commit + reinício

- [ ] **Step 1: `npm test`, `tsc`, `npm run build`.**
- [ ] **Step 2: Verificação ad-hoc** `/tmp/hermes-verify-friends-cache.js`: cache criado com ts/data (600); 2ª chamada deCache; ações removem cache; TTL >60s ignorado; evento friends:changed; testes verdes.
- [ ] **Step 3: Commit** (mensagem descritiva) + reiniciar o app.
