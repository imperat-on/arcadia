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

// Le do cache; se vencido, revalida em background (stale-while-revalidate) e
// devolve o que tem. Nunca bloqueia a resposta em rede externa lenta.
// key e validada por quem chama (catalogKey) — aqui so le/grava no SQLite.
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
        if (r) {
          db.prepare("INSERT OR REPLACE INTO catalog_cache (key, data, at) VALUES (?,?,?)").run(
            key,
            JSON.stringify(r.data),
            r.at,
          )
        }
      })
      .catch(() => {})
  }
  return data
}

// Resolve a key, le do cache e responde. Se a key e invalida -> 400; se nao
// ha cache -> 404.
function responder(uid, req, res, tipo, id) {
  const key = catalogKey(tipo, id)
  if (!key) return res.status(400).json({ error: "key_invalida" })
  const data = getCached(key)
  if (data === null) return res.status(404).json({ error: "cache_vazio" })
  return res.json({ ok: true, data })
}

function registerCatalogRoutes(app) {
  // Todos os endpoints exigem JWT (Bearer).
  app.use("/catalog/v1", (req, res, next) => {
    if (!requireAuth(req)) return res.status(401).json({ error: "nao_autenticado" })
    next()
  })

  // Em alta / populares (SteamSpy). Retorna fatia paginada + total.
  app.get("/catalog/v1/popular", (req, res) => {
    const data = getCached("popular")
    if (data === null) return res.status(404).json({ error: "cache_vazio" })
    const completa = Array.isArray(data.completa) ? data.completa : []
    const limite = Math.max(1, Number(req.query.limite) || 40)
    const offset = Math.max(0, Number(req.query.offset) || 0)
    res.json({ ok: true, itens: completa.slice(offset, offset + limite), total: completa.length, offset })
  })

  // Sushi: lista de appids com manifesto no repo.
  app.get("/catalog/v1/sushi", (req, res) => responder(null, req, res, "sushi"))

  // Listas alternativas de genero.
  app.get("/catalog/v1/genre", (req, res) =>
    responder(null, req, res, "genre", String(req.query.lista || "__all")),
  )

  // Noticias (RSS agregado).
  app.get("/catalog/v1/news", (req, res) => responder(null, req, res, "news"))

  // Indices de fixes.
  app.get("/catalog/v1/fixes", (req, res) => responder(null, req, res, "fixes"))
  app.get("/catalog/v1/ryuu", (req, res) => responder(null, req, res, "ryuu"))

  // Fontes Hydra: JSON completo de uma fonte (com uris).
  app.get("/catalog/v1/sources/:id/games", (req, res) =>
    responder(null, req, res, "hydra", req.params.id),
  )

  // Sysinfo / meta / hltb por appid.
  app.get("/catalog/v1/sysinfo/:appid", (req, res) => responder(null, req, res, "sysinfo", req.params.appid))
  app.get("/catalog/v1/meta/:appid", (req, res) => responder(null, req, res, "meta", req.params.appid))
  app.get("/catalog/v1/hltb/:appid", (req, res) => responder(null, req, res, "hltb", req.params.appid))

  // Items: tipo + arte por appid (em lote).
  app.get("/catalog/v1/items", (req, res) => {
    const appids = String(req.query.appids || "")
      .split(",")
      .map((s) => s.trim())
      .filter((s) => /^\d{1,10}$/.test(s))
    if (!appids.length) return res.json({ ok: true, data: {} })
    const out = {}
    for (const appid of appids) {
      const data = getCached(`items:${appid}`)
      if (data) out[appid] = data
    }
    res.json({ ok: true, data: out })
  })

  // Manifests: disponibilidade de manifesto por appid.
  app.get("/catalog/v1/manifests/:appid", (req, res) =>
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

module.exports = { registerCatalogRoutes, getCached }
