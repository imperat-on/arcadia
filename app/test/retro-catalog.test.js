"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const crypto = require("node:crypto")
const {
  createRetroCatalog,
  normalizeSource,
  normalizePayload,
} = require("../electron/retro-catalog")

function tempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-retro-"))
}

function response(data, status = 200) {
  return new Response(JSON.stringify(data), {
    status,
    headers: { "content-type": "application/json" },
  })
}

test("catálogo Retro aceita somente fontes com status Classics", async () => {
  const dir = tempDir()
  const api = "https://api.test/sources"
  const source = {
    id: 10,
    title: "Fonte clássica",
    description: "PS1",
    url: "https://source.test/classics.json",
    gamesCount: 2,
    status: ["Trusted", "Classics"],
  }
  const ignored = { ...source, id: 11, title: "Fonte moderna", status: ["Trusted"] }
  const calls = []
  const fetchImpl = async (url) => {
    calls.push(url)
    if (url === api) return response({ sources: [source, ignored] })
    throw new Error(`unexpected ${url}`)
  }
  const catalog = createRetroCatalog({
    dataDir: dir,
    apiUrl: api,
    fetchImpl,
    sourcePayloads: {
      10: {
        name: "Fonte clássica",
        downloads: [
          {
            title: "Jogo clássico",
            uris: ["magnet:?xt=urn:btih:ABC123"],
            platform: "ps1",
            fileSize: "700 MB",
            descriptionHtml: '<b>Descrição</b> com <a href="https://example.test">link</a>',
          },
          { title: "Sem URI" },
        ],
      },
    },
  })
  try {
    const page = await catalog.list({ limit: 20 })
    assert.equal(page.ok, true)
    assert.equal(page.total, 1)
    assert.deepEqual(
      page.sources.map((item) => item.id),
      ["10"],
    )
    assert.equal(page.games[0].title, "Jogo clássico")
    assert.equal(page.games[0].sourceTitle, "Fonte clássica")
    assert.equal(page.games[0].description, "Descrição com link")
    assert.equal("uris" in page.games[0], false)
    const full = await catalog.getGame("10:0")
    assert.deepEqual(full.game.uris, ["magnet:?xt=urn:btih:ABC123"])
    assert.equal(calls.length, 1)
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

test("normalização rejeita URLs perigosas e URI sem transporte", () => {
  assert.equal(
    normalizeSource({ id: 1, title: "x", url: "http://example.test/x", status: ["Classics"] }),
    null,
  )
  const source = normalizeSource({
    id: 1,
    title: "x",
    url: "https://example.test/x",
    status: ["classics"],
  })
  assert.ok(source)
  assert.deepEqual(normalizePayload({ downloads: [] }, source), [])
  const games = normalizePayload(
    {
      name: "Feed",
      downloads: [
        { title: "ok", uris: ["https://example.test/file.zip", "file:///etc/passwd"] },
        { title: "bad", uris: ["javascript:alert(1)"] },
      ],
    },
    source,
  )
  assert.equal(games.length, 1)
  assert.deepEqual(games[0].uris, ["https://example.test/file.zip"])
})

test("pagina, busca e cache offline preservam o catálogo anterior", async () => {
  const dir = tempDir()
  const api = "https://api.test/sources"
  const source = {
    id: "classic-feed",
    title: "Feed",
    url: "https://source.test/feed.json",
    status: ["classics"],
  }
  const payload = {
    name: "Feed",
    downloads: [
      { title: "Alpha Game", uris: ["magnet:?xt=urn:btih:A"] },
      { title: "Beta Game", uris: ["magnet:?xt=urn:btih:B"] },
      { title: "Gamma Game", uris: ["magnet:?xt=urn:btih:C"] },
    ],
  }
  const fetchImpl = async (url) =>
    url === api ? response({ sources: [source] }) : response(payload)
  try {
    const first = createRetroCatalog({ dataDir: dir, apiUrl: api, fetchImpl, now: () => 100 })
    const page = await first.list({ query: "game", offset: 1, limit: 1 })
    assert.equal(page.total, 3)
    assert.equal(page.games[0].title, "Beta Game")
    assert.equal(page.hasMore, true)
    assert.equal(page.games[0].uris, undefined)
    const cached = createRetroCatalog({
      dataDir: dir,
      apiUrl: api,
      now: () => 100 + 2 * 24 * 60 * 60 * 1000,
      fetchImpl: async () => {
        throw new Error("offline")
      },
    })
    const offline = await cached.list({ limit: 2 })
    assert.equal(offline.ok, true)
    assert.equal(offline.total, 3)
    assert.equal(offline.games.length, 2)
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

test("tags alternativos e arte HTTPS são normalizados sem aceitar HTTP", () => {
  const source = normalizeSource({
    id: "tagged",
    name: "Tagged",
    link: "https://source.test/feed.json",
    status: ["Trusted"],
    tags: ["CLASSICS"],
  })
  assert.ok(source)
  const games = normalizePayload(
    {
      name: "Tagged",
      downloads: [
        {
          title: "Arte",
          uris: ["https://source.test/game.zip"],
          image: "https://images.test/cover.jpg",
          capa: "http://images.test/capa.jpg",
        },
      ],
    },
    source,
  )
  assert.equal(games[0].cover, "https://images.test/cover.jpg")
  assert.equal(games[0].capa, undefined)
})

test("falha sem cache retorna envelope de erro paginado", async () => {
  const catalog = createRetroCatalog({
    dataDir: tempDir(),
    apiUrl: "https://api.test/sources",
    fetchImpl: async () => {
      throw new Error("offline")
    },
  })
  const result = await catalog.list({ offset: -4, limit: 999 })
  assert.equal(result.ok, false)
  assert.deepEqual(result.games, [])
  assert.equal(result.offset, 0)
  assert.equal(result.limit, 48)
})

test("usa cache local de sources.js quando a fonte clássica responde 403", async () => {
  const dir = tempDir()
  const api = "https://api.test/sources"
  const url = "https://source.test/classics.json"
  const source = { id: 42, title: "Cached", url, status: ["Classics"] }
  const id = crypto.createHash("sha256").update(url).digest("hex").slice(0, 12)
  const file = path.join(dir, "sources", `${id}.json`)
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.writeFileSync(
    file,
    JSON.stringify({
      name: "Cached feed",
      downloads: [{ title: "Offline classic", uris: ["magnet:?xt=urn:btih:CACHED"] }],
    }),
  )
  try {
    const catalog = createRetroCatalog({
      dataDir: dir,
      apiUrl: api,
      fetchImpl: async (requestUrl) => {
        if (requestUrl === api) return response({ sources: [source] })
        throw new Error("HTTP 403")
      },
    })
    const page = await catalog.list()
    assert.equal(page.ok, true)
    assert.equal(page.games[0].title, "Offline classic")
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

test("cache rejeita symlink e não segue destino externo", async () => {
  const dir = tempDir()
  const target = path.join(dir, "outside.json")
  const cache = path.join(dir, "retro-catalog.json")
  fs.writeFileSync(
    target,
    JSON.stringify({
      version: 1,
      updatedAt: Date.now(),
      sources: [],
      games: [{ id: "bad", title: "Bad" }],
    }),
  )
  fs.symlinkSync(target, cache)
  try {
    const catalog = createRetroCatalog({
      dataDir: dir,
      fetchImpl: async () => {
        throw new Error("offline")
      },
    })
    const result = await catalog.list()
    assert.equal(result.ok, false)
    assert.deepEqual(result.games, [])
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})
