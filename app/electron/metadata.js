// Busca de metadados online, em camada de provedores.
//
// Cada provedor sabe achar candidatos de arte para um jogo e devolve todos no
// mesmo formato, para a UI misturar as fontes numa grade só:
//
//   { fonte, url, thumb, largura, altura, animado, autor }
//
// Hoje são dois; a assinatura é a mesma para plugar outros (IGDB, RAWG…).

const { fetchRede } = require("./httpfetch")

const SGDB_BASE = "https://www.steamgriddb.com/api/v2"
const STEAM_CDN = "https://cdn.cloudflare.steamstatic.com/steam/apps"
const STEAM_STORE = "https://store.steampowered.com/api/appdetails"
const IGDB_IMG = "https://images.igdb.com/igdb/image/upload"
// A IGDB só atende com credenciais da Twitch. O Playnite resolve isso guardando
// as credenciais DELE num serviço próprio e servindo a API de lá — é por isso
// que ele nunca pede chave nenhuma ao usuário. O endereço está no plugin.cfg
// da extensão oficial (JosefNemec/PlayniteExtensions).
const IGDB_PROXY = "https://api2.playnite.link/api"

// Qual endpoint da SGDB corresponde a cada arte do nosso Game.
const SGDB_ENDPOINT = { cover: "grids", hero: "heroes", logo: "logos" }

// Dimensões que a SGDB aceita, por tipo. Capa retrato 600x900 = formato PS5.
// A lista é fechada: mandar uma dimensão inventada faz a API recusar.
const SGDB_DIMENSIONS = {
  cover: ["600x900", "660x930", "342x482", "920x430", "460x215"],
  hero: ["1920x620", "3840x1240", "1600x650"],
  logo: [], // logo não tem dimensão fixa
}
// O que é oferecido por padrão quando o usuário não filtra nada.
const SGDB_DIMENSIONS_PADRAO = {
  cover: ["600x900", "660x930", "342x482"], // só retrato
  hero: ["1920x620", "3840x1240", "1600x650"],
  logo: [],
}

// Timeout obrigatório: sem ele, um servidor que aceita a conexão e nunca
// responde deixa a promessa pendurada para sempre. Como esses fetch são
// aguardados dentro de handlers IPC, a tela ficava girando sem fim, sem erro.
async function getJSON(url, headers) {
  const r = await fetchRede(url, { headers, signal: AbortSignal.timeout(20000) })
  if (!r.ok) throw new Error(`HTTP ${r.status} em ${url}`)
  return r.json()
}

// ── SteamGridDB ────────────────────────────────────────────────────────────
// Arte da comunidade, cobre qualquer loja e tem versões animadas.

async function sgdbSearch(titulo, chave) {
  const url = `${SGDB_BASE}/search/autocomplete/${encodeURIComponent(titulo)}`
  const j = await getJSON(url, { Authorization: `Bearer ${chave}` })
  if (!j?.success) throw new Error("SteamGridDB recusou a busca (chave inválida?)")
  return (j.data || []).map((g) => ({ id: g.id, titulo: g.name, ano: g.release_date }))
}

// Monta a URL de arte.
//   animado    inclui webm/gif além das imagens paradas
//   dimensions filtra por resolução; vazio = o padrão do tipo
function sgdbArtURL(sgdbId, kind, { animado = true, dimensions } = {}) {
  const endpoint = SGDB_ENDPOINT[kind]
  if (!endpoint) throw new Error(`tipo de arte desconhecido: ${kind}`)
  const p = new URLSearchParams()
  p.set("types", animado ? "static,animated" : "static")
  // Só passa adiante o que a API reconhece: dimensão inventada faz ela recusar
  // o pedido inteiro, e aí o usuário não veria arte nenhuma. Se sobrar nada do
  // filtro, cai no padrão do tipo — sem parâmetro a API devolveria TUDO, que é
  // o contrário do que quem filtrou pediu.
  const validas = SGDB_DIMENSIONS[kind] || []
  const padrao = (SGDB_DIMENSIONS_PADRAO[kind] || []).filter((d) => validas.includes(d))
  const pedidas = (dimensions || []).filter((d) => validas.includes(d))
  const dims = pedidas.length ? pedidas : padrao
  if (dims.length) p.set("dimensions", dims.join(","))
  p.set("nsfw", "false")
  return `${SGDB_BASE}/${endpoint}/game/${sgdbId}?${p}`
}

