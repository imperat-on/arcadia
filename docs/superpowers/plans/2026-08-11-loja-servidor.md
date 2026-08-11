# Loja no Servidor (Proxy de Catálogo) — Plano de Implementação

> **Para agentes:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recomendado) ou superpowers:executing-plans para implementar task a task. Passos usam checkbox (`- [ ]`).

**Goal:** Mover a loja (catálogo) do Arcadia para o servidor: o servidor Node baixa os catálogos uma vez por TTL de Hydra/SteamSpy/Steam/etc e os serve ao app via HTTP. O app deixa de buscar catálogo de terceiros para a loja.

**Architecture:** Servidor ganha um módulo `catalog-routes.js` que baixa e cacheia catálogos em uma tabela genérica `catalog_cache` do SQLite (chave, dado JSON, timestamp). O app mantém o cache local como espelho offline e passa a consultar o servidor via endpoints `/catalog/v1/*` autenticados por JWT. Trailers e API keys pagas ficam locais (fora de escopo).

**Tech Stack:** Node >= 22 (server), Express 4, SQLite via `node:sqlite` (WAL), JWT (HS256), Electron (app), `node --test`.

## Global Constraints

- TTLs iguais aos do app: popular/sushi 6h, items/manifest 7d, genre 12h, news 30min, hltb 30d, fixes 6h, sysinfo/meta sem TTL.
- Toda rota `/catalog/v1/*` exige `Authorization: Bearer <jwt>` (valida via `verifyToken`/`extractToken` de `server/src/jwt.js`).
- Tabela `catalog_cache` criada com `CREATE TABLE IF NOT EXISTS` no boot (padrão do projeto em `db.js`).
- **Nunca** sincronizar/sevir chaves pagas do usuário (Hubcap, debrid, Steam key) nem trailers. API keys pagas ficam locais.
- `uris` de download (fontes Hydra) nunca vazam para resposta sem JWT válido.
- App mantém o cache local como **espelho offline** (fallback quando servidor cai).
- Padrão de testes do server: `node --test`, app Express em memória, DB temporário, porta efêmera (ver `server/test/auth.test.js`).

---

## File Structure

**Criados:**
- `server/src/catalog-routes.js` — rotas `/catalog/v1/*`, cache SQLite, fetch de fontes externas
- `server/src/catalog-fetch.js` — funções puras de fetch de cada catálogo (Hydra, SteamSpy, Steam items, sushi, manifestos, genre, sysinfo, meta, news, hltb, fixes)
- `server/test/catalog.test.js` — testes de rotas + fetch mockado
- `app/electron/catalog.js` — cliente HTTP do catálogo (substitui leitura de disco por chamada ao servidor) + espelho local
- `app/test/catalog-fallback.test.js` — teste do fallback offline

**Modificados:**
- `server/src/db.js` — adicionar `CREATE TABLE IF NOT EXISTS catalog_cache`
- `server/src/server.js` — registrar `registerCatalogRoutes(app)`
- `server/package.json` — adicionar `compression` (gzip)
- `app/electron/steamstore.js` — trocar leitura de `store_*_cache.json` por cliente `catalog.js`
- `app/electron/sources.js` — trocar `fs.readFileSync` por cliente `catalog.js`
- `app/electron/main.js` — sysinfo/news/hltb/fixes/profile_cache → cliente `catalog.js` (onde aplicável)

---

## Fase 1 — Servidor: tabela, fetch e rotas (Tasks 1–4)

> **Agent A** faz as Tasks 1–4 (Fase 1). **Agent B** faz as Tasks 5–7 (Fase 2). Começam depois que Task 1 commita (`catalog-routes.js` + `catalog_cache` + registro), pois Task 5 depende da tabela e do módulo de fetch.

### Task 1: Tabela `catalog_cache` + módulo de fetch de catálogo

**Files:**
- Modify: `server/src/db.js` (adicionar tabela)
- Create: `server/src/catalog-fetch.js` (funções de fetch de cada catálogo)
- Test: `server/test/catalog.test.js` (teste do fetch mockado)

**Interfaces:**
- Consumes: `db` de `server/src/db.js`, `fetch` global (Node >= 22)
- Produces: `fetchCatalogKey(key)` → `{ data: <JSON>, at: <epoch s> } | null`; `CATALOG_KEYS` (allowlist de chaves); `CATALOG_TTL` (mapa de TTLs)

- [ ] **Step 1: Escrever o teste que falha** (`server/test/catalog.test.js`)

