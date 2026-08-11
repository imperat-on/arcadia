# Sincronizacao por-conta (biblioteca, background, sources) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recomendado) ou superpowers:executing-plans para implementar este plano task-by-task. Steps usam checkbox (`- [ ]`).

**Goal:** Fechar o vazamento de biblioteca entre contas e sincronizar por conta: jogos possuidos, background do perfil (imagem+video) e sources publicas (JSON Hydra). Sources com API key ficam 100% locais. Guest mantem comportamento atual (ve tudo).

**Architecture:** Biblioteca ganha `owned_games.json` (array de ids) escopado por `caminhoConta()`. `readLibrary()` filtra o `library.json` global pela posse quando logado. Background espelha o padrao `setAvatar` (upload pro bucket `backgrounds` imagem+video, grava `background_url` no profile). Sources viram por conta no registro (`sources.json` em `contas/<user>/`) e sincronizam so `{id,url,name}` via RPCs `push_sources`/`pull_sources` (molde `push_library`/`pull_library`). Caches `sources/<id>.json` continuam compartilhados na raiz.

**Tech Stack:** Node >= 22, `node:sqlite` (server), Express, shim `client.js` (fetch), Electron main process, React renderer, `node:test` nos dois lados.

## Global Constraints

1. Sources com API key (debrid/Hubcap) NUNCA sincronizam. Nao ha campo no registro nem schema pra elas.
2. Guest ve tudo (zero regressao no comportamento atual).
3. Sync de sources com servidor = `sources:reconcile` (nao `sources:sync`, que ja significa rebaixar JSONs).
4. Nada sensivel na nuvem (etag/lastMod/count/keys ficam locais).
5. DB do servidor: SQLite via `node:sqlite`.
6. `realName` fica local. Background passa a sincronizar.
7. `owned_games.json` ausente (`null`) = "possui tudo" (migracao).

---

## Fase 1, Servidor

### Task 1: Coluna `background_url` em profiles + whitelist PATCH

**Files:**
- Modify: `server/src/db.js:28-45` (no `CREATE TABLE IF NOT EXISTS profiles`, adicionar `background_url TEXT` apos `avatar_url`).
- Modify: `server/src/rest-routes.js` (`COLUNAS_PROFILES` 12-14 e whitelist PATCH 206 ganham `"background_url"`).
- Test: `server/test/background.test.js` (novo, padrao storage.test.js)

**Interfaces:**
- Consumes: `registerAuthRoutes` (signUp), `registerRestRoutes`.
- Produces: `profiles.background_url` persistido.

- [ ] **Step 1: Teste falha.** signUp + `PATCH /rest/v1/profiles` com `{background_url}` da 200 e select devolve. PATCH `{password_hash}` da 400 `sem_campos`.
- [ ] **Step 2: Implementar.** Coluna + whitelists.
- [ ] **Step 3: Teste passa.** `node --test server/test/background.test.js`.
- [ ] **Step 4: Commit.** `feat(server): coluna background_url no perfil`.

### Task 2: Bucket `backgrounds` no storage (imagem + video, 25MB)

**Files:**
- Modify: `server/src/storage-routes.js` (generalizar por bucket):
  - `avatars`: 5MB, `/^[0-9]+\.(png|jpe?g|webp|gif)$/i`, so imagem, serve `image/*`.
  - `backgrounds`: 25MB, `/^[0-9]+\.(png|jpe?g|webp|gif|webm|mp4|m4v|mov)$/i`, MAGIC_BG = MAGIC + EBML `[0x1a,0x45,0xdf,0xa3]` (webm) + MP4 `[0x00,0x00,0x00]` com `ftyp` nos bytes 4-7 (mp4/m4v/mov), serve `video/webm` ou `video/mp4`.
  - Rotas `/storage/v1/object/backgrounds/:uid/:file`, `/public/backgrounds/...`, DELETE generico por bucket.
- Test: `server/test/background.test.js` (estender): upload webm valido (EBML) da Key e GET da 200. Upload mp4 (`00 00 00 18 66 74 79 70` + `mp42`) da Key e GET da 200. PNG em `backgrounds` passa. Texto da 400 `background_nao_midia`. > 25MB da 400. Owner errado da 403. GET de `avatars` continua servindo (regressao).

**Interfaces:**
- Consumes: `client.js` StorageClient (`upload`/`remove` genericos).
- Produces: `POST/GET/DELETE /storage/v1/object/(public/)?backgrounds/...`.

