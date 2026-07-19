// Busca de metadados online, em camada de provedores.
//
// Cada provedor sabe achar candidatos de arte para um jogo e devolve todos no
// mesmo formato, para a UI misturar as fontes numa grade só:
//
//   { fonte, url, thumb, largura, altura, animado, autor }
//
// Hoje são dois; a assinatura é a mesma para plugar outros (IGDB, RAWG…).

const SGDB_BASE = "https://www.steamgriddb.com/api/v2"
const STEAM_CDN = "https://cdn.cloudflare.steamstatic.com/steam/apps"
const STEAM_STORE = "https://store.steampowered.com/api/appdetails"
const IGDB_BASE = "https://api.igdb.com/v4"
const IGDB_IMG = "https://images.igdb.com/igdb/image/upload"
const TWITCH_TOKEN = "https://id.twitch.tv/oauth2/token"

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

async function getJSON(url, headers) {
  const r = await fetch(url, { headers })
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
    animado: Boolean(a.mime && a.mime !== "image/png" && a.mime !== "image/jpeg" && a.mime !== "image/webp"),
    autor: a.author?.name || "",
  }))
}

// ── Steam ──────────────────────────────────────────────────────────────────
// Sem chave. Só serve para jogos Steam, e a arte é a oficial da loja.

// Extrai o appid de um id do nosso indexador ("steam:346110").
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
    const r = await fetch(url, { method: "HEAD" })
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
// A fonte "grande": arte E texto, para qualquer plataforma. É a que o Playnite
// usa por padrão. Exige credenciais da Twitch (client id + secret).

// O token vale ~60 dias; guardamos em memória para não pedir a cada busca
// (a IGDB limita a 4 pedidos por segundo).
let tokenCache = { valor: null, expiraEm: 0 }

async function igdbToken(clientId, secret) {
  if (tokenCache.valor && Date.now() < tokenCache.expiraEm) return tokenCache.valor
  const p = new URLSearchParams({
    client_id: clientId,
    client_secret: secret,
    grant_type: "client_credentials",
  })
  const r = await fetch(`${TWITCH_TOKEN}?${p}`, { method: "POST" })
  if (!r.ok) throw new Error(`Twitch recusou as credenciais (HTTP ${r.status})`)
  const j = await r.json()
  if (!j.access_token) throw new Error("Twitch não devolveu token")
  // Encolhe 1 minuto para não usar um token que expira no meio do pedido.
  tokenCache = {
    valor: j.access_token,
    expiraEm: Date.now() + Math.max(0, (j.expires_in || 0) - 60) * 1000,
  }
  return tokenCache.valor
}

// Consulta na linguagem APICalypse (corpo de texto, não JSON).
async function igdbQuery(endpoint, corpo, clientId, secret) {
  const token = await igdbToken(clientId, secret)
  const r = await fetch(`${IGDB_BASE}/${endpoint}`, {
    method: "POST",
    headers: {
      "Client-ID": clientId,
      Authorization: `Bearer ${token}`,
      Accept: "application/json",
    },
    body: corpo,
  })
  if (!r.ok) throw new Error(`IGDB respondeu HTTP ${r.status}`)
  return r.json()
}

// URL de imagem da IGDB: o id vira caminho, o tamanho é um token "t_".
function igdbImg(imageId, tamanho) {
  return `${IGDB_IMG}/t_${tamanho}/${imageId}.jpg`
}

function igdbBusca(titulo) {
  const t = String(titulo || "").replace(/"/g, "")
  return (
    `search "${t}"; ` +
    "fields name,summary,storyline,first_release_date,total_rating," +
    "cover.image_id,artworks.image_id,screenshots.image_id,genres.name; " +
    "limit 8;"
  )
}

async function igdbGames(titulo, clientId, secret) {
  return igdbQuery("games", igdbBusca(titulo), clientId, secret)
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

// Descrições da Steam: curta e completa. Sem chave.
async function steamTextos(gameId, lang = "portuguese") {
  const appid = steamAppId(gameId)
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
  const ct = String(contentType || "").split(";")[0].trim().toLowerCase()
  return MIME_EXT[ct] || ".png"
}

// Baixa para um caminho definido pelo chamador. Devolve { path, bytes }.
async function downloadTo(url, destSemExt, fs) {
  const r = await fetch(url)
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
  cover: ["PORTRAIT_BANNER", "MASTER", "GAMEHUB_COVER_ART", "EDITION_KEY_ART", "FOUR_BY_THREE_BANNER"],
  hero: ["SIXTEEN_BY_NINE_BANNER", "BACKGROUND", "EDITION_KEY_ART", "MASTER"],
  logo: ["LOGO"],
}

async function getText(url) {
  const r = await fetch(url, { headers: { "User-Agent": PSN_UA } })
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
  igdbGames,
  igdbArtDe,
  igdbTextosDe,
  igdbImg,
  igdbBusca,
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
  SGDB_DIMENSIONS,
  SGDB_DIMENSIONS_PADRAO,
}