async function sgdbArt(sgdbId, kind, chave, opts) {
  const j = await getJSON(sgdbArtURL(sgdbId, kind, opts), {
    Authorization: `Bearer ${chave}`,
  })
  if (!j?.success) throw new Error("SteamGridDB recusou o pedido de arte")
  return (j.data || []).map((a) => ({
    fonte: "SteamGridDB",
    url: a.url,
    thumb: a.thumb || a.url,
    largura: a.width,
    altura: a.height,
    // A API marca animados pelo mime (image/gif, video/webm…).
    animado: Boolean(
      a.mime && a.mime !== "image/png" && a.mime !== "image/jpeg" && a.mime !== "image/webp",
    ),
    autor: a.author?.name || "",
  }))
}

// ── Steam ──────────────────────────────────────────────────────────────────
// Sem chave. Só serve para jogos Steam, e a arte é a oficial da loja.

// Extrai o appid de um id Steam ("steam:346110").
function steamAppId(gameId) {
  const m = /^steam:(\d+)$/.exec(String(gameId || ""))
  return m ? m[1] : null
}

// Nomes possíveis por tipo. A Steam não tem endpoint de "listar artes", e
// QUAIS existem muda de jogo para jogo (uns têm library_600x900, outros só
// hero_2x). Por isso cada candidato é conferido com HEAD antes de ser
// oferecido — senão a grade encheria de miniatura quebrada.
const STEAM_FILES = {
  cover: ["library_600x900.jpg", "header.jpg"],
  hero: ["library_hero.jpg", "library_hero_2x.jpg", "page_bg_generated_v6b.jpg"],
  logo: ["logo.png", "logo_2x.png"],
}

async function existe(url) {
  try {
    const r = await fetchRede(url, { method: "HEAD" })
    return r.ok
  } catch {
    return false
  }
}

async function steamArt(gameId, kind) {
  const appid = steamAppId(gameId)
  const arquivos = STEAM_FILES[kind]
  if (!appid || !arquivos) return []
  const urls = arquivos.map((f) => `${STEAM_CDN}/${appid}/${f}`)
  const achados = await Promise.all(urls.map(existe))
  return urls
    .filter((_, i) => achados[i])
    .map((url) => ({
      fonte: "Steam",
      url,
      thumb: url,
      largura: 0,
      altura: 0,
      animado: false,
      autor: "oficial",
    }))
}

// ── IGDB ───────────────────────────────────────────────────────────────────
// Arte E texto para qualquer plataforma — inclusive jogo que não está em loja
// de PC nenhuma. Sem chave: passa pelo serviço público do Playnite, que guarda
// as credenciais dele.
//
// Duas ressalvas que decidem o lugar dela na fila:
//
//   1. O servidor é dos outros. Uma chamada por busca manual, nunca em lote e
//      nunca na indexação. Se sair do ar, o resto tem de continuar de pé.
//   2. A IGDB não tem campo de idioma: o texto vem sempre em inglês. Por isso
//      a Steam e a Xbox, que traduzem, vêm antes.

// URL de imagem da IGDB: o id vira caminho, o tamanho é um token "t_".
function igdbImg(imageId, tamanho) {
  return `${IGDB_IMG}/t_${tamanho}/${imageId}.jpg`
}

const IGDB_UA = "Arcadia (github.com/imperat-on/arcadia)"

// ── Circuit Breaker para IGDB ──────────────────────────────────────────────
// Protege contra falhas em cascata quando o serviço da IGDB está com problemas.
// Depois de N falhas consecutivas, entra em estado "aberto" e rejeita pedidos
// rápido sem tentar de verdade. Tenta de novo depois de um tempo (half-open).

