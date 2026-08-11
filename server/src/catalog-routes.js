"use strict"

// Proxy de catalogo da loja: rotas /catalog/v1/*. O servidor busca a fonte
// externa uma vez por TTL, guarda em catalog_cache (SQLite) e responde JSON
// pronto ao app. Tudo exige JWT valido.
//
// Privacidade: nenhuma chave paga do usuario (Hubcap, debrid, Steam) existe
// neste modulo nem nos dados que ele serve. Fontes que exigem credencial
// continuam sendo buscadas diretamente pelo app.

const { db, nowEpochS } = require("./db")
const { verifyToken, extractToken } = require("./jwt")
const { fetchCatalogKey, catalogKey, CATALOG_KEYS, CATALOG_TTL } = require("./catalog-fetch")

// Extrai o Bearer token e valida. Devolve o sub (user id) ou null.
function requireAuth(req) {
  const token = extractToken(req)
  const v = verifyToken(token || "")
  return v.ok ? v.sub : null
}

// Grava o resultado de um fetch no cache SQLite (atomico).
function gravarCache(key, r) {
  db.prepare("INSERT OR REPLACE INTO catalog_cache (key, data, at) VALUES (?,?,?)").run(
    key,
    JSON.stringify(r.data),
    r.at,
  )
}

// Le do cache (sem revalidar). Devolve o JSON ou null se nao ha entrada.
function lerCache(key) {
  const row = db.prepare("SELECT data FROM catalog_cache WHERE key = ?").get(key)
  return row ? JSON.parse(row.data) : null
}

// Busca da fonte externa e grava. Devolve { data } ou null se a fonte falhou.
// Cold-start: quando nao ha cache, a rota chama isto (espera) antes de
// responder — o 404 so aparece se a fonte externa tambem falhar.
async function buscar(key) {
  const r = await fetchCatalogKey(key).catch(() => null)
  if (!r) return null
  gravarCache(key, r)
  return r.data
}

// Le do cache; se vencido, revalida em background (stale-while-revalidate) e
// devolve o que tem. Nunca bloqueia a resposta em rede externa lenta quando
// ja ha cache. Quando NAO ha cache, devolve null (o cold-start fica na rota,
// que chama `buscar` e espera a fonte externa).
function getCached(key) {
  const ttl = CATALOG_TTL[key] ?? CATALOG_TTL[`${key.split(":")[0]}:`] ?? 0
  const row = db.prepare("SELECT data, at FROM catalog_cache WHERE key = ?").get(key)
  if (!row) return null
  const data = JSON.parse(row.data)
  const agora = nowEpochS()
  if (ttl > 0 && agora - row.at > ttl) {
    // revalida em background; erro de rede nao derruba a resposta
    fetchCatalogKey(key)
      .then((r) => {
        if (r) gravarCache(key, r)
      })
      .catch(() => {})
  }
  return data
}

// Resolve a key, le do cache (ou busca no cold start) e responde.
// key invalida -> 400; sem cache E fonte externa falhou -> 404.
async function responder(uid, req, res, tipo, id) {
  const key = catalogKey(tipo, id)
  if (!key) return res.status(400).json({ error: "key_invalida" })
  let data = getCached(key)
  if (data === null) {
    data = await buscar(key)
    if (data === null) return res.status(404).json({ error: "cache_vazio" })
  }
  // ETag: o app manda If-None-Match na proxima vez; se nada mudou (mesmo
  // timestamp), devolve 304 (0 bytes) em vez de re-baixar o JSON inteiro.
  const etag = `"${getCacheAt(key)}"`
  if (req.headers["if-none-match"] === etag) return res.status(304).end()
  res.set("ETag", etag)
  return res.json({ ok: true, data })
}

// Lê o `at` (timestamp) de uma chave de cache — usado como ETag fraco.
function getCacheAt(key) {
  const row = db.prepare("SELECT at FROM catalog_cache WHERE key = ?").get(key)
  return row ? row.at : 0
}

