// Notícias de jogos em PT-BR via RSS (sem chave de API). Faz o fetch dos feeds,
// parseia o XML (regex, no estilo do metadata.js) e normaliza para NewsItem[].

const { semHTML } = require("./metadata")
const { fetchRede } = require("./httpfetch")

const UA =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 " +
  "(KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36"

// Fontes cuja PAUTA é jogos (PT + EN). Trocado o IGN Brasil (site de tech geral,
// que trazia fone/GPU/monitor) por sites dedicados a games. Nenhum é 100% puro
// — de vez em quando escapa um review de celular ou notícia de filme — por isso
// o filtro abaixo continua como rede de segurança.
const FEEDS = [
  { source: "Eurogamer", url: "https://www.eurogamer.pt/feed" },
  { source: "Nintendo Blast", url: "https://www.nintendoblast.com.br/feeds/posts/default?alt=rss" },
  { source: "GameSpot", url: "https://www.gamespot.com/feeds/game-news/" },
  { source: "Rock Paper Shotgun", url: "https://www.rockpapershotgun.com/feed" },
  { source: "Push Square", url: "https://www.pushsquare.com/feeds/latest" },
  { source: "PC Gamer", url: "https://www.pcgamer.com/rss/" },
]

// Peneira anti-anúncio: descarta ofertas/descontos e posts de hardware (fone,
// GPU, monitor, portátil…), que não são notícia de jogo. Roda sobre título + link.
const BLOCK_URL =
  /\/(descontos|ofertas|deals|promo(?:cao|coes)?|hardware|reviews\/hardware|perifericos|celular|smartphone)\b/i

// Palavras de OFERTA (preço/desconto). Independem de ser hardware.
const BLOCK_OFERTA =
  /(cai de pre|menor pre|melhor pre|mais barat|desconto|em oferta|black friday|cupom|\bdeal\b|% off|por (?:apenas|cerca de|r\$))/i

// Nomes de HARDWARE. O sufixo (?:es|s)? cobre o plural em PT/EN (monitor→
// monitores, headset→headsets) sem largar o \b — que ainda protege contra
// falso-positivo tipo "Mousehunt" (jogo) casar com "mouse".
// Stems (sem \b interno): o wrapper `\b(...)(?:es|s)?\b` cuida da borda e do
// plural PT/EN. "fone" pega "fones" mas não "fonema"/"telefone"; "intel" pega
// "Intel" mas não "Intelligent" (o \b final falha no meio da palavra).
const HW = [
  "gpu", "cpu", "ssd", "hd externo", "placa de v[íi]deo", "placa-m[ãa]e",
  "monitor", "headset", "fone", "teclado", "gabinete", "cooler",
  "fonte de alimenta", "notebook", "smartphone", "celular", "processador",
  "cadeira gamer", "power ?bank", "roteador", "smart ?tv", "geladeira",
  "carregador", "rtx", "gtx", "geforce", "radeon", "ryzen", "intel",
]
const BLOCK_HW = new RegExp(`\\b(${HW.join("|")})(?:es|s)?\\b`, "i")

// Portáteis, consoles-hardware e modelos de celular. Aqui NÃO uso o wrapper com
// \b final, porque esses nomes vêm seguidos de número ("Galaxy S26", "Legion
// C700") e o \b falharia entre a letra e o dígito. Cada padrão é específico o
// bastante para não pegar jogo — "galaxy s\d" não casa com "Super Mario Galaxy".
const BLOCK_DISPOSITIVO =
  /(steam deck|rog ally|legion go|legion c\d|msi claw|\bhandheld\b|console port[áa]til|ayn odin|retroid|samsung galaxy|galaxy s\d|galaxy z|iphone|ipad|macbook|pixel \d)/i

function ehJogo(n) {
  const txt = n.title || ""
  const url = n.url || ""
  if (BLOCK_URL.test(url)) return false
  if (BLOCK_OFERTA.test(txt)) return false
  if (BLOCK_HW.test(txt)) return false
  if (BLOCK_DISPOSITIVO.test(txt)) return false
  return true
}

// hash simples e estável (djb2) para o id a partir do link.
function hashId(s) {
  let h = 5381
  for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) | 0
  return "news_" + (h >>> 0).toString(36)
}