class CircuitBreaker {
  constructor({ threshold = 5, timeout = 60000, resetTimeout = 30000 } = {}) {
    this.threshold = threshold // falhas consecutivas para abrir
    this.timeout = timeout // quanto tempo ficar aberto
    this.resetTimeout = resetTimeout // quanto tempo em half-open antes de resetar
    this.failures = 0
    this.state = "closed" // closed | open | half-open
    this.nextAttempt = 0
  }

  async execute(fn) {
    if (this.state === "open") {
      if (Date.now() < this.nextAttempt) {
        throw new Error("IGDB circuit breaker aberto (muitas falhas recentes)")
      }
      // Tempo expirou: tenta de novo (half-open)
      this.state = "half-open"
    }

    try {
      const result = await fn()
      // Sucesso: reseta o circuit breaker
      if (this.state === "half-open") {
        this.state = "closed"
        this.failures = 0
      } else if (this.state === "closed") {
        this.failures = 0
      }
      return result
    } catch (err) {
      this.failures++
      if (this.failures >= this.threshold) {
        this.state = "open"
        this.nextAttempt = Date.now() + this.timeout
      }
      throw err
    }
  }

  reset() {
    this.failures = 0
    this.state = "closed"
    this.nextAttempt = 0
  }
}

const igdbCircuitBreaker = new CircuitBreaker({
  threshold: 5,
  timeout: 60000, // 1 minuto aberto
  resetTimeout: 30000,
})

// ── Cache da IGDB com expiração ────────────────────────────────────────────
// Buscar e abrir a ficha são duas viagens. Reabrir o diálogo no mesmo jogo é
// comum, então o resultado fica guardado pela sessão. Agora com expiração e
// limite de tamanho mais inteligente (LRU).

class LRUCache {
  constructor(maxSize = 100, ttl = 3600000) {
    this.maxSize = maxSize // máximo de entradas
    this.ttl = ttl // time to live em ms (padrão: 1 hora)
    this.cache = new Map()
  }

  get(key) {
    const entry = this.cache.get(key)
    if (!entry) return undefined

    // Verifica expiração
    if (Date.now() > entry.expires) {
      this.cache.delete(key)
      return undefined
    }

    // Move para o final (mais recente)
    this.cache.delete(key)
    this.cache.set(key, entry)
    return entry.value
  }

  set(key, value) {
    // Remove se já existe (para reordenar)
    if (this.cache.has(key)) {
      this.cache.delete(key)
    }

    // Remove o mais antigo se atingiu o limite
    if (this.cache.size >= this.maxSize) {
      const firstKey = this.cache.keys().next().value
      this.cache.delete(firstKey)
    }

    this.cache.set(key, {
      value,
      expires: Date.now() + this.ttl,
    })
  }

  clear() {
    this.cache.clear()
  }

  get size() {
    return this.cache.size
  }
}

const igdbCache = new LRUCache(100, 3600000) // 100 entradas, 1 hora de TTL

// ── Matching melhorado com plataforma e ano ────────────────────────────────
// A IGDB pode ter dezenas de versões do mesmo jogo (PS4, PS5, Switch, PC...).
// Considerar plataforma e ano de lançamento melhora a precisão da escolha.