- [ ] **Step 1: Teste falha.** Upload webm/mp4 falha no bucket atual.
- [ ] **Step 2: Implementar.** Generalizar storage-routes com MAGIC_BG + tetos.
- [ ] **Step 3: Teste passa.**
- [ ] **Step 4: Commit.** `feat(server): bucket backgrounds com imagem+video`.

### Task 3: Tabela `user_sources` + RPCs `push_sources`/`pull_sources`

**Files:**
- Modify: `server/src/db.js` (`CREATE TABLE IF NOT EXISTS user_sources (user_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE, source_id TEXT NOT NULL, url TEXT NOT NULL, name TEXT NOT NULL DEFAULT '', added_at TEXT NOT NULL DEFAULT (datetime('now')), removed_at TEXT, PRIMARY KEY (user_id, source_id))`).
- Modify: `server/src/sync-routes.js` (`rpcPushSources(uid, p_sources)` e `rpcPullSources(uid)`):
  - push: `removed` vira `UPDATE user_sources SET removed_at = datetime('now') WHERE user_id=? AND source_id=?`. Senao upsert com `removed_at = NULL`. Valida `source_id` = `/^[0-9a-f]{12}$/` e `url` = `/^https?:\/\//`.
  - pull: `SELECT source_id, url, name FROM user_sources WHERE user_id = ? AND removed_at IS NULL ORDER BY added_at`.
  - Rotas `POST /rest/v1/rpc/push_sources` e `pull_sources` via `authed()`.
- Test: `server/test/sources-sync.test.js` (novo, padrao friends-sync.test.js): push 2 fontes da pull com 2, sem removed_at/etag/count. `source_id` invalido ignorado. `removed:true` faz sumir. Re-push faz voltar. bob nao ve as de alice. url nao-http ignorada.

**Interfaces:**
- Consumes: `db`, `jwt`.
- Produces: rotas `POST /rest/v1/rpc/push_sources` e `pull_sources`.

- [ ] **Step 1: Teste falha.**
- [ ] **Step 2: Implementar.** Tabela + RPCs.
- [ ] **Step 3: Teste passa.**
- [ ] **Step 4: Commit.** `feat(server): user_sources + RPCs push/pull`.

---

## Fase 2, App (biblioteca)

### Task 4: `owned_games.json` no escopo da conta

**Files:**
- Modify: `app/electron/supabase/conta.js:26-35` (adicionar `"owned_games.json"` ao `ARQS_CONTA`).
- Test: `app/test/conta.test.js` (novo): `definirConta("u1")` + `migrarConta` faz raiz `owned_games.json` herdado. `caminhoArquivoConta("owned_games.json")` cai em `contas/u1/`.

- [ ] **Step 1: Teste falha.**
- [ ] **Step 2: Implementar.**
- [ ] **Step 3: Teste passa.**
- [ ] **Step 4: Commit.** `feat(app): owned_games.json escopado por conta`.

### Task 5: Filtro por posse em `readLibrary` + cache key

**Files:**
- Modify: `app/electron/main.js`:
  - `_libMtimeKey()` (1276-1284): adicionar `caminhoConta(OWNED_GAMES)`. Novo `OWNED_GAMES = "owned_games.json"` junto de `LIB` (61).
  - `readLibrary()` (1333-1338): `rawOwned = readJsonFile(caminhoConta(OWNED_GAMES), null)`. `owned = rawOwned === null ? null : new Set(rawOwned)`. `games = conta() && owned ? globais.filter(g => owned.has(g.id)) : globais`. Se `conta()` e `rawOwned === null`: materializar posse (gravar ids do library.json, best-effort).
- Test: `app/test/owned.test.js` (novo): `library.json` com 3 jogos + conta com owned 1 id da readLibrary com 1. Guest da 3. Sem owned + conta da 3 E arquivo materializado. Touch no owned reflete (cache key).

- [ ] **Step 1: Teste falha.**
- [ ] **Step 2: Implementar.** Filtro + cache key + materializacao.
- [ ] **Step 3: Teste passa.**
- [ ] **Step 4: Commit.** `fix(app): readLibrary filtra por owned_games.json`.

### Task 6: Registrar/remover posse nos pontos existentes

**Files:**
- Modify: `app/electron/main.js` (helpers perto de `adicionarStubPendente` 1425): `ownedSet()`, `ownedAdd(id)` (guest = no-op), `ownedRemove(id)` (writeJson atomico).
- Call sites (`ownedAdd(...)`): `game:import` (2169), `customgame:add` (2196), `store:addToSteam` (2623), `store:addToLibrary` (2648) com `ownedAdd("steam:"+appid)` na linha do `adicionarStubPendente`. `dm.onDone` (2517) apos `runIndexer()` com `ownedAdd(String(item.appid))` (verificar formato no downloadmanager.js).
- `store:removeFromLibrary` (2670): `ownedRemove(id)` SEMPRE, mantendo `hidden` para Steam-ownered.
- `game:uninstall` ramo custom (2294): `ownedRemove(id)`.
- Test: `app/test/owned.test.js` (estender): `ownedAdd` com conta grava. Guest nao cria arquivo. `ownedRemove` tira.

