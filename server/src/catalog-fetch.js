"use strict"

// Busca de catalogos da loja junto a fontes externas (Hydra, SteamSpy, Steam,
// sushi, provedores de manifesto). O servidor e o proxy: baixa uma vez por TTL
// e guarda em catalog_cache. Funcoes de rede puras — recebem uma key e
// devolvem { data, at } ou null.
//
// SEGURANCA / PRIVACIDADE: nenhuma chave paga do usuario (Hubcap, Real-Debrid,
// TorBox, AllDebrid, Premiumize, Steam key) existe ou sai daqui. Fontes que
// exigem credencial continuam sendo buscadas diretamente pelo app. So catalogos
// publicos passam por este modulo.

const { nowEpochS } = require("./db")

// Allowlist de keys de catalogo. Valida entradas de cache: qualquer key fora
// daqui e rejeitada (nada de path traversal / key arbitraria). As chaves com
// prefixo (hydra:, sysinfo:, meta:, hltb:, items:, manifests:, genre:) sao
// validadas por prefixo + restricao do id, nao listadas uma a uma aqui.
const CATALOG_KEYS = [
  "popular",
  "steam250",
  "sushi",
  "news",
  "fixes",
  "ryuu-index",
]

// TTL por prefixo de chave (segundos). Espelha o TTL do app para o mesmo dado.
// Sysinfo e meta nao tem validade (como no app: valem para sempre).
const CATALOG_TTL = {
  popular: 6 * 60 * 60, // 6h
  steam250: 6 * 60 * 60, // 6h
  sushi: 6 * 60 * 60, // 6h
  news: 30 * 60, // 30min
  fixes: 6 * 60 * 60, // 6h
  "ryuu-index": 6 * 60 * 60, // 6h
  "hydra:": 7 * 24 * 60 * 60, // 7d (ETag revalida; usado pelo app)
  "genre:": 12 * 60 * 60, // 12h
  "sysinfo:": 0, // sem validade
  "meta:": 0, // sem validade
  "hltb:": 30 * 24 * 60 * 60, // 30d
  "items:": 7 * 24 * 60 * 60, // 7d
  "manifests:": 7 * 24 * 60 * 60, // 7d
}

// Valida o formato de um id por prefixo. Permite so o que faz sentido na key.
function idValido(prefixo, id) {
  if (prefixo === "hydra:") return /^[0-9a-f]{12}$/.test(id) // sha256.slice(0,12)
  if (prefixo === "sysinfo:" || prefixo === "meta:" || prefixo === "hltb:")
    return /^\d{1,10}$/.test(id) // appid numerico
  if (prefixo === "items:" || prefixo === "manifests:") return /^\d{1,10}$/.test(id)
  if (prefixo === "genre:") return /^[\w_-]{1,40}$/.test(id) // nome de lista
  return false
}

// Monta a key canonica a partir de tipo + id (usado pelas rotas). Devolve a
// key da allowlist ou null se o tipo/id nao e reconhecido.
function catalogKey(tipo, id) {
  if (tipo === "popular") return CATALOG_KEYS.includes("popular") ? "popular" : null
  if (tipo === "steam250") return CATALOG_KEYS.includes("steam250") ? "steam250" : null
  if (tipo === "sushi") return CATALOG_KEYS.includes("sushi") ? "sushi" : null
  if (tipo === "news") return CATALOG_KEYS.includes("news") ? "news" : null
  if (tipo === "fixes") return CATALOG_KEYS.includes("fixes") ? "fixes" : null
  if (tipo === "ryuu") return CATALOG_KEYS.includes("ryuu-index") ? "ryuu-index" : null
  if (tipo === "genre") {
    // Normaliza "all" -> "__all": o app manda ?lista=all (o catalogo completo
    // do Em alta), mas a chave em cache usa prefixo __. Sem isto, o servidor
    // devolvia 400 key_invalida e o app caia no fallback que buscava a
    // SteamSpy viva a cada troca de pagina (~15s de espera).
    const nome = id === "all" ? "__all" : id || "__all"
    const k = `genre:${nome}`
    // Aceita se o id eh valido E ha TTL para o prefixo genre: (a chave exata
    // `genre:__all` nao esta em CATALOG_TTL — so o prefixo `genre:`).
    return idValido("genre:", nome) && "genre:" in CATALOG_TTL ? k : null
  }
  if (tipo === "hydra") {
    const k = `hydra:${id || ""}`
    return idValido("hydra:", id) ? k : null
  }
  if (tipo === "sysinfo" || tipo === "meta" || tipo === "hltb") {
    const k = `${tipo}:${id || ""}`
    return idValido(`${tipo}:`, id) ? k : null
  }
  if (tipo === "items" || tipo === "manifests") {
    const k = `${tipo}:${id || ""}`
    return idValido(`${tipo}:`, id) ? k : null
  }
  return null
}

