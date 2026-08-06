# Cache de Amigos + Queries Paralelas — Design

**Data:** 2026-08-06
**Projeto:** Arcadia (Electron 33 + React + Supabase free)
**Status:** Design aprovado pelo usuário — aguardando implementação

## Problema

A aba de amigos (FriendsView desktop) e a seção de amigos do perfil (ProfilePage console)
demoram ~1.5-2s para aparecer. Medição real: o Supabase free responde ~0.5-0.8s por
query e o `friends.list()` faz 2 queries SEQUENCIAIS (~1.4s) + avatares. Não é bug de
código — é latência do servidor.

## Solução (aprovada)

Cache local da lista de amigos (abre instantâneo + atualiza em background) +
paralelizar as 2 queries. Mesmo padrão do `library:get` (cache + não-bloqueante).

## Arquitetura

### 1. Main — `app/electron/supabase/friends.js`

- **`list({ forcar })`**:
  - Cache em `contas/<user>/friends_cache.json` = `{ ts, data }` — escrita atômica
    (tmp + rename) + `chmod 600`, no escopo da conta (caminho via `caminhoConta`,
    padrão do sync_state).
  - TTL de 60s. Se cache válido e `!forcar`: retorna `{ ok, data: cache }` IMEDIATO
    e dispara `buscarFresco()` em background; ao terminar, avisa via callback
    `onAtualizado(data)` (registrado pelo ipc.js → broadcast `friends:changed`).
  - Sem cache (ou `forcar`): `buscarFresco()` síncrono e retorna.
- **`buscarFresco(me, cacheFile)`**: `Promise.all` das 2 queries
  (friendships + profiles `.in()`), monta `{ friends, incoming, outgoing }`,
  grava o cache e retorna. (~1.4s → ~0.6s)
- **Ações invalidam o cache**: `send`, `accept`, `cancel` (e demais mutações)
  fazem `fs.rm(cacheFile, { force: true })` → o próximo `list()` é fresco.
- `requireUserId` inalterado; guest (sem conta) não tem cache (lista vazia).

### 2. IPC — `app/electron/supabase/ipc.js`

- `friends:list` repassa `{ forcar }` do payload.
- `friends.onAtualizado((data) => broadcast("friends:changed", data))`.

### 3. Preload + tipos

- `preload.js`: `onFriendsChanged(cb)` → `ipcRenderer.on("friends:changed", (_e, data) => cb(data))`, retorna cleanup.
- `global.d.ts`: `onFriendsChanged: (cb: (data: FriendsListData) => void) => () => void` e
  `friendsList: (opts?: { forcar?: boolean }) => Promise<{ ok: boolean; data?: FriendsListData; error?: string }>`.

### 4. Renderer

- **`FriendsContext.tsx`**:
  - `refresh(forcar?)` repassa a flag ao IPC.
  - Login (`status === "logado"`): `refresh(false)` — pinta cache (se houver) + background.
  - Novo `useEffect`: escuta `onFriendsChanged` → `setData(dados)` direto (sem re-fetch).
  - `onFriendRequest` (realtime): `refresh(true)` — badge de pedidos sem atraso.
- **`ProfilePage.tsx`** (console): remove o `friendsList()` do `useEffect` (linhas 44-46)
  e o state `friends`; usa `useFriends()` — `const amigos = data?.friends ?? []`
  (o FriendsProvider já cobre o console desde o fix do badge DONO — main.tsx).
  `profileStats` permanece como está.

## Erros

- Falha de rede no background: `.catch(() => {})` — o cache continua na tela; o
  próximo evento/abertura refaz.
- Cache corrompido (JSON inválido): `try/catch` → tratado como ausente (busca síncrona).
- Falha de escrita do cache: `try/catch` silencioso — cache é otimização, nunca requisito.

## Testes / Verificação

- `npm test` (34/34), `tsc --noEmit`, `npm run build`.
- Verificação ad-hoc `/tmp/hermes-verify-friends-cache.js`:
  1. `list()` 1ª chamada cria o cache com `ts` e `data` (600).
  2. 2ª chamada (cache < 60s) retorna `deCache` e dispara background (evento chega).
  3. `accept`/`cancel` removem o cache.
  4. TTL: cache com `ts` antigo (> 60s) é ignorado.
- Teste real: abrir perfil/aba amigos — pintura instantânea + atualização ~0.6s depois.

## Fora de escopo

- Cache de avatares (imagens) — fica para depois se o usuário pedir.
- Mudanças no FriendsView desktop (não precisa — usa o contexto).