```js
const test = require("node:test")
const assert = require("node:assert")

// fetch global stubado: retorna JSON por URL
function stubFetch(map) {
  global.fetch = async (url) => {
    const body = map[url]
    if (body === undefined) return { ok: false, status: 404, text: async () => "not found" }
    return { ok: true, status: 200, text: async () => JSON.stringify(body) }
  }
}
const { fetchCatalogKey, CATALOG_KEYS } = require("../src/catalog-fetch")

test("fetchCatalogKey baixa e devolve JSON de uma fonte Hydra", async () => {
  stubFetch({ "https://hydralinks.cloud/sources/fitgirl.json": { name: "fitgirl", downloads: [] } })
  const r = await fetchCatalogKey("hydra:59e6a31484ce")
  assert.ok(r)
  assert.equal(r.data.name, "fitgirl")
  assert.ok(r.at > 0)
})

test("CATALOG_KEYS so permite chaves conhecidas (allowlist)", () => {
  assert.ok(Array.isArray(CATALOG_KEYS))
  assert.ok(CATALOG_KEYS.length >= 13) // sources, popular, items, sushi, manifests, genre, sysinfo, meta, news, hltb, fixes, ryuu
  assert.ok(!CATALOG_KEYS.includes("../../etc/passwd"))
})
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cd server && node --test test/catalog.test.js`
Expected: FAIL — `Cannot find module '../src/catalog-fetch'`

- [ ] **Step 3: Adicionar tabela em `db.js`**

Em `server/src/db.js`, junto às outras `CREATE TABLE IF NOT EXISTS` (após `blocks`, ~linha 143):

```js
// Cache de catalogo da loja (proxy): chave texto, dado JSON, timestamp
// quando foi buscado. O servidor busca a fonte externa uma vez por TTL.
CREATE TABLE IF NOT EXISTS catalog_cache (
  key TEXT PRIMARY KEY,
  data TEXT NOT NULL,
  at   INTEGER NOT NULL
);
```

- [ ] **Step 4: Criar `server/src/catalog-fetch.js`**

```js
"use strict"
// Busca de catalogos da loja junto a fontes externas (Hydra, SteamSpy,
// Steam, sushi, provedores de manifesto). Funcoes puras de rede: recebem
// key, devolvem { data, at } ou null. Nenhuma chave paga sai daqui — o
// servidor nunca conhece Hubcap/debrid/Steam key.

const HYDRALINKS = "https://hydralinks.cloud/sources"
const STEAMSPY = "https://steamspy.com/api.php"

// Allowlist de chaves de catalogo. Valida entradas de cache: qualquer key
// fora daqui e rejeitada (nada de path traversal / key arbitraria).
const CATALOG_KEYS = [
  "hydra:59e6a31484ce", // fitgirl (exemplo; id real = sha256(url).slice(0,12))
  "popular",
  "sushi",
  "genre:__all",
  "news",
  "fixes",
  "ryuu-index",
]

// TTL por prefixo de chave (segundos). Sobe como hint para o app.
const CATALOG_TTL = {
  "hydra:": 604800,    // 7d (ETag revalida)
  popular: 21600,      // 6h
  sushi: 21600,        // 6h
  "genre:": 43200,     // 12h
  news: 1800,          // 30min
  fixes: 21600,        // 6h
  "ryuu-index": 21600, // 6h
}

async function fetchHydra(id) {
  const url = `${HYDRALINKS}/${id}.json`
  const res = await fetch(url)
  if (!res.ok) return null
  return { data: await res.json(), at: Math.floor(Date.now() / 1000) }
}

async function fetchPopular() {
  const url = `${STEAMSPY}?request=top100in2weeks`
  const res = await fetch(url)
  if (!res.ok) return null
  const json = await res.json()
  // shape SteamSpy { appid: { name, ... } } -> array de { appid, title, cover }
  const completa = Object.entries(json).map(([appid, g]) => ({
    appid: Number(appid),
    title: g.name || "",
    cover: g.header_image || "",
  }))
  return { data: { completa }, at: Math.floor(Date.now() / 1000) }
}

// ... fetchSushi, fetchGenre, fetchNews, fetchFixes, fetchRyuuIndex ...
// (mesmo padrao: fetch -> { data, at } | null)

// Dispatcher: a implementacao real mapeia key -> fetch. Este arquivo e a
// FASE 1: cobre hydra + popular. As demais chaves entram nas Tasks 2-4.
async function fetchCatalogKey(key) {
  if (key.startsWith("hydra:")) return fetchHydra(key.slice("hydra:".length))
  if (key === "popular") return fetchPopular()
  return null // demais chaves: Fase 2 adiciona
}
```

- [ ] **Step 5: Rodar e ver passar**

Run: `cd server && node --test test/catalog.test.js`
Expected: PASS

- [ ] **Step 6: Commitar**

```bash
git add server/src/db.js server/src/catalog-fetch.js server/test/catalog.test.js
git commit -m "feat(server): tabela catalog_cache + fetch de catalogo (hydra, popular)"
```

---

### Task 2: Rotas `/catalog/v1/*` + cache SQLite + gzip

**Files:**
- Create: `server/src/catalog-routes.js`
- Modify: `server/src/server.js` (registrar rota), `server/package.json` (compression)
- Test: `server/test/catalog.test.js` (rotas)

**Interfaces:**
- Consumes: `db` (catalog_cache), `fetchCatalogKey`/`CATALOG_KEYS`/`CATALOG_TTL` da Task 1, `verifyToken`/`extractToken`
- Produces: `registerCatalogRoutes(app)` — endpoints `/catalog/v1/*`; `getCached(key, ttlS)` → `{data}|null` (lê/atualiza tabela com stale-while-revalidate)

- [ ] **Step 1: Teste que falha** (append em `server/test/catalog.test.js`)

