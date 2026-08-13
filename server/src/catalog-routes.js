"use strict"

// Proxy de catalogo da loja: rotas /catalog/v1/*. O servidor busca a fonte
// externa uma vez por TTL, guarda em catalog_cache (PostgreSQL) e responde JSON
// pronto ao app. Tudo exige JWT valido.
//
// Privacidade: nenhuma chave paga do usuario (Hubcap, debrid, Steam) existe
// neste modulo nem nos dados que ele serve. Fontes que exigem credencial
// continuam sendo buscadas diretamente pelo app.

const { db, nowEpochS } = require("./db")
const { verifyToken, extractToken } = require("./jwt")
const { fetchCatalogKey, catalogKey, CATALOG_KEYS, CATALOG_TTL, cacheDesatualizado, fetchGenero, STEAMSPY_GENEROS } = require("./catalog-fetch")
const asyncHandler = require("./async-handler")

const MAX_PAGE_SIZE = 100
const MAX_BATCH_APPIDS = 100

function pageParam(value, fallback) {
  const n = Number(value)
  return Number.isSafeInteger(n) && n >= 0 ? n : fallback
}

function pageSize(value, fallback) {
  const n = Number(value)
  return Number.isSafeInteger(n) && n > 0 ? Math.min(n, MAX_PAGE_SIZE) : fallback
}

function parseAppids(value) {
  return [...new Set(String(value || "")
    .split(",")
    .map((s) => s.trim())
    .filter((s) => /^\d{1,10}$/.test(s)))].slice(0, MAX_BATCH_APPIDS)
}

// Extrai o Bearer token e valida. Devolve o sub (user id) ou null.
function requireAuth(req) {
  const token = extractToken(req)
  const v = verifyToken(token || "")
  return v.ok ? v.sub : null
}

// Grava o resultado de um fetch no cache PostgreSQL (atomico).
async function gravarCache(key, r) {
  await db.query(
    `INSERT INTO catalog_cache (key, data, at) VALUES ($1, $2, $3)
     ON CONFLICT (key) DO UPDATE SET data = excluded.data, at = excluded.at`,
    [key, JSON.stringify(r.data), r.at],
  )
}

// Le do cache (sem revalidar). Devolve o JSON ou null se nao ha entrada.
async function lerCache(key) {
  const row = (await db.query("SELECT data FROM catalog_cache WHERE key = $1", [key])).rows[0]
  return row ? JSON.parse(row.data) : null
}

// Dedupe em voo: quando varios usuarios pedem a MESMA chave sem cache ao
// mesmo tempo (ex.: todos abrindo o mesmo jogo novo), apenas 1 busca vai à
// fonte externa; os demais esperam a mesma promise. Protege a Steam de um
// pico de N requisicoes iguais.
const buscasEmVoo = new Map()

// Limite de concorrencia para buscas à FONTE EXTERNA. Sem isto, N usuarios
// pedindo jogos DIFERENTES sem cache disparavam N fetches simultaneos à
// Steam -> rate-limit + sobrecarga do notebook (medido: 200 -> 10s, 176
// falhas). Com o semaforo, no maximo MAX_CONCORRENCIA fetches rodam por vez;
// os demais esperam na fila e seguem um a um. Acha o pico sem travar.
const MAX_CONCORRENCIA = 8
let ativos = 0
const fila = []

async function comSemaforo(fn) {
  if (ativos < MAX_CONCORRENCIA) {
    ativos++
    try {
      return await fn()
    } finally {
      ativos--
      // libera o proximo da fila, se houver
      const proximo = fila.shift()
      if (proximo) proximo()
    }
  }
  // espera um slot
  await new Promise((resolve) => fila.push(resolve))
  return comSemaforo(fn)
}

// Busca da fonte externa e grava. Devolve { data } ou null se a fonte falhou.
// Cold-start: quando nao ha cache, a rota chama isto (espera) antes de
// responder — o 404 so aparece se a fonte externa tambem falhar.
async function buscar(key) {
  if (buscasEmVoo.has(key)) return buscasEmVoo.get(key)
  const promessa = comSemaforo(async () => {
    const r = await fetchCatalogKey(key).catch(() => null)
    if (r) await gravarCache(key, r)
    return r ? r.data : null
  })
  buscasEmVoo.set(key, promessa)
  try {
    return await promessa
  } finally {
    buscasEmVoo.delete(key)
  }
}