function registerCatalogRoutes(app) {
  // Todos os endpoints exigem JWT (Bearer).
  app.use("/catalog/v1", (req, res, next) => {
    if (!requireAuth(req)) return res.status(401).json({ error: "nao_autenticado" })
    next()
  })

  // Em alta / populares (SteamSpy). Retorna fatia paginada + total.
  app.get("/catalog/v1/popular", async (req, res) => {
    let data = getCached("popular")
    if (data === null) {
      data = await buscar("popular")
      if (data === null) return res.status(404).json({ error: "cache_vazio" })
    }
    const etag = `"${getCacheAt("popular")}"`
    if (req.headers["if-none-match"] === etag) return res.status(304).end()
    res.set("ETag", etag)
    const completa = Array.isArray(data.completa) ? data.completa : []
    const limite = Math.max(1, Number(req.query.limite) || 40)
    const offset = Math.max(0, Number(req.query.offset) || 0)
    res.json({ ok: true, itens: completa.slice(offset, offset + limite), total: completa.length, offset })
  })

  // Catálogo infinito: os 5864 appids do sushi (com manifesto) paginados,
  // com nome+arte buscados sob demanda (meta/items) e cacheados. Permite
  // rolar/paginar como a Steam — 244 páginas de 24 em vez de só 5 do popular.
  app.get("/catalog/v1/catalog", async (req, res) => {
    const limite = Math.max(1, Number(req.query.limite) || 24)
    const offset = Math.max(0, Number(req.query.offset) || 0)
    // 1. appids do sushi (lista mestre de jogos instaláveis)
    let sushi = getCached("sushi")
    if (sushi === null) {
      sushi = await buscar("sushi")
      if (sushi === null) return res.status(404).json({ error: "cache_vazio" })
    }
    const ids = Array.isArray(sushi.ids) ? sushi.ids : []
    const fatia = ids.slice(offset, offset + limite)
    // 2. para cada appid da página, nome (meta) + arte (items) sob demanda
    const itens = []
    await Promise.all(
      fatia.map(async (appid) => {
        const meta = getCached(`meta:${appid}`) ?? (await buscar(`meta:${appid}`))
        const item = getCached(`items:${appid}`) ?? (await buscar(`items:${appid}`))
        if (!meta && !item) return
        itens.push({
          appid: String(appid),
          title: meta?.name || String(appid),
          cover: `https://cdn.akamai.steamstatic.com/steam/apps/${appid}/header.jpg`,
          heroi: item?.heroi || "",
          capa: item?.capa || "",
        })
      }),
    )
    res.json({ ok: true, itens, total: ids.length, offset })
  })

  // Sushi: lista de appids com manifesto no repo.
  app.get("/catalog/v1/sushi", async (req, res) => responder(null, req, res, "sushi"))

  // Listas alternativas de genero.
  app.get("/catalog/v1/genre", async (req, res) =>
    responder(null, req, res, "genre", String(req.query.lista || "__all")),
  )

  // Noticias (RSS agregado).
  app.get("/catalog/v1/news", async (req, res) => responder(null, req, res, "news"))

  // Indices de fixes.
  app.get("/catalog/v1/fixes", async (req, res) => responder(null, req, res, "fixes"))
  app.get("/catalog/v1/ryuu", async (req, res) => responder(null, req, res, "ryuu"))

  // Fontes Hydra: JSON completo de uma fonte (com uris).
  app.get("/catalog/v1/sources/:id/games", async (req, res) =>
    responder(null, req, res, "hydra", req.params.id),
  )

  // Sysinfo / meta / hltb por appid.
  app.get("/catalog/v1/sysinfo/:appid", async (req, res) => responder(null, req, res, "sysinfo", req.params.appid))
  app.get("/catalog/v1/meta/:appid", async (req, res) => responder(null, req, res, "meta", req.params.appid))
  app.get("/catalog/v1/hltb/:appid", async (req, res) => responder(null, req, res, "hltb", req.params.appid))

  // Items: tipo + arte por appid (em lote).
  app.get("/catalog/v1/items", async (req, res) => {
    const appids = String(req.query.appids || "")
      .split(",")
      .map((s) => s.trim())
      .filter((s) => /^\d{1,10}$/.test(s))
    if (!appids.length) return res.json({ ok: true, data: {} })
    const out = {}
    for (const appid of appids) {
      let data = getCached(`items:${appid}`)
      if (data === null) {
        data = await buscar(`items:${appid}`) // cold start: busca na fonte
      }
      if (data) out[appid] = data
    }
    res.json({ ok: true, data: out })
  })

  // Manifests: disponibilidade de manifesto por appid.
  // Endpoint de LOTE: ?appids=1,2,3 devolve todos em uma chamada, evitando
  // N handshakes TLS quando a loja prepara uma pagina inteira. O app pedia
  // um por jogo (24 chamadas) — agora e 1.
  app.get("/catalog/v1/manifests", async (req, res) => {
    const appids = String(req.query.appids || "")
      .split(",")
      .map((s) => s.trim())
      .filter((s) => /^\d{1,10}$/.test(s))
    if (!appids.length) return res.json({ ok: true, data: {} })
    const out = {}
    for (const appid of appids) {
      let data = getCached(`manifests:${appid}`)
      if (data === null) {
        data = await buscar(`manifests:${appid}`) // cold start
      }
      if (data) out[appid] = data
    }
    res.json({ ok: true, data: out })
  })
  app.get("/catalog/v1/manifests/:appid", async (req, res) =>
    responder(null, req, res, "manifests", req.params.appid),
  )

  // Search: busca no catalogo Hydra em cache (indice do servidor).
  app.get("/catalog/v1/search", (req, res) => {
    const q = String(req.query.q || "").trim().toLowerCase()
    if (!q) return res.json({ ok: true, itens: [] })
    // varre as fontes em cache, monta indice leve e filtra por titulo
    const itens = []
    const fontes = db
      .prepare("SELECT key, data FROM catalog_cache WHERE key LIKE 'hydra:%'")
      .all()
    for (const f of fontes) {
      let dados
      try {
        dados = JSON.parse(f.data)
      } catch {
        continue
      }
      const downloads = Array.isArray(dados.downloads) ? dados.downloads : []
      for (let i = 0; i < downloads.length; i++) {
        const d = downloads[i]
        if (!d?.title) continue
        if (String(d.title).toLowerCase().includes(q)) {
          itens.push({
            ref: `${f.key.replace("hydra:", "")}:${i}`,
            title: d.title,
            fileSize: String(d.fileSize || "").trim(),
            uploadDate: String(d.uploadDate || "").trim(),
          })
        }
      }
    }
    res.json({ ok: true, itens: itens.slice(0, 40) })
  })
}