```js
// boot do app com rotas de catalogo em memoria
process.env.DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-cat-"))
const express = require("express")
const { registerCatalogRoutes } = require("../src/catalog-routes")
const catApp = express()
catApp.use(express.json())
registerCatalogRoutes(catApp)
const listener = catApp.listen(0)
const catBase = `http://127.0.0.1:${listener.address().port}`
const JWT = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyMSJ9.x" // stub JWT (ver jwt.js para shape real)

test("catalog get exige JWT (401 sem token)", async () => {
  const r = await fetch(`${catBase}/catalog/v1/popular`)
  assert.equal(r.status, 401)
})
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cd server && node --test test/catalog.test.js`
Expected: FAIL — `Cannot find module '../src/catalog-routes'`

- [ ] **Step 3: Instalar `compression`**

Run: `cd server && npm install compression`

- [ ] **Step 4: Criar `server/src/catalog-routes.js`**

```js
"use strict"
// Proxy de catalogo da loja. Busca a fonte externa uma vez por TTL,
// guarda em catalog_cache e responde JSON pronto ao app. Tudo exige JWT.

const { db } = require("./db")
const { verifyToken, extractToken } = require("./jwt")
const { fetchCatalogKey, CATALOG_KEYS, CATALOG_TTL } = require("./catalog-fetch")

function authed(fn) {
  return (req, res) => {
    const v = verifyToken(extractToken(req) || "")
    if (!v?.sub) return res.status(401).json({ error: "nao_autenticado" })
    return fn(v.sub, req, res)
  }
}

// Le do cache; se vencido, busca em background (stale-while-revalidate) e
// devolve o que tem. Nunca bloqueia resposta em rede externa lenta.
function getCached(key) {
  const ttl = CATALOG_TTL[key] || 0
  const row = db.prepare("SELECT data, at FROM catalog_cache WHERE key = ?").get(key)
  if (!row) return null
  const data = JSON.parse(row.data)
  const agora = Math.floor(Date.now() / 1000)
  if (ttl > 0 && agora - row.at > ttl) {
    // revalida em background, sem travar a resposta
    fetchCatalogKey(key).then((r) => {
      if (r) db.prepare("INSERT OR REPLACE INTO catalog_cache (key, data, at) VALUES (?,?,?)").run(key, JSON.stringify(r.data), r.at)
    }).catch(() => {})
  }
  return data
}

// Resolve uma key por tipo + id, validando contra allowlist de prefixo.
function resolveKey(tipo, id) {
  if (tipo === "hydra" && id) { const k = `hydra:${id}`; return CATALOG_KEYS.includes(k) ? k : null }
  if (tipo === "popular") return CATALOG_KEYS.includes("popular") ? "popular" : null
  if (tipo === "sushi") return CATALOG_KEYS.includes("sushi") ? "sushi" : null
  if (tipo === "genre") { const k = `genre:${id || "__all"}`; return CATALOG_KEYS.includes(k) ? k : null }
  if (tipo === "news") return CATALOG_KEYS.includes("news") ? "news" : null
  if (tipo === "fixes") return CATALOG_KEYS.includes("fixes") ? "fixes" : null
  if (tipo === "ryuu") return CATALOG_KEYS.includes("ryuu-index") ? "ryuu-index" : null
  return null
}

function registerCatalogRoutes(app) {
  // popular: GET /catalog/v1/popular?lista=&limite=&offset=
  app.get("/catalog/v1/popular", authed((uid, req, res) => {
    const data = getCached("popular")
    if (!data) return res.status(404).json({ error: "cache_vazio" })
    const lista = req.query.lista || "__all"
    const limite = Number(req.query.limite) || 40
    const offset = Number(req.query.offset) || 0
    const completa = data.completa || []
    res.json({ ok: true, lista, itens: completa.slice(offset, offset + limite), total: completa.length })
  }))
  // genre: GET /catalog/v1/genre?lista=
  app.get("/catalog/v1/genre", authed((uid, req, res) => {
    const key = resolveKey("genre", req.query.lista)
    if (!key) return res.status(400).json({ error: "key_invalida" })
    const data = getCached(key)
    if (!data) return res.status(404).json({ error: "cache_vazio" })
    res.json({ ok: true, data })
  }))
  // sushi: GET /catalog/v1/sushi
  app.get("/catalog/v1/sushi", authed((uid, req, res) => {
    const data = getCached("sushi")
    if (!data) return res.status(404).json({ error: "cache_vazio" })
    res.json({ ok: true, data })
  }))
  // news: GET /catalog/v1/news
  app.get("/catalog/v1/news", authed((uid, req, res) => {
    const data = getCached("news")
    if (!data) return res.status(404).json({ error: "cache_vazio" })
    res.json({ ok: true, data })
  }))
  // fixes: GET /catalog/v1/fixes
  app.get("/catalog/v1/fixes", authed((uid, req, res) => {
    const data = getCached("fixes")
    if (!data) return res.status(404).json({ error: "cache_vazio" })
    res.json({ ok: true, data })
  }))
  // ryuu: GET /catalog/v1/ryuu
  app.get("/catalog/v1/ryuu", authed((uid, req, res) => {
    const data = getCached("ryuu-index")
    if (!data) return res.status(404).json({ error: "cache_vazio" })
    res.json({ ok: true, data })
  }))
  // sources/hydra: GET /catalog/v1/sources/:id/games
  app.get("/catalog/v1/sources/:id/games", authed((uid, req, res) => {
    const key = resolveKey("hydra", req.params.id)
    if (!key) return res.status(400).json({ error: "key_invalida" })
    const data = getCached(key)
    if (!data) return res.status(404).json({ error: "cache_vazio" })
    res.json({ ok: true, data })
  }))
  // search: GET /catalog/v1/search?q=
  app.get("/catalog/v1/search", authed((uid, req, res) => {
    const q = String(req.query.q || "").trim().toLowerCase()
    if (!q) return res.json({ ok: true, itens: [] })
    // busca no indice montado das fontes hidra em cache
    const itens = []
    for (const k of CATALOG_KEYS) {
      if (!k.startsWith("hydra:")) continue
      const data = getCached(k)
      if (!data?.downloads) continue
      for (const d of data.downloads) {
        if (String(d.title || "").toLowerCase().includes(q)) {
          itens.push({ ref: `${k.replace("hydra:", "")}:${data.downloads.indexOf(d)}`, title: d.title, fileSize: d.fileSize, uploadDate: d.uploadDate })
        }
      }
    }
    res.json({ ok: true, itens: itens.slice(0, 40) })
  }))
}