// Decodifica entidades numéricas (&#8243; &#x201d;) que o semHTML não trata.
function decodeNum(s) {
  return String(s || "")
    .replace(/&#x([0-9a-f]+);/gi, (_, h) => String.fromCodePoint(parseInt(h, 16)))
    .replace(/&#(\d+);/g, (_, d) => String.fromCodePoint(parseInt(d, 10)))
}

// Limpa texto de feed: alguns feeds (IGN) mandam HTML ESCAPADO (&lt;img&gt;), então
// é preciso decodificar as entidades ANTES de tirar as tags — senão a tag vira
// texto visível. Faz as duas coisas, na ordem certa.
function limparTexto(s) {
  let t = decodeNum(String(s || ""))
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
  // remove CDATA se houver
  return m[1].replace(/^<!\[CDATA\[/, "").replace(/\]\]>$/, "").trim()
}

// Sobe a resolução da imagem. Quase todo feed manda thumbnail, que esticado no
// banner do destaque vira um borrão. Cada CDN tem seu jeito de pedir o tamanho
// cheio — aqui reescrevo os padrões conhecidos; o resto passa intacto.
function melhorarImagem(url) {
  if (!url) return url
  let u = url.replace(/&amp;/g, "&") // Eurogamer manda a URL com &amp;

  // Blogger (Nintendo Blast): segmento /s72-w640-h360-c/ define o tamanho.
  // /s0/ devolve o original; troco qualquer variação por ele.
  if (/blogger\.googleusercontent\.com|bp\.blogspot\.com/.test(u)) {
    u = u.replace(/\/(s\d+(?:-[a-z]\d+)*(?:-c)?)\//i, "/s0/")
    return u
  }
  // Push Square: .../small.jpg | .../medium.jpg → .../large.jpg
  if (/images\.pushsquare\.com/.test(u)) {
    return u.replace(/\/(small|medium|thumb)\.(jpg|png|webp)/i, "/large.$2")
  }
  // GameSpot: ?w=300 (ou qualquer largura) → 1280
  if (/gamespot\.com/.test(u)) {
    return u.includes("?w=") ? u.replace(/\?w=\d+/, "?w=1280") : u
  }
  // Eurogamer (gnwcdn): ?width=690&... → 1280
  if (/gnwcdn\.com/.test(u)) {
    return u.replace(/([?&]width=)\d+/, "$11280").replace(/([?&]quality=)\d+/, "$190")
  }
  return u
}

// Primeira imagem do item (media:content / media:thumbnail / enclosure / <img>).
function pegarImagem(bloco) {
  const attr =
    bloco.match(/<(?:media:content|media:thumbnail|enclosure)[^>]*\burl="([^"]+)"/i)
  if (attr) return melhorarImagem(attr[1])
  const img = bloco.match(/<img[^>]*\bsrc="([^"]+)"/i)
  if (img) return melhorarImagem(img[1])
  const anyUrl = bloco.match(/https?:\/\/[^"'\s<>]+\.(?:jpg|jpeg|png|webp)/i)
  return anyUrl ? melhorarImagem(anyUrl[0]) : ""
}

async function buscarFeed(feed) {
  // Feed de notícias sem timeout pendurava a aba Notícias indefinidamente.
  // 8s: os feeds respondem em 3–10s. Com 20s, um único feed pendurado segurava
  // o Promise.all inteiro e, no caso sem cache, a aba junto.
  const r = await fetchRede(feed.url, { headers: { "User-Agent": UA }, signal: AbortSignal.timeout(8000) })
  if (!r.ok) throw new Error(`HTTP ${r.status}`)
  const xml = await r.text()
  const itens = xml.match(/<item[\s>][\s\S]*?<\/item>/gi) || []
  return itens.map((bloco) => {
    const link = pegar(bloco, "link") || pegar(bloco, "guid")
    const resumo = limparTexto(pegar(bloco, "description")).slice(0, 280)
    const data = pegar(bloco, "pubDate") || pegar(bloco, "dc:date")
    const iso = data ? new Date(data).toISOString() : ""
    return {
      id: hashId(link),
      title: limparTexto(pegar(bloco, "title")),
      summary: resumo,
      source: feed.source,
      url: link,
      image: pegarImagem(bloco),
      date: iso,
    }
  })
}

// Junta todas as fontes, ordena por data desc, remove duplicados e corta.
async function getNews(limite = 40) {
  const listas = await Promise.all(
    FEEDS.map((f) => buscarFeed(f).catch(() => [])),
  )
  const vistos = new Set()
  const todas = []
  for (const lista of listas) {
    for (const n of lista) {
      if (!n.title || !n.url || vistos.has(n.url)) continue
      if (!ehJogo(n)) continue // fora ofertas/hardware
      vistos.add(n.url)
      todas.push(n)
    }
  }
  todas.sort((a, b) => (b.date || "").localeCompare(a.date || ""))
  return todas.slice(0, limite)
}

module.exports = { getNews }