function scoreIgdbMatch(candidato, busca) {
  const nomeNorm = normalizaTitulo(candidato.name || "")
  const buscaNorm = normalizaTitulo(busca.titulo || "")
  let score = 0

  // Match exato de título: +100
  if (nomeNorm === buscaNorm) {
    score += 100
  }
  // Título começa com a busca ou vice-versa: +50
  else if (nomeNorm.startsWith(buscaNorm) || buscaNorm.startsWith(nomeNorm)) {
    score += 50
  }
  // Título contém a busca: +20
  else if (nomeNorm.includes(buscaNorm) || buscaNorm.includes(nomeNorm)) {
    score += 20
  }

  // Match de plataforma: +30
  if (busca.plataforma && candidato.platforms) {
    const plat = busca.plataforma.toLowerCase()
    const plataformasNome = candidato.platforms.map((p) =>
      (p.name || "").toLowerCase()
    )

    // Mapeamento de plataformas comuns
    const platMap = {
      windows: ["pc", "windows", "microsoft windows"],
      pc: ["pc", "windows", "microsoft windows"],
      playstation: ["playstation", "ps4", "ps5", "sony"],
      ps4: ["ps4", "playstation 4"],
      ps5: ["ps5", "playstation 5"],
      xbox: ["xbox", "microsoft"],
      switch: ["switch", "nintendo switch"],
      steam: ["pc", "windows", "steam"],
      "sony-playstation": ["playstation", "playstation 1"],
      "sony-playstation-2": ["playstation 2"],
      "sony-playstation-3": ["playstation 3"],
      "sony-psp": ["playstation portable", "psp"],
      "nintendo-gamecube": ["gamecube"],
      "nintendo-wii": ["wii"],
      "nintendo-ds": ["nintendo ds"],
      "nintendo-dsi": ["nintendo dsi"],
      "nintendo-nes": ["nintendo entertainment system", "nes"],
      "nintendo-snes": ["super nintendo entertainment system", "snes"],
      "nintendo-game-boy": ["game boy"],
      "nintendo-game-boy-color": ["game boy color"],
      "nintendo-game-boy-advance": ["game boy advance"],
      "nintendo-64": ["nintendo 64"],
    }

    const aliases = platMap[plat] || [plat]
    if (plataformasNome.some((p) => aliases.some((a) => p.includes(a)))) {
      score += 30
    }
  }

  // Match de ano: +20 se igual, -10 se diferença > 2 anos
  if (busca.ano && candidato.first_release_date) {
    const anoJogo = new Date(candidato.first_release_date * 1000).getFullYear()
    const anoBusca = parseInt(busca.ano, 10)

    if (anoJogo === anoBusca) {
      score += 20
    } else {
      const diff = Math.abs(anoJogo - anoBusca)
      if (diff <= 1) {
        score += 10 // ano próximo
      } else if (diff > 2) {
        score -= 10 // ano distante
      }
    }
  }

  // Penaliza jogos sem release date (provavelmente não lançados)
  if (!candidato.first_release_date) {
    score -= 5
  }

  return score
}

async function igdbProxy(titulo, { plataforma = null, ano = null } = {}) {
  const t = String(titulo || "").trim()
  if (!t) return []

  // Cache key inclui plataforma e ano para melhor precisão
  const chave = `${t.toLowerCase()}|${plataforma || ""}|${ano || ""}`
  const cached = igdbCache.get(chave)
  if (cached) return cached

  return igdbCircuitBreaker.execute(async () => {
    const r = await fetchRede(`${IGDB_PROXY}/igdb/search`, {
      method: "POST",
      headers: { "Content-Type": "application/json", "User-Agent": IGDB_UA },
      body: JSON.stringify({ SearchTerm: t }),
      signal: AbortSignal.timeout(15000), // timeout menor (15s)
    })
    if (!r.ok) throw new Error(`IGDB respondeu HTTP ${r.status}`)
    const achados = (await r.json())?.data || []
    if (!achados.length) {
      igdbCache.set(chave, [])
      return []
    }

    // A busca é fuzzy e ela não devolve o resumo nem as artes — só a ficha
    // completa tem. Então: escolhe um e busca a ficha.
    //
    // Agora usa scoring inteligente considerando plataforma e ano, não apenas
    // o título. "Resident Evil 4" no PS5 é diferente da versão de GameCube.
    const busca = { titulo: t, plataforma, ano }
    const candidatosComScore = achados.map((c) => ({
      ...c,
      score: scoreIgdbMatch(c, busca),
    }))

    // Ordena por score (maior primeiro)
    candidatosComScore.sort((a, b) => b.score - a.score)

    // Pega o melhor match, mas só se o score for razoável (> 0)
    const melhor = candidatosComScore[0]
    if (!melhor || melhor.score <= 0) {
      igdbCache.set(chave, [])
      return []
    }

    const f = await fetchRede(`${IGDB_PROXY}/igdb/game/${melhor.id}`, {
      headers: { "User-Agent": IGDB_UA },
      signal: AbortSignal.timeout(15000), // timeout menor (15s)
    })
    if (!f.ok) throw new Error(`IGDB respondeu HTTP ${f.status}`)
    const ficha = (await f.json())?.data
    const jogos = ficha ? [igdbNormaliza(ficha)] : []

    igdbCache.set(chave, jogos)
    return jogos
  })
}

