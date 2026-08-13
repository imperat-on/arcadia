"use strict"

// Testes do proxy de catalogo da loja: catalog-fetch (fetch externo) e
// catalog-routes (/catalog/v1/*). Roda com node --test, DB temporario.

const test = require("node:test")
const assert = require("node:assert")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

process.env.DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-catalog-"))

const { fetchCatalogKey, CATALOG_KEYS, CATALOG_TTL, catalogKey, idValido } = require("../src/catalog-fetch")
const { db } = require("../src/db")

const UPSERT_CACHE = `INSERT INTO catalog_cache (key, data, at) VALUES ($1, $2, $3)
  ON CONFLICT (key) DO UPDATE SET data = excluded.data, at = excluded.at`

// Stub do fetch global: devolve JSON por URL.
function stubFetch(map) {
  const antigo = global.fetch
  global.fetch = async (url, opts = {}) => {
    const chave = `${String(opts.method || "GET")} ${url}`
    const alvo = map[chave] ?? map[url]
    if (alvo === undefined) return { ok: false, status: 404, json: async () => ({}) }
    if (typeof alvo === "function") return alvo()
    return { ok: true, status: 200, json: async () => alvo }
  }
  return () => {
    global.fetch = antigo
  }
}

test("catalog-fetch: allowlist tem as chaves base", () => {
  assert.ok(Array.isArray(CATALOG_KEYS))
  for (const k of ["popular", "sushi", "news", "fixes", "ryuu-index"]) {
    assert.ok(CATALOG_KEYS.includes(k), `falta ${k}`)
  }
  // nao aceita path traversal
  assert.ok(!CATALOG_KEYS.includes("../../etc/passwd"))
})

test("catalog-fetch: idValido valida formatos por prefixo", () => {
  assert.ok(idValido("hydra:", "59e6a31484ce")) // sha256.slice(0,12)
  assert.ok(!idValido("hydra:", "../../x"))
  assert.ok(idValido("sysinfo:", "2622380"))
  assert.ok(!idValido("sysinfo:", "abc"))
  assert.ok(idValido("genre:", "__all"))
  assert.ok(!idValido("genre:", "../"))
})

test("catalog-fetch: catalogKey monta keys validas", () => {
  assert.equal(catalogKey("popular"), "popular")
  assert.equal(catalogKey("sushi"), "sushi")
  assert.equal(catalogKey("hydra", "59e6a31484ce"), "hydra:59e6a31484ce")
  assert.equal(catalogKey("hydra", "../etc"), null)
  assert.equal(catalogKey("sysinfo", "2622380"), "sysinfo:2622380")
  assert.equal(catalogKey("items", "2622380"), "items:2622380")
  assert.equal(catalogKey("bogus"), null)
})

test("catalog-fetch: fetchCatalogKey baixa popular do SteamSpy", async () => {
  const restaurar = stubFetch({
    "https://steamspy.com/api.php?request=top100in2weeks": {
      "10": { appid: 10, name: "Counter-Strike", ccu: 5000 },
      "70": { appid: 70, name: "Half-Life", ccu: 3000 },
    },
  })
  try {
    const r = await fetchCatalogKey("popular")
    assert.ok(r)
    assert.ok(Array.isArray(r.data.completa))
    assert.equal(r.data.completa[0].appid, "10") // mais jogado primeiro
    assert.equal(r.data.completa[0].cover, "https://cdn.akamai.steamstatic.com/steam/apps/10/header.jpg")
    assert.ok(r.at > 0)
  } finally {
    restaurar()
  }
})

test("catalog-fetch: fetchCatalogKey baixa sushi do GitHub", async () => {
  const restaurar = stubFetch({
    "https://api.github.com/repos/sushi-dev55-alt/sushitools-games-repo-alt/git/trees/main": {
      truncated: false,
      tree: [{ path: "100.zip" }, { path: "200.zip" }, { path: "readme.md" }],
    },
  })
  try {
    const r = await fetchCatalogKey("sushi")
    assert.ok(r)
    assert.deepEqual(r.data.ids, ["100", "200"])
  } finally {
    restaurar()
  }
})

