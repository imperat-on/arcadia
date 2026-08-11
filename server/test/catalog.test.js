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