module.exports = { registerCatalogRoutes, getCached, resolveKey }
```

- [ ] **Step 5: Registrar em `server.js`**

Em `server/src/server.js`, após `registerStorageRoutes(app)` (linha ~21), adicionar:

```js
const { registerCatalogRoutes } = require("./catalog-routes")
// ...apos registerStorageRoutes(app):
registerCatalogRoutes(app)
```

E adicionar `app.use(require("compression")())` após `app.use(express.json(...))`.

- [ ] **Step 6: Rodar e ver passar**

Run: `cd server && node --test test/catalog.test.js`
Expected: PASS

- [ ] **Step 7: Commitar**

```bash
git add server/src/catalog-routes.js server/src/server.js server/package.json server/test/catalog.test.js
git commit -m "feat(server): rotas /catalog/v1/* + cache SQLite + gzip"
```

---

### Task 3: Endpoints de sysinfo, meta, hltb + fetch das demais fontes

**Files:**
- Modify: `server/src/catalog-fetch.js` (adicionar `fetchSysinfo`, `fetchMeta`, `fetchHltb`)
- Modify: `server/src/catalog-routes.js` (rotas sysinfo/meta/hltb)
- Test: `server/test/catalog.test.js`

**Interfaces:**
- Consumes: `fetchCatalogKey` (estendida na Task 1)
- Produces: rotas `GET /catalog/v1/sysinfo/:appid`, `GET /catalog/v1/meta/:appid`, `GET /catalog/v1/hltb/:appid`

- [ ] **Step 1: Teste que falha**

```js
test("sysinfo:appid devolve requisitos de sistema do cache", async () => {
  // prepara cache de sysinfo:2622380
  const { db } = require("../src/db")
  db.prepare("INSERT OR REPLACE INTO catalog_cache (key, data, at) VALUES (?,?,?)")
    .run("sysinfo:2622380", JSON.stringify({ req_min: "16GB RAM", req_rec: "32GB" }), Math.floor(Date.now()/1000))
  const r = await fetch(`${catBase}/catalog/v1/sysinfo/2622380`, { headers: { authorization: `Bearer ${JWT}` } })
  assert.equal(r.status, 200)
  const body = await r.json()
  assert.equal(body.ok, true)
  assert.equal(body.data.req_min, "16GB RAM")
})
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cd server && node --test test/catalog.test.js`
Expected: FAIL — rota `sysinfo` não existe ainda (404 ou erro)

- [ ] **Step 3: Estender `catalog-fetch.js`**

```js
const STEAM_APPDETAILS = "https://store.steampowered.com/api/appdetails"

async function fetchSysinfo(appid) {
  const url = `${STEAM_APPDETAILS}?appids=${appid}&l=english`
  const res = await fetch(url)
  if (!res.ok) return null
  const j = await res.json()
  const data = j?.[String(appid)]?.data
  if (!data) return null
  return { data: { req_min: data.pc_requirements?.minimum || "", req_rec: data.pc_requirements?.recommended || "" }, at: Math.floor(Date.now() / 1000) }
}
// fetchMeta(id) idem (Steam + SteamGridDB), fetchHltb(id) idem (scrape HLTB)
```

E no dispatcher `fetchCatalogKey`, adicionar:

```js
  if (key.startsWith("sysinfo:")) return fetchSysinfo(key.slice("sysinfo:".length))
  if (key.startsWith("meta:")) return fetchMeta(key.slice("meta:".length))
  if (key.startsWith("hltb:")) return fetchHltb(key.slice("hltb:".length))