test("catalog-fetch: fetchCatalogKey devolve null para chave desconhecida", async () => {
  const r = await fetchCatalogKey("bogus:1")
  assert.equal(r, null)
})

test("catalog-fetch: TTL tem entradas para os prefixos usados", () => {
  assert.equal(CATALOG_TTL.popular, 6 * 60 * 60)
  assert.equal(CATALOG_TTL.sushi, 6 * 60 * 60)
  assert.equal(CATALOG_TTL["sysinfo:"], 0) // sem validade
  assert.equal(CATALOG_TTL["items:"], 7 * 24 * 60 * 60)
})

test("catalog-fetch: fetchSysinfo extrai requisitos do appdetails", async () => {
  const restaurar = stubFetch({
    "https://store.steampowered.com/api/appdetails?appids=2622380&l=english": {
      "2622380": {
        data: {
          pc_requirements: { minimum: "<strong>16GB RAM</strong>", recommended: "32GB" },
          short_description: "desc",
          header_image: "h",
          background_raw: "b",
        },
      },
    },
  })
  try {
    const r = await fetchCatalogKey("sysinfo:2622380")
    assert.ok(r)
    assert.equal(r.data.req_min, "<strong>16GB RAM</strong>")
    assert.equal(r.data.req_rec, "32GB")
    assert.equal(r.data.appid, "2622380")
    assert.equal(r.data.header, "h")
  } finally {
    restaurar()
  }
})

test("catalog-fetch: fetchItems extrai tipo+arte do IStoreBrowseService", async () => {
  // a URL real leva ?input_json=... — casa por prefixo da base
  const antigo = global.fetch
  global.fetch = async (url) => {
    if (String(url).startsWith("https://api.steampowered.com/IStoreBrowseService/GetItems/v1/")) {
      return {
        ok: true,
        status: 200,
        json: async () => ({
          response: {
            store_items: [
              {
                appid: 2622380,
                type: 0,
                assets: {
                  asset_url_format: "https://cdn/steam/apps/2622380/${FILENAME}",
                  library_capsule: "capsule_616x353.jpg",
                  library_hero_2x: "library_hero_2x.jpg",
                  icon: "icon.jpg",
                },
              },
            ],
          },
        }),
      }
    }
    return { ok: false, status: 404, json: async () => ({}) }
  }
  try {
    const r = await fetchCatalogKey("items:2622380")
    assert.ok(r)
    assert.equal(r.data.tipo, 0)
    assert.ok(r.data.capa.includes("capsule_616x353.jpg"))
    assert.ok(r.data.heroi.includes("library_hero_2x.jpg"))
    assert.ok(r.data.icon.includes("icon.jpg"))
  } finally {
    global.fetch = antigo
  }
})

test("catalog-fetch: fetchManifests sonda provedores com HEAD", async () => {
  const restaurar = stubFetch({})
  // sobrecarrega para HEAD: ryuu 200, sushi 404, twentytwo erro
  global.fetch = async (url, opts = {}) => {
    const metodo = opts.method || "GET"
    if (metodo === "HEAD" && url.includes("167.235.229.108")) return { ok: true, status: 200 }
    if (metodo === "HEAD" && url.includes("raw.githubusercontent")) return { ok: false, status: 404 }
    if (url.includes("twentytwocloud")) throw new Error("rede fora")
    return { ok: false, status: 404 }
  }
  try {
    const r = await fetchCatalogKey("manifests:2622380")
    assert.ok(r)
    const urls = Object.keys(r.data)
    assert.equal(urls.length, 3)
    const okValues = Object.values(r.data).filter((v) => v.ok)
    assert.equal(okValues.length, 1) // so o ryuu respondeu 200
  } finally {
    restaurar()
  }
})

