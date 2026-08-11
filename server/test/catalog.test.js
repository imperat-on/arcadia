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
  // popula o cache direto no SQLite
  db.prepare("INSERT OR REPLACE INTO catalog_cache (key, data, at) VALUES (?,?,?)").run(
    "popular",
    JSON.stringify({
      completa: [
        { appid: "10", title: "A", cover: "", manifest: false },
        { appid: "20", title: "B", cover: "", manifest: false },
      ],
    }),
    Math.floor(Date.now() / 1000),
  )
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
  db.prepare("INSERT OR REPLACE INTO catalog_cache (key, data, at) VALUES (?,?,?)").run(
    "sysinfo:2622380",
    JSON.stringify({ appid: "2622380", req_min: "16GB", req_rec: "32GB" }),
    Math.floor(Date.now() / 1000),
  )
  const r = await fetch(`${catBase}/catalog/v1/sysinfo/2622380`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  assert.equal(r.status, 200)
  const body = await r.json()
  assert.equal(body.data.req_min, "16GB")
})

test("catalog rotas: meta por appid devolve metadados", async () => {
  db.prepare("INSERT OR REPLACE INTO catalog_cache (key, data, at) VALUES (?,?,?)").run(
    "meta:2622380",
    JSON.stringify({ appid: "2622380", name: "Elden Ring", genre: "RPG" }),
    Math.floor(Date.now() / 1000),
  )
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
  db.prepare("INSERT OR REPLACE INTO catalog_cache (key, data, at) VALUES (?,?,?)").run(
    "items:2622380",
    JSON.stringify({ tipo: 0, capa: "u", heroi: "h", icon: "i" }),
    Math.floor(Date.now() / 1000),
  )
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

test("catalog rotas: search devolve itens do cache hydra", async () => {
  db.prepare("INSERT OR REPLACE INTO catalog_cache (key, data, at) VALUES (?,?,?)").run(
    "hydra:59e6a31484ce",
    JSON.stringify({ name: "fitgirl", downloads: [{ title: "Elden Ring" }, { title: "Cyberpunk" }] }),
    Math.floor(Date.now() / 1000),
  )
  const r = await fetch(`${catBase}/catalog/v1/search?q=elden`, {
    headers: { authorization: `Bearer ${tokenUsuario("zes")}` },
  })
  assert.equal(r.status, 200)
  const body = await r.json()
  assert.equal(body.itens.length, 1)
  assert.equal(body.itens[0].title, "Elden Ring")
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
    db.prepare("DELETE FROM catalog_cache").run()
    warmUpCatalog()
    // aguarda o warm-up em background terminar
    await new Promise((r) => setTimeout(r, 500))
    const popular = db.prepare("SELECT data FROM catalog_cache WHERE key='popular'").get()
    const sushi = db.prepare("SELECT data FROM catalog_cache WHERE key='sushi'").get()
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
  db.prepare("INSERT OR REPLACE INTO catalog_cache (key, data, at) VALUES (?,?,?)").run(
    "popular",
    JSON.stringify({ completa: [{ appid: "999", title: "MARCADOR", cover: "", manifest: false }] }),
    Math.floor(Date.now() / 1000),
  )
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
    const row = db.prepare("SELECT data FROM catalog_cache WHERE key='popular'").get()
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
  db.prepare("INSERT OR REPLACE INTO catalog_cache (key, data, at) VALUES (?,?,?)").run(
    "popular",
    JSON.stringify({ completa: [{ appid: "10", title: "CS", cover: "", manifest: false }] }),
    at,
  )
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
  db.prepare("INSERT OR REPLACE INTO catalog_cache (key, data, at) VALUES (?,?,?)").run(
    "manifests:2622380",
    JSON.stringify({ "https://sushi/2622380.zip": { ok: true } }),
    at,
  )
  db.prepare("INSERT OR REPLACE INTO catalog_cache (key, data, at) VALUES (?,?,?)").run(
    "manifests:10",
    JSON.stringify({ "https://ryuu/10": { ok: true } }),
    at,
  )
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