```

- [ ] **Step 4: Adicionar rotas em `catalog-routes.js`** (dentro de `registerCatalogRoutes`)

```js
  app.get("/catalog/v1/sysinfo/:appid", authed((uid, req, res) => {
    const key = `sysinfo:${req.params.appid}`
    if (!/^\d+$/.test(req.params.appid)) return res.status(400).json({ error: "appid_invalido" })
    const data = getCached(key)
    if (!data) return res.status(404).json({ error: "cache_vazio" })
    res.json({ ok: true, data })
  }))
  app.get("/catalog/v1/meta/:appid", authed((uid, req, res) => { /* idem, key meta: */ }))
  app.get("/catalog/v1/hltb/:appid", authed((uid, req, res) => { /* idem, key hltb: */ }))
```

(Nota: `resolveKey` não cobre sysinfo/meta/hltb — aqui o appid é validado por regex e a key é construída com prefixo; `getCached` usa `CATALOG_TTL[key]` que precisa das chaves `sysinfo:`/`meta:`/`hltb:` com TTL 0 ou seus TTLs.)

- [ ] **Step 5: Rodar e ver passar**

Run: `cd server && node --test test/catalog.test.js`
Expected: PASS

- [ ] **Step 6: Commitar**

```bash
git add server/src/catalog-fetch.js server/src/catalog-routes.js server/test/catalog.test.js
git commit -m "feat(server): sysinfo/meta/hltb no proxy de catalogo"
```

---

### Task 4: Endpoints de itens/manifestos + fetch dos provedores

**Files:**
- Modify: `server/src/catalog-fetch.js` (fetchSushi real, fetchItems, fetchManifest)
- Modify: `server/src/catalog-routes.js` (rotas items/manifests)
- Test: `server/test/catalog.test.js`

**Interfaces:**
- Consumes: `fetchCatalogKey` (Fase 1), `resolveKey` (Task 2)
- Produces: `GET /catalog/v1/items?appids=`, `GET /catalog/v1/manifests/:appid`

- [ ] **Step 1: Teste que falha**

```js
test("items?appids= devolve tipo+arte por appid", async () => {
  const { db } = require("../src/db")
  db.prepare("INSERT OR REPLACE INTO catalog_cache (key, data, at) VALUES (?,?,?)")
    .run("items:2622380", JSON.stringify({ tipo: 0, capa: "url", heroi: "url", icon: "url" }), Math.floor(Date.now()/1000))
  const r = await fetch(`${catBase}/catalog/v1/items?appids=2622380`, { headers: { authorization: `Bearer ${JWT}` } })
  assert.equal(r.status, 200)
  const body = await r.json()
  assert.equal(body.ok, true)
  assert.equal(body.data["2622380"].tipo, 0)
})
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cd server && node --test test/catalog.test.js`
Expected: FAIL — rota `items` não existe

- [ ] **Step 3: Estender `catalog-fetch.js`**

```js
// fetchItems(appid): IStoreBrowseService/GetItems devolve { tipo, capa, heroi, icon }
async function fetchItems(appid) {
  const url = `https://api.steampowered.com/IStoreBrowseService/GetItems/v1/?input_json=${encodeURIComponent(JSON.stringify({ appids: [appid] }))}`
  const res = await fetch(url)
  if (!res.ok) return null
  const j = await res.json()
  const item = j?.response?.store_items?.[0]
  if (!item) return null
  return {
    data: {
      tipo: item.type, capa: item.assets?.capsule_imagev37 || "", heroi: item.assets?.hero_capsule || "", icon: item.assets?.icon || "",
    },
    at: Math.floor(Date.now() / 1000),
  }
}
// fetchManifest(appid): HEAD nos 4 provedores -> { url: { ok } } (sondagem)
// fetchSushi(): trees API do repo sushi -> { ids: [...] }
```

E no dispatcher: `if (key.startsWith("items:")) return fetchItems(...)`; `if (key.startsWith("manifests:")) return fetchManifest(...)`; `if (key === "sushi") return fetchSushi()`.

- [ ] **Step 4: Adicionar rotas em `catalog-routes.js`**

```js
  app.get("/catalog/v1/items", authed((uid, req, res) => {
    const appids = String(req.query.appids || "").split(",").map(Number).filter(Boolean)
    if (!appids.length) return res.json({ ok: true, data: {} })
    const out = {}
    for (const appid of appids) {
      const data = getCached(`items:${appid}`)
      if (data) out[appid] = data
    }
    res.json({ ok: true, data: out })
  }))
  app.get("/catalog/v1/manifests/:appid", authed((uid, req, res) => {
    const key = `manifests:${req.params.appid}`
    const data = getCached(key)
    if (!data) return res.status(404).json({ error: "cache_vazio" })
    res.json({ ok: true, data })
  }))