test("catalog-fetch: fetchMeta extrai metadados do appdetails", async () => {
  const restaurar = stubFetch({
    "https://store.steampowered.com/api/appdetails?appids=2622380&l=english": {
      "2622380": {
        data: {
          name: "Elden Ring",
          developers: ["FromSoftware"],
          publishers: ["Bandai"],
          genres: [{ description: "RPG" }],
          release_date: { date: "2022" },
        },
      },
    },
  })
  try {
    const r = await fetchCatalogKey("meta:2622380")
    assert.ok(r)
    assert.equal(r.data.name, "Elden Ring")
    assert.equal(r.data.genre, "RPG")
    assert.deepEqual(r.data.developers, ["FromSoftware"])
  } finally {
    restaurar()
  }
})

// ---------- Rotas /catalog/v1/* ----------
const express = require("express")
const { registerCatalogRoutes } = require("../src/catalog-routes")

const catApp = express()
catApp.use(express.json())
registerCatalogRoutes(catApp)
const listener = catApp.listen(0)
const catBase = `http://127.0.0.1:${listener.address().port}`
// fecha o listener no fim para o node --test nao pendurar
test.after(() => new Promise((r) => listener.close(r)))
const JWT = require("../src/jwt")

// Emite um JWT valido com sub=user1 (mesma chave do server em teste)
function tokenUsuario(username) {
  // issueTokens assina com SECRET do jwt.js; usamos o mesmo modulo.
  // Nao ha profile real aqui, entao montamos um minimo.
  const { issueTokens } = JWT
  const t = issueTokens({ id: "user1", email: `${username}@teste`, username })
  return t.access_token
}

test("catalog rotas: sem JWT devolve 401", async () => {
  const r = await fetch(`${catBase}/catalog/v1/popular`)
  assert.equal(r.status, 401)
})