// Le do cache; se vencido, revalida em background (stale-while-revalidate) e
// devolve o que tem. Nunca bloqueia a resposta em rede externa lenta quando
// ja ha cache. Quando NAO ha cache, devolve null (o cold-start fica na rota,
// que chama `buscar` e espera a fonte externa).
async function getCached(key, includeMeta = false) {
  const ttl = CATALOG_TTL[key] ?? CATALOG_TTL[`${key.split(":")[0]}:`] ?? 0
  const row = (await db.query("SELECT data, at FROM catalog_cache WHERE key = $1", [key])).rows[0]
  if (!row) return null
  const data = JSON.parse(row.data)
  const agora = nowEpochS()
  // Formato antigo (campo novo entrou depois que a linha foi gravada): conta
  // como cache vazio para a rota rebuscar agora. Chaves sem TTL (sysinfo/meta)
  // nunca chegariam na revalidacao por tempo abaixo. Se a fonte externa estiver
  // fora, `responder` ainda cai no `lerCache` e serve o formato velho.
  if (cacheDesatualizado(key, data)) return null
  if (ttl > 0 && agora - row.at > ttl) {
    // revalida em background; erro de rede nao derruba a resposta
    buscar(key).catch(() => {})
  }
  return includeMeta ? { data, at: row.at } : data
}

// Resolve a key, le do cache (ou busca no cold start) e responde.
// key invalida -> 400; sem cache E fonte externa falhou -> 404.
async function responder(uid, req, res, tipo, id) {
  const key = catalogKey(tipo, id)
  if (!key) return res.status(400).json({ error: "key_invalida" })
  const cached = await getCached(key, true)
  let data = cached?.data ?? null
  if (data === null) {
    data = await buscar(key)
    // Fonte externa fora: serve o que houver em cache, mesmo em formato antigo
    // — dado incompleto ainda e melhor que 404 para a loja.
    if (data === null) data = await lerCache(key)
    if (data === null) return res.status(404).json({ error: "cache_vazio" })
  }
  // ETag: o app manda If-None-Match na proxima vez; se nada mudou (mesmo
  // timestamp), devolve 304 (0 bytes) em vez de re-baixar o JSON inteiro.
  const etag = `"${cached?.at ?? await getCacheAt(key)}"`
  if (req.headers["if-none-match"] === etag) return res.status(304).end()
  res.set("ETag", etag)
  return res.json({ ok: true, data })
}

// Lê o `at` (timestamp) de uma chave de cache — usado como ETag fraco.
async function getCacheAt(key) {
  const row = (await db.query("SELECT at FROM catalog_cache WHERE key = $1", [key])).rows[0]
  return row ? row.at : 0
}