// Um fetch generico com timeout e User-Agent de navegador. Devolve Response.
async function http(url, opts = {}) {
  const ctrl = new AbortController()
  const to = setTimeout(() => ctrl.abort(), opts.timeoutMs || 25000)
  try {
    return await fetch(url, {
      ...opts,
      headers: { "User-Agent": "Mozilla/5.0 (X11; Linux x86_64)", ...(opts.headers || {}) },
      signal: ctrl.signal,
    })
  } finally {
    clearTimeout(to)
  }
}

// ---------- Fontes Hydra (catalogos de download) ----------
// Formato Hydra: { name, downloads: [{ title, uris, fileSize, uploadDate }] }.
// A URL real da fonte vem do registro do app (sources.json por conta); o id
// aqui e o sha256(url).slice(0,12). Este modulo nao conhece as URLs — quem as
// conhece e o app. Por isso o fetch de uma fonte especifica precisa da URL.
// O servidor NAO guarda a URL das fontes do usuario (nao ha coluna; privacidade).

// ---------- "Em alta" (SteamSpy) ----------
// SteamSpy devolve um OBJETO indexado por appid. Normaliza para array com
// capa, igual o app faz (buscarPopular em steamstore.js).
const STEAMSPY = "https://steamspy.com/api.php"

async function fetchPopular() {
  const r = await http(`${STEAMSPY}?request=top100in2weeks`, { timeoutMs: 15000 })
  if (!r.ok) return null
  const data = await r.json()
  const completa = Object.values(data)
    .map((g) => ({
      appid: String(g.appid || ""),
      title: g.name || "",
      cover: `https://cdn.akamai.steamstatic.com/steam/apps/${g.appid}/header.jpg`,
      manifest: false,
    }))
    .filter((g) => g.appid && g.title)
    // SteamSpy reordena por appid; o "Em alta" deve vir por jogadores ativos
    .sort((a, b) => (Number(data[b.appid]?.ccu) || 0) - (Number(data[a.appid]?.ccu) || 0))
  return { data: { completa }, at: nowEpochS() }
}

// ---------- Steam250 (catálogo com nome real) ----------
// Fonte do catalogo do Hydra: steam250.com lista os melhores jogos da Steam
// (top250, mais jogados, hidden gems, do ano) com NOME e appid — sem precisar
// buscar appdetails por jogo. Combina 4 paginas em ~890 jogos unicos.
const STEAM250_PATHS = ["/top250", "/most_played", "/hidden_gems", `/${new Date().getFullYear()}`]