- [ ] **Step 1: Teste falha.**
- [ ] **Step 2: Implementar.** Helpers + call sites.
- [ ] **Step 3: Teste passa.**
- [ ] **Step 4: Commit.** `feat(app): posse registrada nos pontos de adicao/remocao`.

### Task 7: Push/pull de posse via biblioteca.js

**Files:**
- Modify: `app/electron/supabase/biblioteca.js`:
  - `push()`: apos diff de custom, ids possuidos sem watermark sobem como `p_lib.push({appid: id, title: id, platform: "windows"})`.
  - `pull()`: rows do servidor, se `owned !== null` e `!owned.has(row.appid)`, faz `owned.add` + persistir.
  - Persistir `owned_games.json` no fim (writeJson).
- Test: `app/test/owned.test.js` (estender): push com owned 2 ids faz `push_library` receber. Pull com row faz owned ganhar id e custom_games ganhar stub (regressao do comportamento atual).

- [ ] **Step 1: Teste falha.**
- [ ] **Step 2: Implementar.**
- [ ] **Step 3: Teste passa.**
- [ ] **Step 4: Commit.** `feat(app): posse no push/pull da biblioteca`.

---

## Fase 3, App (background)

### Task 8: `setBackground` no auth.js (upload + URL no profile)

**Files:**
- Modify: `app/electron/supabase/auth.js`:
  - `CAMPOS_PERFIL` (246): adicionar `"background_url"`.
  - `MAGIC_MIDIA` (MAGIC + EBML + MP4), `BACKGROUND_MAX = 25MB`, `BACKGROUND_EXT` (png/jpg/jpeg/gif/webp/webm/mp4/m4v/mov).
  - `async function setBackground(filePath)`: espelho de `setAvatar` (teto, magicDeMidia, upload `storage.from("backgrounds").upload(me + "/" + Date.now() + ext, buf, {contentType})`, grava `background_url`, remove antiga via parse `/public/backgrounds/`). Sem re-encode (video).
  - Exportar `setBackground`.
- Test: `app/test/auth.test.js` (estender, padrao storage.test.js): PNG grava `background_url`. webm da URL `.webm`. Texto da erro. > 25MB da erro. `updateProfile({background_url})` faz `myProfile()` devolver.

- [ ] **Step 1: Teste falha.**
- [ ] **Step 2: Implementar.**
- [ ] **Step 3: Teste passa.**
- [ ] **Step 4: Commit.** `feat(app): setBackground com upload de imagem/video`.

### Task 9: Renderer, `background_url` no PerfilOnline + EditProfile

**Files:**
- Modify: `app/src/components/account/AccountContext.tsx` (`PerfilOnline` +`background_url`, `CAMPOS_ESPELHO` +`"background_url"`, `updatePerfil` alvo inclui `background_url`).
- Modify: `app/src/components/ps5-launcher/EditProfile.tsx` (`pick()` ramo background: se logado, `subirBackground(path)` espelho `subirAvatar` com `conta.setBackground` e `onChange` + `setConfig`).
- Modify: `app/src/global.d.ts`, `app/electron/preload.js` (perto de 209), `app/electron/supabase/ipc.js` (perto de "account:setAvatar") com `account:setBackground` IPC.
- Test: sem infra de renderer no repo. Verificacao: `npm run build` compila + smoke manual. **ponytail:** testes de renderer quando houver vitest+RTL.

- [ ] **Step 1: Implementar.** IPC + preload + tipos.
- [ ] **Step 2: Implementar.** AccountContext + EditProfile.
- [ ] **Step 3: Build passa.** `npm run build`.
- [ ] **Step 4: Commit.** `feat(app): background sincronizado na edicao de perfil`.

---

## Fase 4, App (sources)

### Task 10: `sources.json` por conta

**Files:**
- Modify: `app/electron/supabase/conta.js` (adicionar `"sources.json"` ao `ARQS_CONTA`).
- Modify: `app/electron/sources.js` (`REGISTRY = () => caminhoArquivoConta("sources.json")`. `readRegistry`/`writeRegistry` usam `REGISTRY()`. `SRC_DIR` caches continua na raiz).
- Test: `app/test/sources.test.js` (novo): `ARCADIA_DATA_DIR` temp. `definirConta("u1")`. `addSource` (stub fetch) grava registro em `contas/u1/sources.json`. Guest da vazio na raiz. Cache em `sources/<id>.json` na raiz. `srcId(url)` deterministico.