// ---------- Warm-up (pre-aquecimento do cache no boot) ----------
// Busca os catalogos mais pesados em background assim que o servidor sobe.
// Assim a PRIMEIRA pessoa que abrir a loja ja encontra cache quente (nao paga
// o cold-start). Nada aqui bloqueia o boot nem a resposta — erros sao
// engolidos. Os catalagos que nao responderem no primeiro try sao buscados
// por demanda (cold-start) quando alguem pedir.
const WARM_KEYS = ["popular", "sushi", "news", "fixes", "ryuu-index"]

// True se ha cache e ele ainda esta dentro do TTL (nao precisa re-buscar).
function cacheFresco(key) {
  const ttl = CATALOG_TTL[key] ?? CATALOG_TTL[`${key.split(":")[0]}:`] ?? 0
  const row = db.prepare("SELECT at FROM catalog_cache WHERE key = ?").get(key)
  if (!row) return false
  return ttl === 0 || nowEpochS() - row.at < ttl
}

// Pre-aquece apenas os catalogos que estao AUSENTES ou VENCIDOS. No boot o
// cache persiste no SQLite; respeitar o TTL evita re-buscar (rede/CPU) o que
// ainda e valido, deixando o restart leve e a loja rapida de imediato.
function warmUpCatalog() {
  for (const key of WARM_KEYS) {
    if (cacheFresco(key)) {
      console.log(`[warmup] ${key}: cache valido (sem re-buscar)`)
      continue
    }
    fetchCatalogKey(key)
      .then((r) => {
        if (r) {
          gravarCache(key, r)
          console.log(`[warmup] ${key}: ${r.data?.completa?.length ?? r.data?.noticias?.length ?? r.data?.ids?.length ?? "?"} (${key === "popular" ? "jogos" : key === "news" ? "noticias" : key === "sushi" ? "appids" : "entradas"})`)
        } else {
          console.log(`[warmup] ${key}: fonte nao respondeu (fica p/ cold-start)`)
        }
      })
      .catch(() => {})
  }
}

module.exports = { registerCatalogRoutes, getCached, warmUpCatalog }