function registerCatalogRoutes(app) {
  const get = (route, handler) => app.get(route, asyncHandler(handler))

  // Todos os endpoints exigem JWT (Bearer).
  app.use("/catalog/v1", (req, res, next) => {
    if (!requireAuth(req)) return res.status(401).json({ error: "nao_autenticado" })
    next()
  })

  // Em alta / populares (SteamSpy). Retorna fatia paginada + total.
  get("/catalog/v1/popular", async (req, res) => {
    const cached = await getCached("popular", true)
    let data = cached?.data ?? null
    if (data === null) {
      data = await buscar("popular")
      if (data === null) return res.status(404).json({ error: "cache_vazio" })
    }
    const etag = `"${cached?.at ?? await getCacheAt("popular")}"`
    if (req.headers["if-none-match"] === etag) return res.status(304).end()
    res.set("ETag", etag)
    const completa = Array.isArray(data.completa) ? data.completa : []
    const limite = pageSize(req.query.limite, 40)
    const offset = pageParam(req.query.offset, 0)
    res.json({ ok: true, itens: completa.slice(offset, offset + limite), total: completa.length, offset })
  })

  // Catálogo completo: ~100.000+ jogos da Steam coletados via SteamSpy por
  // gênero (com NOME real), paginados como a Steam/Hydra. Enquanto a coleta
  // ainda não terminou, serve o que já tem. Arte via items sob demanda.
  get("/catalog/v1/catalog", async (req, res) => {
    const limite = pageSize(req.query.limite, 24)
    const offset = pageParam(req.query.offset, 0)
    const cached = await getCached("catalogo_completo", true)
    let data = cached?.data ?? null
    if (data === null) {
      // coleta ainda rodando ou vazia — dispara e serve vazio por enquanto
      precarregarCatalogoCompleto()
      return res.json({ ok: true, itens: [], total: 0, offset, coletando: true })
    }
    const completa = Array.isArray(data.completa) ? data.completa : []
    const etag = `"${cached?.at ?? await getCacheAt("catalogo_completo")}"`
    if (req.headers["if-none-match"] === etag) return res.status(304).end()
    res.set("ETag", etag)
    const fatia = completa.slice(offset, offset + limite)
    // capa sempre vem da CDN da Steam (não precisa do items). O items (heroi/
    // capa retrato) é só um enriquecimento — se não estiver em cache, deixa
    // vazio em vez de buscar na Steam na hora (evita rate-limit/timeout).
    const itens = await Promise.all(fatia.map(async (g) => {
      const item = await getCached(`items:${g.appid}`)
      return {
        appid: String(g.appid),
        title: g.title || String(g.appid),
        cover: `https://cdn.akamai.steamstatic.com/steam/apps/${g.appid}/header.jpg`,
        heroi: item?.heroi || "",
        capa: item?.capa || "",
      }
    }))
    res.json({ ok: true, itens, total: completa.length, offset })
  })

  // Steam250: catálogo com nome real (~890 jogos: top250, mais jogados,
  // hidden gems, do ano). Fonte igual a do Hydra. Paginado com arte via items.
  get("/catalog/v1/steam250", async (req, res) => {
    const limite = pageSize(req.query.limite, 24)
    const offset = pageParam(req.query.offset, 0)
    const cached = await getCached("steam250", true)
    let data = cached?.data ?? null
    if (data === null) {
      data = await buscar("steam250")
      if (data === null) return res.status(404).json({ error: "cache_vazio" })
    }
    const completa = Array.isArray(data.completa) ? data.completa : []
    const etag = `"${cached?.at ?? await getCacheAt("steam250")}"`
    if (req.headers["if-none-match"] === etag) return res.status(304).end()
    res.set("ETag", etag)
    const fatia = completa.slice(offset, offset + limite)
    // arte (capa/heroi) via items, sob demanda e cacheado
    const comArte = await Promise.all(
      fatia.map(async (g) => {
        const item = (await getCached(`items:${g.appid}`)) ?? (await buscar(`items:${g.appid}`))
        return {
          appid: String(g.appid),
          title: g.title || String(g.appid),
          cover: `https://cdn.akamai.steamstatic.com/steam/apps/${g.appid}/header.jpg`,
          heroi: item?.heroi || "",
          capa: item?.capa || "",
        }
      }),
    )
    res.json({ ok: true, itens: comArte, total: completa.length, offset })
  })

  // Sushi: lista de appids com manifesto no repo.
  get("/catalog/v1/sushi", async (req, res) => responder(null, req, res, "sushi"))

  // Listas alternativas de genero.
  get("/catalog/v1/genre", async (req, res) =>
    responder(null, req, res, "genre", String(req.query.lista || "__all")),
  )

  // Noticias (RSS agregado).
  get("/catalog/v1/news", async (req, res) => responder(null, req, res, "news"))

  // Indices de fixes.
  get("/catalog/v1/fixes", async (req, res) => responder(null, req, res, "fixes"))
  get("/catalog/v1/ryuu", async (req, res) => responder(null, req, res, "ryuu"))

  // Fontes Hydra: JSON completo de uma fonte (com uris).
  get("/catalog/v1/sources/:id/games", async (req, res) =>
    responder(null, req, res, "hydra", req.params.id),
  )

  // Sysinfo / meta / hltb por appid.
  get("/catalog/v1/sysinfo/:appid", async (req, res) => responder(null, req, res, "sysinfo", req.params.appid))
  // Stats: estatisticas agregadas (dev, owners, ccu, reviews, preco) do SteamSpy.
  get("/catalog/v1/stats/:appid", async (req, res) => responder(null, req, res, "stats", req.params.appid))

  // Reviews da comunidade. O servidor e a fonte de verdade (nao depende da
  // Steam). GET devolve as reviews do jogo; POST adiciona uma (autenticado).
  get("/catalog/v1/reviews/:appid", async (req, res) => {
    const appid = String(req.params.appid || "")
    if (!/^\d{1,10}$/.test(appid)) return res.status(400).json({ error: "appid_invalido" })
    const rows = (await db.query(
        `SELECT r.id, r.text, r.positive, r.hours, r.created_at, p.username
         FROM user_reviews r JOIN profiles p ON p.id = r.user_id
         WHERE r.appid = $1 ORDER BY r.created_at DESC LIMIT 100`,
        [appid],
      )).rows
    res.json({ ok: true, reviews: rows })
  })

  app.post("/catalog/v1/reviews/:appid", asyncHandler(async (req, res) => {
    const uid = requireAuth(req)
    if (!uid) return res.status(401).json({ error: "nao_autenticado" })
    const appid = String(req.params.appid || "")
    if (!/^\d{1,10}$/.test(appid)) return res.status(400).json({ error: "appid_invalido" })
    const { text, positive, hours } = req.body || {}
    const txt = String(text || "").trim().slice(0, 4000)
    if (!txt) return res.status(400).json({ error: "texto_vazio" })
    await db.query(
      "INSERT INTO user_reviews (user_id, appid, text, positive, hours) VALUES ($1, $2, $3, $4, $5)",
      [uid, appid, txt, positive === false ? 0 : 1, Math.max(0, Math.min(Number(hours) || 0, 999999))],
    )
    res.json({ ok: true })
  }))
  get("/catalog/v1/meta/:appid", async (req, res) => responder(null, req, res, "meta", req.params.appid))
  get("/catalog/v1/hltb/:appid", async (req, res) => responder(null, req, res, "hltb", req.params.appid))

  // Items: tipo + arte por appid (em lote).
  get("/catalog/v1/items", async (req, res) => {
    const appids = parseAppids(req.query.appids)
    if (!appids.length) return res.json({ ok: true, data: {} })
    const out = {}
    for (const appid of appids) {
      let data = await getCached(`items:${appid}`)
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
  get("/catalog/v1/manifests", async (req, res) => {
    const appids = parseAppids(req.query.appids)
    if (!appids.length) return res.json({ ok: true, data: {} })
    // Busca em PARALELO: em série, 40 appids no cold start eram 40 round-trips
    // sequenciais ao provedor (3 HEADs cada) — a busca da loja travava. Cada
    // appid pode ir junto; o `buscar` já dedupeia em voo e o semaforo limita a
    // concorrencia externa.
    const out = {}
    await Promise.all(
      appids.map(async (appid) => {
        let data = await getCached(`manifests:${appid}`)
        if (data === null) {
          data = await buscar(`manifests:${appid}`) // cold start
        }
        if (data) out[appid] = data
      }),
    )
    res.json({ ok: true, data: out })
  })
  get("/catalog/v1/manifests/:appid", async (req, res) =>
    responder(null, req, res, "manifests", req.params.appid),
  )

  // Search: busca no catalogo Hydra em cache (indice do servidor).
  get("/catalog/v1/search", async (req, res) => {
    const q = String(req.query.q || "").trim().toLowerCase()
    if (!q) return res.json({ ok: true, itens: [] })
    // Busca no catálogo completo (85k jogos da Steam, com appid real + nome).
    // Este é o caminho da loja: resultados com capa e que abrem a tela rica.
    const data = await getCached("catalogo_completo")
    const completa = Array.isArray(data?.completa) ? data.completa : []
    // Ranking por relevancia (nao substring na ordem da colecao):
    //   0) prefixo exato ("cyber" bate em "Cyberpunk 2077" primeiro)
    //   1) limite de palavra ("punk" bate em "Cyberpunk" via "Cyber"+"punk")
    //   2) substring generica (ultimo recurso)
    // Desempate: titulo mais curto vem antes (mais provavelmente o jogo certo).
    const candidatos = []
    for (const g of completa) {
      if (!g?.title || candidatos.length >= 60) continue
      const t = String(g.title).toLowerCase()
      if (!t.includes(q)) continue
      // nota: 0 prefixo exato | 1 prefixo de palavra | 2 substring generica
      let nota = 2
      if (t.startsWith(q)) nota = 0
      else if (t.split(/[^a-z0-9]+/).some((palavra) => palavra.startsWith(q))) nota = 1
      // Desempate dentro da mesma nota: titulo com MENOS texto alem da busca.
      // O ganho real do ranking e prefixo > palavra > substring.
      const sufixo = t.slice(q.length).trim().split(/[^a-z0-9]+/).filter(Boolean).length
      candidatos.push({ appid: String(g.appid), title: g.title, nota, sufixo })
    }
    candidatos.sort((a, b) => a.nota - b.nota || a.sufixo - b.sufixo)
    const alvo = candidatos.slice(0, 40)
    // Filtra DLCs/demos/trilhas sonoras usando o tipo do IStoreBrowseService
    // (0 = jogo). Busca os items em paralelo (dedupe/cache do buscar) e só
    // mantém os que são jogos de verdade.
    const comTipo = await Promise.all(
      alvo.map(async (g) => {
        const item = (await getCached(`items:${g.appid}`)) ?? (await buscar(`items:${g.appid}`))
        return { ...g, tipo: item?.tipo }
      }),
    )
    const itens = comTipo
      .filter((g) => g.tipo === undefined || g.tipo === 0) // sem tipo = mantém; tipo 0 = jogo
      .map((g) => ({
        appid: g.appid,
        title: g.title,
        cover: `https://cdn.akamai.steamstatic.com/steam/apps/${g.appid}/header.jpg`,
      }))
    res.json({ ok: true, itens })
  })
}

// ---------- Warm-up (pre-aquecimento do cache no boot) ----------
// Busca os catalogos mais pesados em background assim que o servidor sobe.
// Assim a PRIMEIRA pessoa que abrir a loja ja encontra cache quente (nao paga
// o cold-start). Nada aqui bloqueia o boot nem a resposta — erros sao
// engolidos. Os catalagos que nao responderem no primeiro try sao buscados
// por demanda (cold-start) quando alguem pedir.
const WARM_KEYS = ["popular", "steam250", "sushi", "news", "fixes", "ryuu-index"]

// True se ha cache e ele ainda esta dentro do TTL (nao precisa re-buscar).
async function cacheFresco(key) {
  const ttl = CATALOG_TTL[key] ?? CATALOG_TTL[`${key.split(":")[0]}:`] ?? 0
  const row = (await db.query("SELECT at FROM catalog_cache WHERE key = $1", [key])).rows[0]
  if (!row) return false
  return ttl === 0 || nowEpochS() - row.at < ttl
}

// Pre-aquece apenas os catalogos que estao AUSENTES ou VENCIDOS. No boot o
// cache persiste no PostgreSQL; respeitar o TTL evita re-buscar (rede/CPU) o que
// ainda e valido, deixando o restart leve e a loja rapida de imediato.
async function warmUpCatalog() {
  for (const key of WARM_KEYS) {
    if (await cacheFresco(key)) {
      console.log(`[warmup] ${key}: cache valido (sem re-buscar)`)
      continue
    }
    fetchCatalogKey(key)
      .then((r) => {
        if (r) {
          return gravarCache(key, r).then(() => {
            console.log(`[warmup] ${key}: ${r.data?.completa?.length ?? r.data?.noticias?.length ?? r.data?.ids?.length ?? "?"} (${key === "popular" ? "jogos" : key === "news" ? "noticias" : key === "sushi" ? "appids" : "entradas"})`)
          })
        } else {
          console.log(`[warmup] ${key}: fonte nao respondeu (fica p/ cold-start)`)
        }
      })
      .catch(() => {})
  }
}

// Coleta o catálogo COMPLETO da Steam via SteamSpy por gênero: cada gênero
// lista dezenas de milhares de jogos com NOME real. Deduplica por appid e
// grava no PostgreSQL como 'catalogo_completo'. ~100.000+ jogos no total — como
// o Hydra coleta no servidor. Roda em background no boot; erros engolidos.
let coletando = false
function precarregarCatalogoCompleto() {
  if (coletando) return
  coletando = true
  console.log("[precarregar] coletando catalogo completo via SteamSpy (generos)...")
  let todos = []
  let i = 0
  async function proximoGenero() {
    if (i >= STEAMSPY_GENEROS.length) {
      // dedupe + grava
      const unicos = [...new Map(todos.map((g) => [g.appid, g])).values()]
      await gravarCache("catalogo_completo", {
        data: { completa: unicos },
        at: nowEpochS(),
      })
      console.log(`[precarregar] catalogo completo pronto: ${unicos.length} jogos`)
      coletando = false
      return
    }
    const genero = STEAMSPY_GENEROS[i++]
    const jogos = await fetchGenero(genero).catch(() => [])
    if (jogos.length) {
      todos = todos.concat(jogos)
      console.log(`[precarregar] genero ${genero}: +${jogos.length} (total ${todos.length})`)
    }
    // pausa leve para nao sobrecarregar o SteamSpy
    setTimeout(proximoGenero, 300)
  }
  proximoGenero()
}

module.exports = { registerCatalogRoutes, getCached, warmUpCatalog, precarregarCatalogoCompleto }