- [ ] **Step 1: Teste falha.**
- [ ] **Step 2: Implementar.**
- [ ] **Step 3: Teste passa.**
- [ ] **Step 4: Commit.** `feat(app): sources.json por conta`.

### Task 11: `sources:reconcile` (sync com servidor)

**Files:**
- Create: `app/electron/supabase/sources.js` (novo, molde biblioteca.js):
  - `push()`: registro por conta. Watermark `st.srcPush` em `sync_state.json` (por conta). Diff. `rpc("push_sources", {p_sources})`. Watermark so no sucesso.
  - `pull()`: `rpc("pull_sources")` insere direto no registro (sem baixar) com `{id, url, name, etag:"", lastMod:"", addedAt: Date.now()}` + writeRegistry.
  - `reconcile()`: `push()` + `pull()`, chamado no SIGNED_IN.
  - `agendarPush()`: debounce 2s.
- Modify: `app/electron/sources.js` (`addSource`/`removeSource` chamam `agendarPush()` no sucesso, try/catch).
- Modify: `app/electron/supabase/ipc.js` (SIGNED_IN 58: `sourcesSync.reconcile().catch(() => {})`).
- Test: `app/test/sources.test.js` (estender, servidor real): push fonte nova faz `push_sources` receber. Segunda chamada nao re-envia (diff). Pull faz registro ganhar (sem download). `removeSource` apos debounce envia `removed:true`. Guest: reconcile no-op.

- [ ] **Step 1: Teste falha.**
- [ ] **Step 2: Implementar.** supabase/sources.js + gatilhos.
- [ ] **Step 3: Teste passa.**
- [ ] **Step 4: Commit.** `feat(app): sources publicas sincronizam por conta (reconcile no login)`.

---

## Fase 5, E2E

### Task 12: E2E, biblioteca nao vaza entre contas

**Files:**
- Test: `server/test/e2e-conta.test.js` (novo, Express completo + shim): alice e bob. alice `push_library` com `{appid:"steam:10"}`. owned alice com 1 id. `pull_library` bob da vazio. `readLibrary()` alice da 1 jogo. bob (owned vazio) da 0. guest da 5. `custom_games.json` de alice nao aparece pra bob.

- [ ] **Step 1: Teste falha** (comportamento antigo).
- [ ] **Step 2: Passa com Fases 1-4.**
- [ ] **Step 3: Commit.** `test(e2e): isolamento por conta (biblioteca)`.

### Task 13: E2E, background sincroniza entre maquinas

**Files:**
- Test: `server/test/e2e-conta.test.js` (estender): alice `setBackground` (PNG real) da URL publica. Fetch da URL da 200, bytes PNG. Nova sessao (maquina 2) faz `myProfile()` devolver `background_url`. bob da nulo.

- [ ] **Step 1-3: Idem Task 12.** Commit: `test(e2e): background sincroniza`.

### Task 14: E2E, sources publicas sincronizam, chaves nao

**Files:**
- Test: `server/test/e2e-conta.test.js` (estender): alice `push_sources` 2 fontes. bob da vazio, alice 2. Entrada com etag/count extra ignorada pelo servidor. `source_id` invalido ignorado. `removed:true` faz sumir. Re-add faz voltar.

- [ ] **Step 1-3: Idem.** Commit: `test(e2e): sources publicas sincronizam`.

---

## Ordem e dependencias

1. T1, T2, T3 (servidor, paralelizaveis).
2. T4, T5, T6, T7 (biblioteca app).
3. T8, T9 (background app, depende de T1+T2).
4. T10, T11 (sources app, depende de T3+T10).
5. T12, T13, T14 (E2E).

## Riscos a confirmar durante implementacao

- **DM onDone id**: formato de `item.appid` no downloadmanager.js (prefixo steam/epic), usar o mesmo do resto.
- **`hidden` vs posse em removeFromLibrary**: manter `hidden` para Steam-ownered. A posse remove da conta.
- **Migracao owned_games**: materializacao lazy na primeira `readLibrary` logada (T5) e idempotente.
- **guest + push**: biblioteca/sources push ja retornam cedo sem usuario. Posse nao sincroniza pra guest (correto).

## Critical Files

- `app/electron/main.js`
- `app/electron/supabase/biblioteca.js`
- `app/electron/sources.js`
- `server/src/sync-routes.js`
- `server/src/storage-routes.js`