/**
 * O proxy devolve os campos aninhados com sufixo `_expanded`. Traduz para o
 * formato da API crua, que é o que `igdbArtDe` e `igdbTextosDe` já consomem.
 */
function igdbNormaliza(g) {
  return {
    name: g.name || "",
    summary: g.summary || "",
    storyline: g.storyline || "",
    cover: g.cover_expanded || null,
    artworks: g.artworks_expanded || [],
    screenshots: g.screenshots_expanded || [],
  }
}

// Arte da IGDB no nosso formato. Capa vem de `cover`; fundo, de `artworks` e
// `screenshots` (a IGDB não tem "hero", então screenshot faz as vezes).
//
// Sempre o token "_2x": t_cover_big entrega 264x374, MENOS pixels do que a
// própria fileira precisa num monitor 2x (152x228 CSS = 304x456 reais). A
// miniatura da grade continua pequena de propósito — lá são dezenas de
// imagens, e baixar a versão cheia em cada uma travaria a busca.
function igdbArtDe(jogos, kind) {
  const out = []
  for (const g of jogos || []) {
    if (kind === "cover" && g.cover?.image_id) {
      out.push(igdbCandidato(g.cover.image_id, "cover_big_2x", 528, 748))
    }
    if (kind === "hero") {
      for (const a of g.artworks || []) {
        out.push(igdbCandidato(a.image_id, "1080p_2x", 3840, 2160))
      }
      for (const s of g.screenshots || []) {
        out.push(igdbCandidato(s.image_id, "1080p_2x", 3840, 2160))
      }
    }
    // A IGDB não tem logo transparente: nada a oferecer para kind === "logo".
  }
  return out
}

function igdbCandidato(imageId, tamanho, largura, altura) {
  return {
    fonte: "IGDB",
    url: igdbImg(imageId, tamanho),
    thumb: igdbImg(imageId, "screenshot_med"),
    largura,
    altura,
    animado: false,
    autor: "IGDB",
  }
}

// ── Xbox / Microsoft Store ─────────────────────────────────────────────────
// Catálogo público: NÃO precisa de chave. Dá capa retrato 1440x2160 (2:3, a
// proporção da nossa fileira), fundo em 4K e descrição no idioma pedido.
// Só cobre o que é vendido na loja da Microsoft.

const XBOX_SEARCH = "https://storeedgefd.dsx.mp.microsoft.com/v9.0/search"
const XBOX_CATALOG = "https://displaycatalog.mp.microsoft.com/v7.0/products"

// Que ImagePurpose serve para cada arte nossa, em ordem de preferência.
const XBOX_IMAGENS = {
  cover: ["Poster", "BrandedKeyArt", "BoxArt"],
  hero: ["SuperHeroArt", "TitledHeroArt", "Screenshot"],
  logo: ["Logo"],
}

// Tira acentos, pontuação e caixa, para comparar títulos.
function normalizaTitulo(s) {
  return String(s || "")
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "") // marcas de acento soltas pelo NFD
    .toLowerCase()
    .replace(/[^a-z0-9]/g, "")
}