test("catalog rotas: com JWT e cache, popular devolve fatia paginada", async () => {
  // popula o cache direto no PostgreSQL
  await db.query(UPSERT_CACHE, [
    "popular",
    JSON.stringify({
      completa: [
        { appid: "10", title: "A", cover: "", manifest: false },
        { appid: "20", title: "B", cover: "", manifest: false },
      ],
    }),
    Math.floor(Date.now() / 1000),
  ])
  const r = await fetch(`${catBase}/catalog/v1/popular?limite=1&offset=1`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  assert.equal(r.status, 200)
  const body = await r.json()
  assert.equal(body.ok, true)
  assert.equal(body.total, 2)
  assert.equal(body.itens.length, 1)
  assert.equal(body.itens[0].appid, "20")
})

test("catalog rotas: sysinfo por appid devolve requisitos", async () => {
  await db.query(UPSERT_CACHE, [
    "sysinfo:2622380",
    // `v` = formato atual: sem isto a rota trataria como cache velho e iria a
    // fonte externa (o teste passaria a depender de rede).
    JSON.stringify({ v: 2, appid: "2622380", req_min: "16GB", req_rec: "32GB" }),
    Math.floor(Date.now() / 1000),
  ])
  const r = await fetch(`${catBase}/catalog/v1/sysinfo/2622380`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  assert.equal(r.status, 200)
  const body = await r.json()
  assert.equal(body.data.req_min, "16GB")
})

test("catalog rotas: meta por appid devolve metadados", async () => {
  await db.query(UPSERT_CACHE, [
    "meta:2622380",
    JSON.stringify({ v: 1, appid: "2622380", name: "Elden Ring", genre: "RPG" }),
    Math.floor(Date.now() / 1000),
  ])
  const r = await fetch(`${catBase}/catalog/v1/meta/2622380`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  assert.equal(r.status, 200)
  const body = await r.json()
  assert.equal(body.data.name, "Elden Ring")
})

test("catalog rotas: hltb devolve 404 sem cache (placeholder)", async () => {
  const r = await fetch(`${catBase}/catalog/v1/hltb/2622380`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  assert.equal(r.status, 404)
})

test("catalog rotas: items em lote devolve mapa por appid", async () => {
  await db.query(UPSERT_CACHE, [
    "items:2622380",
    JSON.stringify({ tipo: 0, capa: "u", heroi: "h", icon: "i" }),
    Math.floor(Date.now() / 1000),
  ])
  const r = await fetch(`${catBase}/catalog/v1/items?appids=2622380`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  assert.equal(r.status, 200)
  const body = await r.json()
  assert.equal(body.data["2622380"].tipo, 0)
})

test("catalog rotas: key invalida devolve 400", async () => {
  // id de fonte nao-hexadecimal: passa na rota mas falha a allowlist
  const r = await fetch(`${catBase}/catalog/v1/sources/not-a-valid-hash/games`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  assert.equal(r.status, 400)
})

test("catalog rotas: cache vazio devolve 404", async () => {
  const r = await fetch(`${catBase}/catalog/v1/fixes`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  assert.equal(r.status, 404)
})

test("catalog rotas: search devolve jogos do catalogo completo", async () => {
  await db.query(UPSERT_CACHE, [
    "catalogo_completo",
    JSON.stringify({ completa: [
      { appid: "1245620", title: "ELDEN RING" },
      { appid: "1091500", title: "Cyberpunk 2077" },
    ] }),
    Math.floor(Date.now() / 1000),
  ])
  const r = await fetch(`${catBase}/catalog/v1/search?q=elden`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  assert.equal(r.status, 200)
  const body = await r.json()
  assert.equal(body.itens.length, 1)
  assert.equal(body.itens[0].title, "ELDEN RING")
  assert.equal(body.itens[0].appid, "1245620")
})

test("catalog rotas: search rankeia prefixo exato antes de substring", async () => {
  await db.query(UPSERT_CACHE, [
    "catalogo_completo",
    JSON.stringify({ completa: [
      { appid: "99999", title: "Super Cyberpunk Simulator" },
      { appid: "1091500", title: "Cyberpunk 2077" },
      { appid: "55555", title: "The Punky Adventure" },
    ] }),
    Math.floor(Date.now() / 1000),
  ])
  const r = await fetch(`${catBase}/catalog/v1/search?q=cyberpunk`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  assert.equal(r.status, 200)
  const body = await r.json()
  assert.equal(body.itens.length, 2)
  // Prefixo exato ("Cyberpunk 2077") vem antes do substring ("Super Cyberpunk")
  assert.equal(body.itens[0].title, "Cyberpunk 2077")
  assert.equal(body.itens[1].title, "Super Cyberpunk Simulator")
})

test("catalog rotas: search casa por palavra (limite) e nao so prefixo", async () => {
  await db.query(UPSERT_CACHE, [
    "catalogo_completo",
    JSON.stringify({ completa: [
      { appid: "1245620", title: "ELDEN RING" },
      { appid: "77777", title: "Some Other Game" },
    ] }),
    Math.floor(Date.now() / 1000),
  ])
  const r = await fetch(`${catBase}/catalog/v1/search?q=ring`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  assert.equal(r.status, 200)
  const body = await r.json()
  assert.equal(body.itens.length, 1)
  assert.equal(body.itens[0].title, "ELDEN RING")
})

test("catalog rotas: warmUpCatalog popula popular/sushi/news em background", async () => {
  const { warmUpCatalog } = require("../src/catalog-routes")
  const restaurar = stubFetch({
    "https://steamspy.com/api.php?request=top100in2weeks": {
      "10": { appid: 10, name: "CS", ccu: 5 },
    },
    "https://api.github.com/repos/sushi-dev55-alt/sushitools-games-repo-alt/git/trees/main": {
      truncated: false,
      tree: [{ path: "10.zip" }],
    },
  })
  try {
    // limpa cache antes
    await db.query("DELETE FROM catalog_cache")
    warmUpCatalog()
    // aguarda o warm-up em background terminar
    await new Promise((r) => setTimeout(r, 500))
    const popular = (await db.query(
      "SELECT data FROM catalog_cache WHERE key = $1",
      ["popular"],
    )).rows[0]
    const sushi = (await db.query(
      "SELECT data FROM catalog_cache WHERE key = $1",
      ["sushi"],
    )).rows[0]
    assert.ok(popular, "popular deve estar em cache")
    assert.ok(sushi, "sushi deve estar em cache")
    assert.ok(JSON.parse(popular.data).completa.length === 1)
    assert.deepEqual(JSON.parse(sushi.data).ids, ["10"])
  } finally {
    restaurar()
  }
})

test("catalog rotas: warmUpCatalog NAO re-busca cache valido", async () => {
  const { warmUpCatalog } = require("../src/catalog-routes")
  // popula popular com dados "marcadores" e at recente (dentro do TTL 6h)
  await db.query(UPSERT_CACHE, [
    "popular",
    JSON.stringify({ completa: [{ appid: "999", title: "MARCADOR", cover: "", manifest: false }] }),
    Math.floor(Date.now() / 1000),
  ])
  // stub que, se chamado, retornaria dados DIFERENTES — nao deve ser chamado
  const chamou = { fetch: false }
  const antigo = global.fetch
  global.fetch = async (url) => {
    if (url.includes("steamspy")) {
      chamou.fetch = true
      return { ok: true, status: 200, json: async () => ({ "1": { appid: 1, name: "DIFERENTE", ccu: 1 } }) }
    }
    return { ok: false, status: 404, json: async () => ({}) }
  }
  try {
    warmUpCatalog()
    await new Promise((r) => setTimeout(r, 300))
    // o cache NAO deve ter sido sobrescrito pelo fetch
    const row = (await db.query(
      "SELECT data FROM catalog_cache WHERE key = $1",
      ["popular"],
    )).rows[0]
    const data = JSON.parse(row.data)
    assert.equal(data.completa[0].title, "MARCADOR", "cache valido nao pode ser re-buscado")
    assert.equal(chamou.fetch, false, "nao deve chamar a fonte para cache fresco")
  } finally {
    global.fetch = antigo
  }
})

test("catalog rotas: If-None-Match com etag atual devolve 304", async () => {
  // garante que existe cache de popular com at conhecido
  const at = Math.floor(Date.now() / 1000)
  await db.query(UPSERT_CACHE, [
    "popular",
    JSON.stringify({ completa: [{ appid: "10", title: "CS", cover: "", manifest: false }] }),
    at,
  ])
  // primeira chamada pega o etag
  const r1 = await fetch(`${catBase}/catalog/v1/popular`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  const etag = r1.headers.get("etag")
  assert.ok(etag, "deve ter etag")
  // segunda chamada com If-None-Match igual -> 304, corpo vazio
  const r2 = await fetch(`${catBase}/catalog/v1/popular`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}`, "if-none-match": etag },
  })
  assert.equal(r2.status, 304)
  const corpo = await r2.text()
  assert.equal(corpo.length, 0)
  // etag diferente -> 200
  const r3 = await fetch(`${catBase}/catalog/v1/popular`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}`, "if-none-match": '"0"' },
  })
  assert.equal(r3.status, 200)
})

test("catalog rotas: manifests em lote devolve varios appids numa chamada", async () => {
  // popula cache de manifests para 2 appids
  const at = Math.floor(Date.now() / 1000)
  await db.query(UPSERT_CACHE, [
    "manifests:2622380",
    JSON.stringify({ "https://sushi/2622380.zip": { ok: true } }),
    at,
  ])
  await db.query(UPSERT_CACHE, [
    "manifests:10",
    JSON.stringify({ "https://ryuu/10": { ok: true } }),
    at,
  ])
  const r = await fetch(`${catBase}/catalog/v1/manifests?appids=2622380,10`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  assert.equal(r.status, 200)
  const body = await r.json()
  assert.ok(body.data["2622380"], "deve ter 2622380")
  assert.ok(body.data["10"], "deve ter 10")
  assert.equal(body.data["2622380"]["https://sushi/2622380.zip"].ok, true)
  assert.equal(body.data["10"]["https://ryuu/10"].ok, true)
})

test("catalog rotas: manifests em lote ignora appids invalidos", async () => {
  const r = await fetch(`${catBase}/catalog/v1/manifests?appids=2622380,abc,,x`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  assert.equal(r.status, 200)
  const body = await r.json()
  assert.ok(body.data["2622380"], "appid valido processado")
  assert.equal(body.data["abc"], undefined, "appid invalido ignorado")
})

test("catalog-fetch: catalogKey normaliza genre all para __all", () => {
  assert.equal(catalogKey("genre", "all"), "genre:__all")
  assert.equal(catalogKey("genre", "__all"), "genre:__all")
  assert.equal(catalogKey("genre", "rpg"), "genre:rpg")
})

test("catalog rotas: genre?lista=all serve o catalogo Em alta (nao 400)", async () => {
  // popula o popular no cache
  await db.query(UPSERT_CACHE, [
    "popular",
    JSON.stringify({ completa: [{ appid: "10", title: "CS", cover: "", manifest: false }] }),
    Math.floor(Date.now() / 1000),
  ])
  const r = await fetch(`${catBase}/catalog/v1/genre?lista=all`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  assert.equal(r.status, 200)
  const body = await r.json()
  assert.ok(Array.isArray(body.data.completa) || Array.isArray(body.data), "deve ter o catalogo")
})

test("catalog rotas: steam250 devolve jogos com nome paginados", async () => {
  // popula steam250 + items de alguns appids
  const at = Math.floor(Date.now() / 1000)
  await db.query(UPSERT_CACHE, [
    "steam250",
    JSON.stringify({ completa: [
      { appid: "413150", title: "Stardew Valley" },
      { appid: "105600", title: "Terraria" },
      { appid: "550", title: "Left 4 Dead 2" },
      { appid: "730", title: "Counter-Strike 2" },
      { appid: "570", title: "Dota 2" },
    ] }),
    at,
  ])
  await db.query(UPSERT_CACHE, [
    "items:413150",
    JSON.stringify({ tipo: 0, capa: "capa", heroi: "heroi" }),
    at,
  ])
  const r = await fetch(`${catBase}/catalog/v1/steam250?offset=0&limite=2`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  assert.equal(r.status, 200)
  const body = await r.json()
  assert.equal(body.total, 5)
  assert.equal(body.itens.length, 2)
  assert.equal(body.itens[0].appid, "413150")
  assert.equal(body.itens[0].title, "Stardew Valley")
  // paginação: offset=2 traz os próximos
  const r2 = await fetch(`${catBase}/catalog/v1/steam250?offset=2&limite=2`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  const b2 = await r2.json()
  assert.equal(b2.itens[0].appid, "550")
  assert.equal(b2.itens[0].title, "Left 4 Dead 2")
})

test("catalog rotas: reviews POST adiciona e GET devolve", async () => {
  // cria um profile real para o user_id da review (FK valida)
  await db.query(
    `INSERT INTO profiles (id, email, password_hash, username) VALUES ($1, $2, $3, $4)
     ON CONFLICT DO NOTHING`,
    ["user1", "user1@teste", "hash", "user1"],
  )
  // POST uma review (autenticado)
  const rPost = await fetch(`${catBase}/catalog/v1/reviews/730`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${tokenUsuario("zes")}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({ text: "Jogo incrivel!", positive: true, hours: 120 }),
  })
  assert.equal(rPost.status, 200)
  // GET devolve a review
  const rGet = await fetch(`${catBase}/catalog/v1/reviews/730`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  assert.equal(rGet.status, 200)
  const body = await rGet.json()
  assert.ok(body.reviews.length >= 1)
  assert.equal(body.reviews[0].text, "Jogo incrivel!")
  assert.equal(body.reviews[0].positive, 1)
  assert.ok(body.reviews[0].username, "deve ter o username do autor")
})

// Stub que SO intercepta a fonte externa: o fetch para o proprio servidor de
// teste (127.0.0.1) continua real. Sem isto o stub responderia a requisicao da
// rota antes de o Express rodar.
function stubExterno(map) {
  const antigo = global.fetch
  global.fetch = async (url, opts = {}) => {
    const u = String(url)
    if (u.startsWith("http://127.0.0.1")) return antigo(url, opts)
    const alvo = map[u]
    if (alvo === undefined) return { ok: false, status: 404, json: async () => ({}) }
    return { ok: true, status: 200, json: async () => alvo }
  }
  return () => {
    global.fetch = antigo
  }
}

// Regressao: sysinfo nao tem TTL, entao uma linha gravada ANTES de um campo
// novo (o `about`, descricao rica) ficaria velha para sempre. A versao do
// payload faz a linha antiga se corrigir sozinha na primeira consulta.
test("catalog rotas: sysinfo em formato antigo (sem about) se corrige sozinho", async () => {
  const APPID = "424370"
  // Linha no formato ANTIGO: sem `v` e sem `about`, como as ja gravadas.
  await db.query(UPSERT_CACHE, [
    `sysinfo:${APPID}`,
    JSON.stringify({ appid: APPID, req_min: "8GB", req_rec: "16GB", short_description: "velho" }),
    Math.floor(Date.now() / 1000),
  ])

  const restaurar = stubExterno({
    [`https://store.steampowered.com/api/appdetails?appids=${APPID}&l=english`]: {
      [APPID]: {
        data: {
          pc_requirements: { minimum: "8GB", recommended: "16GB" },
          short_description: "novo",
          about_the_game: '<p>Descricao</p><img src="https://cdn/a.jpg">',
          header_image: "h",
        },
      },
    },
  })
  try {
    const r = await fetch(`${catBase}/catalog/v1/sysinfo/${APPID}`, {
      headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
    })
    assert.equal(r.status, 200)
    const body = await r.json()
    assert.ok(body.data.about, "deve devolver a descricao rica ja na 1a consulta")
    assert.match(body.data.about, /<img/, "about precisa manter as imagens")
    assert.equal(body.data.v, 2, "payload novo carimba a versao")
  } finally {
    restaurar()
  }

  // A linha no PostgreSQL foi reescrita no formato novo (nao rebusca na proxima).
  const salvo = JSON.parse((await db.query(
    "SELECT data FROM catalog_cache WHERE key = $1",
    [`sysinfo:${APPID}`],
  )).rows[0].data)
  assert.equal(salvo.v, 2)
  assert.match(salvo.about, /<img/)
})

// Fonte externa fora + cache em formato antigo: serve o que tem (nao 404).
test("catalog rotas: formato antigo sobrevive se a Steam estiver fora", async () => {
  const APPID = "424380"
  await db.query(UPSERT_CACHE, [
    `sysinfo:${APPID}`,
    JSON.stringify({ appid: APPID, req_min: "4GB", short_description: "antigo" }),
    Math.floor(Date.now() / 1000),
  ])
  const restaurar = stubExterno({}) // qualquer fonte externa -> 404
  try {
    const r = await fetch(`${catBase}/catalog/v1/sysinfo/${APPID}`, {
      headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
    })
    assert.equal(r.status, 200, "cache antigo e melhor que 404")
    const body = await r.json()
    assert.equal(body.data.short_description, "antigo")
  } finally {
    restaurar()
  }
})

// Cache ja no formato atual e servido direto, sem tocar a fonte externa.
test("catalog rotas: sysinfo no formato atual nao rebusca", async () => {
  const APPID = "424390"
  await db.query(UPSERT_CACHE, [
    `sysinfo:${APPID}`,
    JSON.stringify({ v: 2, appid: APPID, req_min: "2GB", about: "<p>ok</p>" }),
    Math.floor(Date.now() / 1000),
  ])
  let bateuNaFonte = false
  const antigo = global.fetch
  global.fetch = async (url, opts = {}) => {
    const u = String(url)
    if (!u.startsWith("http://127.0.0.1")) bateuNaFonte = true
    return antigo(url, opts)
  }
  try {
    const r = await fetch(`${catBase}/catalog/v1/sysinfo/${APPID}`, {
      headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
    })
    assert.equal(r.status, 200)
    const body = await r.json()
    assert.equal(body.data.req_min, "2GB")
    assert.equal(bateuNaFonte, false, "formato atual nao pode disparar fetch externo")
  } finally {
    global.fetch = antigo
  }
})