function steam250Decode(s) {
  return String(s || "")
    .replace(/&#x20;/g, " ")
    .replace(/&#x3A;/g, ":")
    .replace(/&#x27;/g, "'")
    .replace(/&amp;/g, "&")
    .replace(/&#(\d+);/g, (_, d) => String.fromCodePoint(Number(d)))
    .replace(/&#x([0-9a-f]+);/gi, (_, h) => String.fromCodePoint(parseInt(h, 16)))
}

async function fetchSteam250() {
  const resultados = await Promise.all(
    STEAM250_PATHS.map(async (path) => {
      try {
        const r = await http(`https://steam250.com${path}`, { timeoutMs: 15000 })
        if (!r.ok) return []
        const t = await r.text()
        const games = []
        for (const m of t.matchAll(/data-title=([^ >]+)[^>]*href="?[^"]*\/app\/(\d+)/g)) {
          games.push({ appid: m[2], title: steam250Decode(m[1]) })
        }
        return games
      } catch {
        return []
      }
    }),
  )
  // dedupe por appid, preservando a ordem
  const unicos = [...new Map(resultados.flat().map((g) => [g.appid, g])).values()]
  return { data: { completa: unicos }, at: nowEpochS() }
}

// ---------- Sushi (repo de manifestos) ----------
const SUSHI_TREE =
  "https://api.github.com/repos/sushi-dev55-alt/sushitools-games-repo-alt/git/trees/main"

async function fetchSushi() {
  const r = await http(SUSHI_TREE, {
    headers: { accept: "application/vnd.github+json" },
    timeoutMs: 25000,
  })
  if (!r.ok) return null
  const d = await r.json()
  // Arvore truncada devolveria indice incompleto — melhor nao cachear.
  if (d.truncated) return null
  const ids = []
  for (const n of d.tree || []) {
    const m = /^(\d+)\.zip$/.exec(n.path || "")
    if (m) ids.push(m[1])
  }
  return { data: { ids }, at: nowEpochS() }
}

// ---------- Noticias (RSS) ----------
// Mesmo agregado do app (app/electron/news.js): 6 feeds PT/EN, parse por
// regex, filtro anti-anuncio (oferta/hardware/dispositivo). Sem chave de API.
const NEWS_FEEDS = [
  { source: "Eurogamer", url: "https://www.eurogamer.pt/feed" },
  { source: "Nintendo Blast", url: "https://www.nintendoblast.com.br/feeds/posts/default?alt=rss" },
  { source: "GameSpot", url: "https://www.gamespot.com/feeds/game-news/" },
  { source: "Rock Paper Shotgun", url: "https://www.rockpapershotgun.com/feed" },
  { source: "Push Square", url: "https://www.pushsquare.com/feeds/latest" },
  { source: "PC Gamer", url: "https://www.pcgamer.com/rss/" },
]

const BLOCK_URL_NEWS =
  /\/(descontos|ofertas|deals|promo(?:cao|coes)?|hardware|reviews\/hardware|perifericos|celular|smartphone)\b/i
const BLOCK_OFERTA_NEWS =
  /(cai de pre|menor pre|melhor pre|mais barat|desconto|em oferta|black friday|cupom|\bdeal\b|% off|por (?:apenas|cerca de|r\$))/i
const BLOCK_HW_NEWS =
  /\b(gpu|cpu|ssd|hd externo|placa de v[íi]deo|placa-m[ãa]e|monitor|headset|fone|teclado|gabinete|cooler|fonte de alimenta|notebook|smartphone|celular|processador|cadeira gamer|power ?bank|roteador|smart ?tv|geladeira|carregador|rtx|gtx|geforce|radeon|ryzen|intel)(?:es|s)?\b/i
const BLOCK_DISP_NEWS =
  /(steam deck|rog ally|legion go|legion c\d|msi claw|\bhandheld\b|console port[áa]til|ayn odin|retroid|samsung galaxy|galaxy s\d|galaxy z|iphone|ipad|macbook|pixel \d)/i

function ehNoticiaJogo(n) {
  if (BLOCK_URL_NEWS.test(n.url || "")) return false
  if (BLOCK_OFERTA_NEWS.test(n.title || "")) return false
  if (BLOCK_HW_NEWS.test(n.title || "")) return false
  if (BLOCK_DISP_NEWS.test(n.title || "")) return false
  return true
}

function hashId(s) {
  let h = 5381
  for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) | 0
  return "news_" + (h >>> 0).toString(36)
}

function decodificarNum(s) {
  return String(s || "")
    .replace(/&#x([0-9a-f]+);/gi, (_, h) => String.fromCodePoint(parseInt(h, 16)))
    .replace(/&#(\d+);/g, (_, d) => String.fromCodePoint(parseInt(d, 10)))
}

function limparTexto(s) {
  let t = decodificarNum(String(s || ""))
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;|&apos;/g, "'")
    .replace(/&nbsp;/g, " ")
  t = t.replace(/<[^>]+>/g, " ").replace(/&amp;/g, "&")
  return t.replace(/\s+/g, " ").trim()
}

function pegar(bloco, tag) {
  const m = bloco.match(new RegExp(`<${tag}[^>]*>([\\s\\S]*?)</${tag}>`, "i"))
  if (!m) return ""
  return m[1]
    .replace(/^<!\[CDATA\[/, "")
    .replace(/\]\]>$/, "")
    .trim()
}

function melhorarImagem(url) {
  if (!url) return url
  let u = url.replace(/&amp;/g, "&")
  if (/blogger\.googleusercontent\.com|bp\.blogspot\.com/.test(u)) return u.replace(/\/(s\d+(?:-[a-z]\d+)*(?:-c)?)\//i, "/s0/")
  if (/images\.pushsquare\.com/.test(u)) return u.replace(/\/(small|medium|thumb)\.(jpg|png|webp)/i, "/large.$2")
  if (/gamespot\.com/.test(u)) return u.includes("?w=") ? u.replace(/\?w=\d+/, "?w=1280") : u
  if (/gnwcdn\.com/.test(u)) return u.replace(/([?&]width=)\d+/, "$11280").replace(/([?&]quality=)\d+/, "$190")
  return u
}

function pegarImagem(bloco) {
  const attr = bloco.match(/<(?:media:content|media:thumbnail|enclosure)[^>]*\burl="([^"]+)"/i)
  if (attr) return melhorarImagem(attr[1])
  const img = bloco.match(/<img[^>]*\bsrc="([^"]+)"/i)
  if (img) return melhorarImagem(img[1])
  const anyUrl = bloco.match(/https?:\/\/[^"'\s<>]+\.(?:jpg|jpeg|png|webp)/i)
  return anyUrl ? melhorarImagem(anyUrl[0]) : ""
}

async function buscarFeed(feed) {
  const r = await http(feed.url, { timeoutMs: 8000 })
  if (!r.ok) return []
  const xml = await r.text()
  const itens = xml.match(/<item[\s>][\s\S]*?<\/item>/gi) || []
  return itens.map((bloco) => {
    const link = pegar(bloco, "link") || pegar(bloco, "guid")
    const data = pegar(bloco, "pubDate") || pegar(bloco, "dc:date")
    return {
      id: hashId(link),
      title: limparTexto(pegar(bloco, "title")),
      summary: limparTexto(pegar(bloco, "description")).slice(0, 280),
      source: feed.source,
      url: link,
      image: pegarImagem(bloco),
      date: data ? new Date(data).toISOString() : "",
    }
  })
}

async function fetchNews() {
  const resultados = await Promise.allSettled(NEWS_FEEDS.map(buscarFeed))
  const itens = []
  for (const r of resultados) {
    if (r.status === "fulfilled") for (const n of r.value) if (ehNoticiaJogo(n)) itens.push(n)
  }
  // Ordena por data (mais recente primeiro), corta em 60
  itens.sort((a, b) => (a.date < b.date ? 1 : -1))
  return { data: { noticias: itens.slice(0, 60) }, at: nowEpochS() }
}

// ---------- Indices de fixes ----------
const FIXES_INDEX_URL = "https://index.luatools.work/fixes-index.json"
const RYUU_CATALOG_URL = "https://generator.ryuu.lol/files/fixes.json"

async function fetchFixes() {
  const r = await http(FIXES_INDEX_URL, { timeoutMs: 20000 })
  if (!r.ok) return null
  const data = await r.json()
  return { data, at: nowEpochS() }
}

async function fetchRyuuIndex() {
  const r = await http(RYUU_CATALOG_URL, { timeoutMs: 20000 })
  if (!r.ok) return null
  const data = await r.json()
  return { data, at: nowEpochS() }
}

// ---------- Sysinfo (requisitos de sistema, Steam) ----------
// Igual buildSysinfo do main.js: appdetails devolve pc_requirements.
const APPDETAILS = "https://store.steampowered.com/api/appdetails"

async function fetchSysinfo(appid) {
  const r = await http(`${APPDETAILS}?appids=${appid}&l=english`, { timeoutMs: 20000 })
  if (!r.ok) return null
  const j = await r.json()
  const data = j?.[String(appid)]?.data
  if (!data) return null
  const reqs = data.pc_requirements
  const info = {
    appid: String(appid),
    req_min: (reqs && !Array.isArray(reqs) && reqs.minimum) || "",
    req_rec: (reqs && !Array.isArray(reqs) && reqs.recommended) || "",
    short_description: data.short_description || "",
    header: data.header_image || "",
    background: data.background_raw || data.background || "",
  }
  return { data: info, at: nowEpochS() }
}

// ---------- Meta (metadados de jogo: nome, genero, dev) ----------
// O app usa o indexer Python (index.py) para meta_cache. Aqui o servidor
// agrega via Steam appdetails + SteamSpy. Nao usa SteamGridDB (key do user).
async function fetchMeta(appid) {
  const r = await http(`${APPDETAILS}?appids=${appid}&l=english`, { timeoutMs: 20000 })
  if (!r.ok) return null
  const j = await r.json()
  const data = j?.[String(appid)]?.data
  if (!data) return null
  return {
    data: {
      appid: String(appid),
      name: data.name || "",
      developers: data.developers || [],
      publishers: data.publishers || [],
      genre: (data.genres || []).map((g) => g.description).join(", "),
      release_date: data.release_date?.date || "",
    },
    at: nowEpochS(),
  }
}

// ---------- HowLongToBeat ----------
// Placeholder: o app faz scrape do bundle _next (hltb.js). Aqui usamos o
// endpoint publico da comunidade quando disponivel; senao, o app mantem o
// cache local. Por enquanto devolve null (nao e proxyado pelo servidor).
async function fetchHltb(appid) {
  return null
}

// ---------- Items (tipo + arte por appid, IStoreBrowseService) ----------
const ITEMS_URL = "https://api.steampowered.com/IStoreBrowseService/GetItems/v1/"

async function fetchItems(appid) {
  const input = {
    ids: [{ appid: Number(appid) }],
    context: { language: "english", country_code: "BR", steam_realm: 1 },
    data_request: { include_assets: true },
  }
  const r = await http(`${ITEMS_URL}?input_json=${encodeURIComponent(JSON.stringify(input))}`, {
    timeoutMs: 20000,
  })
  if (!r.ok) return null
  const j = await r.json()
  const it = j?.response?.store_items?.[0]
  if (!it || typeof it.type !== "number") return null
  const assets = it.assets || {}
  const urlDeAsset = (a, nome) => {
    if (!a?.asset_url_format || !nome) return ""
    return a.asset_url_format.replace("${FILENAME}", nome)
  }
  return {
    data: {
      tipo: it.type,
      capa: urlDeAsset(assets, assets.library_capsule),
      heroi: urlDeAsset(assets, assets.library_hero_2x || assets.library_hero),
      icon: urlDeAsset(assets, assets.icon || assets.community_icon),
    },
    at: nowEpochS(),
  }
}

// ---------- Manifestos (sondagem de disponibilidade por URL) ----------
// O app sonda cada provedor (HEAD) e guarda store_manifest_cache.json.
// Aqui fazemos o mesmo para um appid: verifica em quais provedores o zip
// existe. So provedores SEM chave (Ryuu, Sushi, TwentyTwo) — Hubcap precisa
// da key do usuario e fica no app.
const RYUU_ZIP = (appid) => `http://167.235.229.108/${appid}`
const SUSHI_ZIP = (appid) =>
  `https://raw.githubusercontent.com/sushi-dev55-alt/sushitools-games-repo-alt/refs/heads/main/${appid}.zip`
const TWENTYTWO_ZIP = (appid) => `https://api.twentytwocloud.com/download?appid=${appid}`

async function fetchManifests(appid) {
  const urls = {
    ryuu: RYUU_ZIP(appid),
    sushi: SUSHI_ZIP(appid),
    twentytwo: TWENTYTWO_ZIP(appid),
  }
  const out = {}
  for (const [nome, url] of Object.entries(urls)) {
    try {
      const r = await http(url, { method: "HEAD", timeoutMs: 6000 })
      out[url] = { at: nowEpochS(), ok: r.ok }
    } catch {
      out[url] = { at: nowEpochS(), ok: false }
    }
  }
  return { data: out, at: nowEpochS() }
}

// ---------- Dispatcher ----------
// Resolve key -> fetch. Retorna { data, at } ou null se a fonte nao respondeu.
async function fetchCatalogKey(key) {
  if (key === "popular") return fetchPopular()
  if (key === "steam250") return fetchSteam250()
  if (key === "sushi") return fetchSushi()
  if (key === "news") return fetchNews()
  if (key === "fixes") return fetchFixes()
  if (key === "ryuu-index") return fetchRyuuIndex()
  if (key.startsWith("genre:")) {
    // "all"/"__all" = o catalogo completo do Em alta (mesmo do popular).
    // O app manda ?lista=all e espera esta lista; sem isto o servidor
    // devolvia vazio e o app caia na SteamSpy viva a cada pagina.
    if (key === "genre:__all" || key === "genre:all") return fetchPopular()
    return null // outras listas: app mantem
  }
  if (key.startsWith("sysinfo:")) return fetchSysinfo(key.slice("sysinfo:".length))
  if (key.startsWith("meta:")) return fetchMeta(key.slice("meta:".length))
  if (key.startsWith("hltb:")) return null // placeholder (app mantem local)
  if (key.startsWith("items:")) return fetchItems(key.slice("items:".length))
  if (key.startsWith("manifests:")) return fetchManifests(key.slice("manifests:".length))
  if (key.startsWith("hydra:")) return null // precisa da URL da fonte (app)
  return null
}

module.exports = { fetchCatalogKey, catalogKey, CATALOG_KEYS, CATALOG_TTL, idValido, http }