// A busca da loja é fuzzy e devolve lixo para jogo que ela não tem ("Elden
// Ring" volta como "Eldrynn"). Sem esta peneira, o usuário veria a capa do
// jogo errado etiquetada como certa.
function tituloBate(a, b) {
  const x = normalizaTitulo(a)
  const y = normalizaTitulo(b)
  if (!x || !y) return false
  return x === y || x.startsWith(y) || y.startsWith(x)
}

async function xboxSearch(titulo, market = "US", locale = "en-us") {
  const p = new URLSearchParams({
    query: titulo,
    market,
    locale,
    deviceFamily: "Windows.Desktop",
  })
  const j = await getJSON(`${XBOX_SEARCH}?${p}`, { "User-Agent": "Mozilla/5.0" })
  const achados = j?.Payload?.SearchResults || []
  return achados
    .filter((r) => tituloBate(r.Title, titulo))
    .map((r) => ({ id: r.ProductId, titulo: r.Title }))
}

async function xboxProduto(productId, market = "US", languages = "en-us") {
  const p = new URLSearchParams({ bigIds: productId, market, languages, "MS-CV": "x" })
  const j = await getJSON(`${XBOX_CATALOG}?${p}`, { "User-Agent": "Mozilla/5.0" })
  return j?.Products?.[0]?.LocalizedProperties?.[0] || null
}

// As URIs vêm sem protocolo ("//store-images..."): o <img> aceita, o fetch não.
function xboxUri(uri) {
  const u = String(uri || "")
  return u.startsWith("//") ? "https:" + u : u
}

// O serviço de imagem da Microsoft aceita tamanho e qualidade na query. Pedir
// a URI crua devolve a resolução certa, mas com a compressão PADRÃO dela: a
// mesma capa 1440x2160 sai com 680 KB crua e 2,3 MB com q=100. É de onde vinha
// o artefato. Aqui pedimos explícito.
function xboxImg(uri, { w, h, q = 100 } = {}) {
  const base = xboxUri(uri)
  if (!base) return base
  const p = new URLSearchParams({ q: String(q), format: "jpg" })
  if (w) p.set("w", String(w))
  if (h) p.set("h", String(h))
  return `${base}?${p}`
}

function xboxArtDe(loc, kind) {
  const querem = XBOX_IMAGENS[kind] || []
  const imgs = (loc?.Images || []).filter((i) => querem.includes(i.ImagePurpose))
  // Mantém a ordem de preferência do tipo, e maior primeiro dentro dela.
  imgs.sort(
    (a, b) =>
      querem.indexOf(a.ImagePurpose) - querem.indexOf(b.ImagePurpose) ||
      (b.Width || 0) - (a.Width || 0),
  )
  return imgs.map((i) => ({
    fonte: "Xbox",
    // Download: tamanho cheio do asset, qualidade máxima.
    url: xboxImg(i.Uri, { w: i.Width, h: i.Height, q: 100 }),
    // Miniatura: pequena de propósito, senão a grade baixa dezenas de imagens
    // cheias só para mostrar cards de 140px.
    thumb: xboxImg(i.Uri, { w: 320, q: 90 }),
    largura: i.Width || 0,
    altura: i.Height || 0,
    animado: false,
    autor: "oficial",
  }))
}

function xboxTextoDe(loc) {
  const t = semHTML(loc?.ProductDescription)
  return t ? [{ fonte: `Xbox — ${loc.ProductTitle}`, texto: t }] : []
}

// ── Descrições ─────────────────────────────────────────────────────────────

// Tira as tags do about_the_game da Steam, que vem em HTML.
function semHTML(s) {
  return String(s || "")
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/<\/p>/gi, "\n")
    .replace(/<[^>]+>/g, "")
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&nbsp;/g, " ")
    .replace(/\n{3,}/g, "\n\n")
    .trim()
}