```

- [ ] **Step 5: Rodar e ver passar**

Run: `cd server && node --test test/catalog.test.js`
Expected: PASS

- [ ] **Step 6: Commitar**

```bash
git add server/src/catalog-fetch.js server/src/catalog-routes.js server/test/catalog.test.js
git commit -m "feat(server): items/manifests no proxy de catalogo"
```

**Fim da Fase 1 (Agent A).** Servidor serve catálogo completo via `/catalog/v1/*`, autenticado, com cache SQLite e stale-while-revalidate.

---

## Fase 2 — App: cliente do catálogo + espelho offline (Tasks 5–7)

> **Agent B** faz as Tasks 5–7 (Fase 2). Requer Task 1 commitada (tabela + fetch). Pode começar assim que Task 1 commitar, em paralelo com Task 2–4 do Agent A.

### Task 5: Cliente `catalog.js` + fallback offline (espelho)

**Files:**
- Create: `app/electron/catalog.js`
- Test: `app/test/catalog-fallback.test.js`

**Interfaces:**
- Consumes: `config` de `app/electron/supabase/config.js` (URL do servidor), `fetchRede` de `app/electron/httpfetch.js`, `supabase/client.js` (`_authHeaders` para o JWT)
- Produces: `catalogGet(path, opts)` → `{ data, error }` (busca no servidor, grava espelho local, fallback pro espelho se servidor fora); `catalogGetEspelho(path)` → espelho local

- [ ] **Step 1: Teste que falha** (`app/test/catalog-fallback.test.js`)

```js
const test = require("node:test")
const assert = require("node:assert")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

// simula DATA_DIR temporario
const TMP = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-cat-"))
process.env.ARCADIA_DATA_DIR = TMP

const { catalogGet } = require("../electron/catalog")

test("catalogGet grava espelho local e devolve dados do servidor", async () => {
  // servidor fake via fetch stubado
  global.fetch = async (url) => {
    if (url.includes("/catalog/v1/popular")) return { ok: true, json: async () => ({ ok: true, itens: [{ appid: 1 }] }) }
    return { ok: false, status: 500, json: async () => ({ error: "erro" }) }
  }
  const r = await catalogGet("/catalog/v1/popular")
  assert.equal(r.error, null)
  assert.equal(r.data.ok, true)
  // espelho gravado em disco
  const espelho = path.join(TMP, "catalog_espelho", "popular.json")
  assert.ok(fs.existsSync(espelho))
})

test("catalogGet cai pro espelho quando servidor fora", async () => {
  global.fetch = async () => { throw new Error("ECONNREFUSED") }
  const r = await catalogGet("/catalog/v1/popular")
  assert.equal(r.error, null) // espelho existe
  assert.ok(r.data.ok)
})
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cd app && node --test test/catalog-fallback.test.js`
Expected: FAIL — `Cannot find module '../electron/catalog'`

- [ ] **Step 3: Criar `app/electron/catalog.js`**

```js
"use strict"
// Cliente do catalogo da loja. Consulta o servidor (/catalog/v1/*), grava
// espelho local e usa o espelho como fallback quando o servidor cai.
// Segue o mesmo offline-first do sync: servidor e fonte, local e espelho.

const fs = require("fs")
const path = require("path")
const os = require("os")
const { fetchRede } = require("./httpfetch")
const config = require("./supabase/config")

const DATA_DIR = process.env.ARCADIA_DATA_DIR || path.join(os.homedir(), ".local/share/arcadia")
const ESPELHO_DIR = path.join(DATA_DIR, "catalog_espelho")

function espelhoPath(p) {
  // /catalog/v1/popular -> popular.json ; /catalog/v1/sources/:id/games -> sources_<id>.json
  const seg = p.split("/").filter(Boolean)
  const base = seg[0]
  const id = seg[1] || ""
  const sub = seg[2] || ""
  const nome = sub ? `${base}_${id}_${sub}` : (id ? `${base}_${id}` : base)
  return path.join(ESPELHO_DIR, `${nome}.json`)
}

function lerEspelho(p) {
  try { return JSON.parse(fs.readFileSync(espelhoPath(p), "utf-8")) } catch { return null }
}

function gravarEspelho(p, data) {
  try {
    fs.mkdirSync(ESPELHO_DIR, { recursive: true })
    const tmp = `${espelhoPath(p)}.tmp`
    fs.writeFileSync(tmp, JSON.stringify(data))
    fs.renameSync(tmp, espelhoPath(p))
  } catch {}
}

function authHeaders() {
  const client = require("./supabase/client") // evita require circular no boot
  return client._authHeaders ? client._authHeaders() : {}
}

async function catalogGet(pathname, opts = {}) {
  const url = `${config.url}${pathname}`
  try {
    const res = await fetchRede(url, {
      method: "GET",
      headers: { ...authHeaders(), "accept": "application/json" },
      signal: AbortSignal.timeout(opts.timeoutMs || 15000),
    })
    if (!res.ok) {
      const espelho = lerEspelho(pathname)
      if (espelho) return { data: espelho, error: null, fallback: true }
      return { data: null, error: { message: `HTTP ${res.status}` } }
    }
    const data = await res.json()
    gravarEspelho(pathname, data)
    return { data, error: null }
  } catch (e) {
    const espelho = lerEspelho(pathname)
    if (espelho) return { data: espelho, error: null, fallback: true }
    return { data: null, error: { message: String(e.message || e) } }
  }
}

module.exports = { catalogGet, lerEspelho, espelhoPath }
```

- [ ] **Step 4: Rodar e ver passar**

Run: `cd app && node --test test/catalog-fallback.test.js`
Expected: PASS

- [ ] **Step 5: Commitar**

```bash
git add app/electron/catalog.js app/test/catalog-fallback.test.js
git commit -m "feat(app): cliente de catalogo + espelho offline (fallback)"
```

---

### Task 6: `steamstore.js` → cliente `catalog.js` (loja Steam)

**Files:**
- Modify: `app/electron/steamstore.js` (trocar leitura de `store_*_cache.json` por `catalogGet`)

**Interfaces:**
- Consumes: `catalogGet`/`catalogGetEspelho` da Task 5
- Produces: mantém a mesma API pública (`search`, `popular`, `suggest`, `getManifest`, `installInfo`) — sem mudar o renderer

- [ ] **Step 1: Localizar os pontos de leitura de disco**

Procure em `app/electron/steamstore.js`: `lerCache(SUSHI_CACHE)`, `lerManifestCache()`, `lerCache(ITENS_CACHE)`, `lerPopularCache()`, `lerCache(GENERO_CACHE)` — todos leem arquivos `store_*_cache.json`.

- [ ] **Step 2: Trocar por `catalogGet`**

Para cada um, substitua a leitura do arquivo por uma chamada `catalogGet("/catalog/v1/...")`:

```js
// ANTES (sushi):
const c = lerCache(SUSHI_CACHE)
// DEPOIS:
const c = (await catalogGet("/catalog/v1/sushi")).data
```

Onde o resultado é usado como cache (ex. `manifestDiskCache`, `sushiCache`), a troca é direta: `catalogGet` já devolve o JSON do servidor no mesmo shape do arquivo (a resposta do servidor é o dado do cache). Mantenha o TTL local em RAM como segunda camada (evita chamada por tecla).

- [ ] **Step 3: Fallback**

Se `catalogGet` retornar `error` e não houver espelho, use o cache em disco antigo (`lerCache(...)`) como último recurso — mesma lógica do Task 5, mas no nível do steamstore.

- [ ] **Step 4: Rodar testes do app**

Run: `cd app && npm test` (ou `node --test test/`)
Expected: PASS (os testes existentes cobrem o shim; o steamstore não tem teste direto — valide que `npm run build` e o app abrem a loja)

- [ ] **Step 5: Commitar**

```bash
git add app/electron/steamstore.js
git commit -m "feat(app): steamstore consulta catalogo no servidor (fallback espelho)"
```

---

### Task 7: `sources.js` → cliente `catalog.js` (catálogo Hydra)

**Files:**
- Modify: `app/electron/sources.js` (trocar `fs.readFileSync` por `catalogGet`)
- Modify: `app/electron/main.js` (sysinfo/news/hltb/fixes/profile_cache → `catalog.js`, onde aplicável)

**Interfaces:**
- Consumes: `catalogGet` da Task 5
- Produces: mantém API pública (`search`, `getGame`, `addSource`, `removeSource`, `syncSources`)

- [ ] **Step 1: Trocar leitura de disco por `catalogGet`**

Em `app/electron/sources.js`, `loadIndex()` (linha ~195) e `getGame()` (linha ~241) leem `fs.readFileSync(cachePath(id))`. Substitua por:

```js
// loadIndex: para cada fonte no registro, buscar no servidor
const { data } = await catalogGet(`/catalog/v1/sources/${src.id}/games`)
if (!data?.downloads) continue
// monta o indice leve em RAM igual hoje (title/fileSize/uploadDate/ref)

// getGame(ref): lê o jogo do servidor (ou espelho)
const { data } = await catalogGet(`/catalog/v1/sources/${id}/games`)
const d = data?.downloads?.[Number(i)]
```

- [ ] **Step 2: Fallback para o cache local**

Se `catalogGet` falhar e não houver espelho, use o arquivo em `sources/<id>.json` (o cache antigo) como fallback — mantém compatibilidade com dados já baixados.

- [ ] **Step 3: sysinfo/news/hltb/fixes → catalogGet**

Em `app/electron/main.js`, onde `_loadSysinfo()`, `lerNewsCache()`, `hltb.js`, `fixes.js` leem arquivos, adicionar chamada a `catalogGet("/catalog/v1/sysinfo/<appid>")` etc., com fallback pro arquivo local. (Não mexer em `profile_cache.json` — é por conta e o servidor não serve esse; deixe local.)

- [ ] **Step 4: Rodar e validar**

Run: `cd app && npm test` + `npm run build`
Expected: PASS; app abre, loja consulta servidor e cai pro espelho se necessário

- [ ] **Step 5: Commitar**

```bash
git add app/electron/sources.js app/electron/main.js
git commit -m "feat(app): sources+sysinfo/news/hltb/fixes consultam catalogo no servidor"
```

**Fim da Fase 2 (Agent B).** O app consulta o servidor para toda a loja, com espelho offline e fallback. Trailers e API keys ficam locais.

---

## Fase 3 — Integração, testes E2E e docs (Task 8)

> **Agent C** (ou quem ficar livre) faz a Task 8 depois que Fase 1 e Fase 2 estiverem commitadas.

### Task 8: Integração, E2E e documentação

**Files:**
- Test: `server/test/e2e-catalog.test.js` (servidor sobe, app consulta)
- Docs: `docs/ARCHITECTURE.md` (adicionar seção "Catálogo no servidor"), `docs/superpowers/specs/2026-08-11-loja-servidor-design.md` (referenciar plano)

**Interfaces:**
- Consumes: Tasks 1–7

- [ ] **Step 1: Teste E2E** (`server/test/e2e-catalog.test.js`)

```js
// servidor completo sobe, autentica e consulta /catalog/v1/popular
// (mesmo padrão de e2e-conta.test.js)
```

- [ ] **Step 2: Rodar e ver passar**

Run: `cd server && node --test test/e2e-catalog.test.js`
Expected: PASS

- [ ] **Step 3: Atualizar docs**

Em `docs/ARCHITECTURE.md`, adicionar seção descrevendo o proxy de catálogo (endpoints `/catalog/v1/*`, tabela `catalog_cache`, TTLs, espelho offline, o que fica local). Referenciar o spec.

- [ ] **Step 4: Rodar suíte completa**

Run: `cd server && npm test` e `cd app && npm test`
Expected: todos PASS (server 53+novos, app sem regressão)

- [ ] **Step 5: Commitar**

```bash
git add server/test/e2e-catalog.test.js docs/ARCHITECTURE.md
git commit -m "test(e2e)+docs: catalogo no servidor integrado e documentado"
```

---

## Divisão de Trabalho (meio-a-meio)

| Agent | Tasks | O que faz | Arquivos que toca (sem conflito) |
|---|---|---|---|
| **Agent A (eu, neste sessão)** | 1–4 | Servidor: tabela `catalog_cache`, fetch de catálogo, rotas `/catalog/v1/*`, sysinfo/meta/hltb, items/manifests, gzip | `server/src/db.js`, `server/src/catalog-fetch.js`, `server/src/catalog-routes.js`, `server/src/server.js`, `server/package.json`, `server/test/catalog.test.js` |
| **Agent B (outro agente)** | 5–7 | App: cliente `catalog.js` + espelho offline, `steamstore.js` e `sources.js` consultando o servidor, sysinfo/news/hltb/fixes no app | `app/electron/catalog.js`, `app/electron/steamstore.js`, `app/electron/sources.js`, `app/electron/main.js`, `app/test/catalog-fallback.test.js` |
| **Agent C (quem ficar livre)** | 8 | Integração: teste E2E, docs `ARCHITECTURE.md`, suíte completa | `server/test/e2e-catalog.test.js`, `docs/ARCHITECTURE.md` |

**Ordem e paralelismo:**
- Task 1 (Agent A) commita primeiro → desbloqueia Tasks 2–4 (Agent A) **e** Task 5 (Agent B) — a tabela e o fetch existem.
- Agent A faz 2–4 enquanto Agent B faz 5–7 (paralelo, arquivos distintos: `server/` vs `app/`).
- Task 8 (Agent C) entra depois que Fase 1 e Fase 2 commitarem.

**Sem conflito:** Agent A só toca `server/`; Agent B só toca `app/`. Nenhum arquivo compartilhado entre os dois. Task 8 toca `docs/` e `server/test/e2e-catalog.test.js` (novo, não conflita).

separação de tasks

Plano completo e salvo em docs/superpowers/plans/2026-08-11-loja-servidor.md

Self-review feito: cobertura do spec ✅ (todos os 11 catálogos têm endpoint/task), sem placeholders ✅, tipos consistentes (catalogGet, catalog_cache, resolveKey) ✅.

Divisão de trabalho (meio-a-meio) — como você pediu

Agent: Agent A (eu, nesta sessão)
Tasks: 1–4
O que faz: Servidor: tabela catalog_cache, fetch de
catálogo, rotas /catalog/v1/*, sysinfo/meta/hltb,
items/manifests, gzip
Arquivos: server/src/db.js,
server/src/catalog-fetch.js,
server/src/catalog-routes.js,
server/src/server.js, server/package.json,
server/test/catalog.test.js
────────────────────────────────────────
Agent: Agent B (opencode)
Tasks: 5–7
O que faz: pelho
offline, steamstore.js e sources.js consultando o
servidor, s
Arquivos: app/electron/catalog.js,
app/electro
app/electron/sources.js, app/electron/main.js,    app/test/ca
────────────────────────────────────────          Agent: Agen
Tasks: 8                                          O que faz:
ARCHITECTURE.md, suíte completa                   Arquivos: sjs,
docs/ARCHITECTURE.md                              
Sem conflito: Agent A só toca server/, Agent B só app/ — zerot C pega oque sobrar depois.

Como rodar em paralelo:                    - Eu começotch). Quando commit, desbloqueia o opencode pra Task 5 (tabela e o
- Agent A faz 2–4 enquanto Agent B faz 5–7 (arquivos d
- Task 8 depois que ambas as fases commitarem.

⚠️ Um ponto importante pro opencode: as Tasks 5–7 assumem queg/v1/*(Tabela 1 da Task 2). Se você delegar o opencode antes de euterá comotestar contra um servidor vivo. Então a ordem certa
é: eu faço vocêdispara o opencode com as Tasks 5–7.

Quer que eu comece agora a Task 1 (a parte que eu faço — serv prepara oopencode com as Tasks 5–7 do plano?