// Descrições da Steam: curta e completa. Sem chave, e a Steam traduz.
//
// `appidFallback` é para jogo que não é da Steam (Epic, GOG, custom): quem
// chama resolve o appid pelo título e passa aqui. A maioria dos jogos de PC
// tem página na Steam mesmo quando o usuário comprou em outro lugar, então
// esta é a melhor fonte traduzida que temos para eles.
async function steamTextos(gameId, lang = "english", appidFallback = "") {
  const appid = steamAppId(gameId) || String(appidFallback || "")
  if (!appid) return []
  const p = new URLSearchParams({ appids: appid, l: lang })
  const j = await getJSON(`${STEAM_STORE}?${p}`, { "User-Agent": "Mozilla/5.0" })
  const info = j?.[appid]?.success ? j[appid].data : null
  if (!info) return []
  const out = []
  if (info.short_description) {
    out.push({ fonte: "Steam (curta)", texto: semHTML(info.short_description) })
  }
  if (info.about_the_game) {
    out.push({ fonte: "Steam (completa)", texto: semHTML(info.about_the_game) })
  }
  return out
}

// Descrições da IGDB: resumo e enredo.
function igdbTextosDe(jogos) {
  const out = []
  for (const g of jogos || []) {
    if (g.summary) out.push({ fonte: `IGDB — ${g.name}`, texto: semHTML(g.summary) })
    if (g.storyline) {
      out.push({ fonte: `IGDB (enredo) — ${g.name}`, texto: semHTML(g.storyline) })
    }
  }
  return out
}

// ── Download ───────────────────────────────────────────────────────────────

const MIME_EXT = {
  "image/png": ".png",
  "image/jpeg": ".jpg",
  "image/webp": ".webp",
  "image/gif": ".gif",
  "video/webm": ".webm",
  "video/mp4": ".mp4",
}

// Extensão do arquivo: o caminho da URL manda; o content-type é o desempate.
// Sem isso, um hero animado .webm viraria ".jpg" e não tocaria.
function extFromUrl(url, contentType) {
  try {
    const p = new URL(url).pathname
    const m = /\.([a-z0-9]{2,5})$/i.exec(p)
    if (m) return "." + m[1].toLowerCase()
  } catch {
    /* URL torta: cai no content-type */
  }
  const ct = String(contentType || "")
    .split(";")[0]
    .trim()
    .toLowerCase()
  return MIME_EXT[ct] || ".png"
}

// Baixa para um caminho definido pelo chamador. Devolve { path, bytes }.
async function downloadTo(url, destSemExt, fs) {
  const r = await fetchRede(url)
  if (!r.ok) throw new Error(`download falhou: HTTP ${r.status}`)
  const ext = extFromUrl(url, r.headers.get("content-type"))
  const buf = Buffer.from(await r.arrayBuffer())
  const dest = destSemExt + ext
  fs.writeFileSync(dest, buf)
  return { path: dest, bytes: buf.length }
}

// ── Wallhaven ────────────────────────────────────────────────────────────────
// Wallpapers reais (16:9, 4K de verdade) para usar como fundo. Sem chave (SFW).
// Bom para o `hero`, já que o fundo é meio que um wallpaper da tela toda.

const WALLHAVEN_BASE = "https://wallhaven.cc/api/v1"

async function wallhavenBusca(titulo, { atleast = "3840x2160" } = {}) {
  const busca = async (min) => {
    const p = new URLSearchParams({
      q: titulo,
      categories: "110", // geral + anime, sem "pessoas"
      purity: "100", // só SFW
      ratios: "16x9", // preenche a tela sem esticar
      atleast: min, // resolução mínima
      sorting: "relevance",
    })
    const j = await getJSON(`${WALLHAVEN_BASE}/search?${p}`)
    return (j?.data || []).map((w) => ({
      fonte: "Wallhaven",
      url: w.path,
      thumb: w.thumbs?.small || w.thumbs?.large || w.path,
      largura: w.dimension_x,
      altura: w.dimension_y,
      animado: false,
      autor: "",
    }))
  }
  let out = await busca(atleast)
  // Sem nada em 4K? Afrouxa para 1440p para não deixar o usuário na mão.
  if (!out.length && atleast !== "2560x1440") out = await busca("2560x1440")
  return out
}

// ── PlayStation Store ────────────────────────────────────────────────────────
// Arte oficial da PS Store, pública (sem login). Faz scraping da busca e da
// página do jogo — as imagens vêm num JSON embutido {"__typename":"Media",...}.
// Papéis: PORTRAIT_BANNER (capa 2:3), SIXTEEN_BY_NINE_BANNER/BACKGROUND (fundo
// 4K), LOGO (PNG transparente). Mesmo método do plugin playnite-metadata-psn.

const PSN_STORE = "https://store.playstation.com/en-us"
const PSN_UA =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 " +
  "(KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36"

// Prioridade de papel por tipo de arte (o 1º que existir ganha a lista).
const PSN_ROLES = {
  cover: [
    "PORTRAIT_BANNER",
    "MASTER",
    "GAMEHUB_COVER_ART",
    "EDITION_KEY_ART",
    "FOUR_BY_THREE_BANNER",
  ],
  hero: ["SIXTEEN_BY_NINE_BANNER", "BACKGROUND", "EDITION_KEY_ART", "MASTER"],
  logo: ["LOGO"],
}

async function getText(url) {
  const r = await fetchRede(url, { headers: { "User-Agent": PSN_UA } })
  if (!r.ok) throw new Error(`HTTP ${r.status} em ${url}`)
  return r.text()
}

async function psnStoreSearch(titulo) {
  const html = await getText(`${PSN_STORE}/search/${encodeURIComponent(titulo)}`)
  const tiles = []
  const seen = new Set()
  const re = /data-track="web:store:(concept|product)-tile"[^>]*data-telemetry-meta="([^"]+)"/g
  let m
  while ((m = re.exec(html))) {
    let meta
    try {
      // o JSON vem com entidades HTML (&quot; &#x27; …)
      const decoded = m[2]
        .replace(/&quot;/g, '"')
        .replace(/&#x27;/g, "'")
        .replace(/&#39;/g, "'")
        .replace(/&amp;/g, "&")
      meta = JSON.parse(decoded)
    } catch {
      continue
    }
    if (!meta.id || seen.has(meta.id)) continue
    seen.add(meta.id)
    tiles.push({ id: meta.id, tipo: m[1], titulo: meta.name || "" })
  }
  return tiles
}

// Escolhe o melhor resultado: título que bate exatamente; senão o 1º "jogo"
// (ignora DLC/pacotes de moeda quando dá para perceber pelo nome).
function psnMelhorResultado(tiles, titulo) {
  const exato = tiles.find((t) => tituloBate(t.titulo, titulo))
  return exato || tiles[0] || null
}

async function psnStoreArt(id, tipo, kind) {
  const html = await getText(`${PSN_STORE}/${tipo}/${id}`)
  const porRole = {}
  const re = /\{"__typename":"Media","role":"([A-Z_]+)","type":"IMAGE","url":"([^"]+)"\}/g
  let m
  while ((m = re.exec(html))) {
    if (!porRole[m[1]]) porRole[m[1]] = m[2]
  }
  const roles = PSN_ROLES[kind] || []
  const candidatos = []
  for (const role of roles) {
    const url = porRole[role]
    if (!url) continue
    candidatos.push({
      fonte: "PS Store",
      url,
      thumb: url + (url.includes("?") ? "&" : "?") + "w=320",
      largura: 0,
      altura: 0,
      animado: false,
      autor: "",
    })
  }
  return candidatos
}

module.exports = {
  sgdbSearch,
  sgdbArt,
  wallhavenBusca,
  psnStoreSearch,
  psnMelhorResultado,
  psnStoreArt,
  sgdbArtURL,
  steamArt,
  steamTextos,
  steamAppId,
  igdbProxy,
  igdbArtDe,
  igdbTextosDe,
  igdbImg,
  xboxSearch,
  xboxProduto,
  xboxArtDe,
  xboxTextoDe,
  xboxUri,
  tituloBate,
  normalizaTitulo,
  semHTML,
  extFromUrl,
  downloadTo,
  SGDB_ENDPOINT,
}
