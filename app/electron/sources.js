// Fontes de download estilo Hydra. O indice e os jogos completos vem primeiro
// do catalogo do servidor; os JSONs locais continuam como fallback offline.
//
// O Hydra manda a URL para a API deles, que valida/indexa/devolve delta. Aqui
// o cliente faz tudo: baixa o JSON da fonte (formato { name, downloads: [...] }),
// valida, cacheia em disco e indexa em RAM só os campos leves (título/tamanho).
// Os dados completos (uris) ficam no disco e são lidos sob demanda (getGame) —
// fontes grandes (2-15MB) não ficam paradas na memória.
//
// Sync usa ETag/If-Modified-Since: 304 = nada mudou, sem download nem parse.
const fs = require("fs")
const path = require("path")
const crypto = require("crypto")
const { fetchRede } = require("./httpfetch")
const { catalogGet } = require("./catalog")
const { getDataDir } = require("./runtime-paths")

const { caminhoArquivoConta } = require("./supabase/conta")
const DATA_DIR = getDataDir()
const SRC_DIR = path.join(DATA_DIR, "sources")
const REGISTRY = () => caminhoArquivoConta("sources.json")

function srcId(url) {
  return crypto.createHash("sha256").update(String(url)).digest("hex").slice(0, 12)
}

function cachePath(id) {
  return path.join(SRC_DIR, `${id}.json`)
}

function readRegistry() {
  try {
    return JSON.parse(fs.readFileSync(REGISTRY(), "utf-8"))
  } catch {
    return []
  }
}

function writeRegistry(list) {
  try {
    fs.mkdirSync(DATA_DIR, { recursive: true })
    const tmp = `${REGISTRY()}.tmp`
    fs.writeFileSync(tmp, JSON.stringify(list, null, 2))
    fs.renameSync(tmp, REGISTRY())
  } catch {}
}

// Aceita tanto { name, downloads: [...] } (Hydra) quanto um array puro.
function validar(data) {
  const downloads = Array.isArray(data) ? data : data?.downloads
  if (!Array.isArray(downloads) || !downloads.length) return null
  const temUri = (d) => {
    if (Array.isArray(d?.uris)) return d.uris.some((uri) => String(uri || "").trim())
    if (typeof d?.uris === "string" && d.uris.trim()) return true
    return typeof d?.uri === "string" && d.uri.trim().length > 0
  }
  const ok = downloads.filter((d) => d && d.title && temUri(d))
  return ok.length ? ok : null
}

// Baixa (condicionalmente) e grava o cache da fonte. Retorna
// { mudou, count } ou lança erro com motivo legível para a UI.
// Sessão persistente das fontes: guarda o cf_clearance do Cloudflare. Depois
// de passar no desafio UMA vez (via janela oculta), os fetches diretos com a
// mesma sessão passam sem desafio até o cookie expirar.
function sesFontes() {
  try {
    const { session } = require("electron")
    return session.fromPartition("persist:sources")
  } catch {
    return null
  }
}

// Fontes atrás de Cloudflare (ex.: hydralinks.cloud) respondem 403 + desafio
// JS para qualquer cliente que não é navegador completo. Sem servidor próprio
// para resolver isso, o jeito é usar um navegador de verdade: janela oculta
// carrega a URL, o Chromium resolve o desafio sozinho e a página vira o JSON.
function ehDesafioCloudflare(r) {
  if (r.status !== 403) return false
  return Boolean(r.headers.get("cf-mitigated")) || r.headers.get("server") === "cloudflare"
}

async function fetchViaJanela(url) {
  const { BrowserWindow } = require("electron")
  const win = new BrowserWindow({
    show: false,
    webPreferences: {
      partition: "persist:sources",
      contextIsolation: true,
      nodeIntegration: false,
    },
  })
  try {
    await win.loadURL(url)
    // O desafio recarrega a página sozinho ao passar: faz poll até o corpo
    // virar JSON (ou estourar o prazo).
    const limite = Date.now() + 45000
    while (Date.now() < limite) {
      const texto = await win.webContents
        .executeJavaScript("document.body ? document.body.innerText : ''")
        .catch(() => "")
      const s = String(texto || "").trim()
      if (s.startsWith("{") || s.startsWith("[")) {
        try {
          return JSON.parse(s)
        } catch {
          /* desafio ainda em andamento */
        }
      }
      await new Promise((r) => setTimeout(r, 1000))
    }
    throw new Error("desafio Cloudflare não resolvido (45s)")
  } finally {
    win.destroy()
  }
}

async function baixarFonte(src) {
  const headers = { "User-Agent": "arcadia" }
  if (src.etag) headers["If-None-Match"] = src.etag
  if (src.lastMod) headers["If-Modified-Since"] = src.lastMod
  const ses = sesFontes()
  const r = await fetchRede(src.url, {
    headers,
    signal: AbortSignal.timeout(60000),
    ...(ses ? { session: ses } : {}),
  })
  if (r.status === 304) return { mudou: false, count: src.count || 0 }
  if (!r.ok && !ehDesafioCloudflare(r)) throw new Error(`HTTP ${r.status}`)
  const data = r.ok ? await r.json() : await fetchViaJanela(src.url)
  const downloads = validar(data)
  if (!downloads) throw new Error("formato inválido (esperado { downloads: [...] })")
  fs.mkdirSync(SRC_DIR, { recursive: true })
  const tmp = cachePath(src.id) + ".tmp"
  fs.writeFileSync(tmp, JSON.stringify({ name: data.name || src.name || "", downloads }))
  fs.renameSync(tmp, cachePath(src.id))
  src.etag = r.headers.get("etag") || ""
  src.lastMod = r.headers.get("last-modified") || ""
  src.name = data.name || src.name || ""
  src.count = downloads.length
  return { mudou: true, count: downloads.length }
}

async function addSource(url) {
  url = String(url || "").trim()
  if (!/^https?:\/\//.test(url)) return { ok: false, error: "URL inválida" }
  const reg = readRegistry()
  if (reg.some((s) => s.url === url)) return { ok: false, error: "fonte já adicionada" }
  const src = { id: srcId(url), url, name: "", etag: "", lastMod: "", addedAt: Date.now() }
  try {
    await baixarFonte(src)
  } catch (e) {
    return { ok: false, error: String(e.message || e) }
  }
  reg.push(src)
  writeRegistry(reg)
  invalidarIndice() // índice mudou: reconstrói (em memória e disco)
  _ultimoArquivo = { id: "", data: null }
  return { ok: true, source: src }
}

function removeSource(id) {
  const reg = readRegistry()
  const rest = reg.filter((s) => s.id !== id)
  if (rest.length === reg.length) return { ok: false, error: "fonte não encontrada" }
  writeRegistry(rest)
  try {
    fs.rmSync(cachePath(id), { force: true })
  } catch {}
  invalidarIndice()
  _ultimoArquivo = { id: "", data: null }
  return { ok: true }
}

async function syncSources() {
  const reg = readRegistry()
  const out = []
  for (const src of reg) {
    try {
      const r = await baixarFonte(src)
      out.push({ id: src.id, ok: true, ...r })
    } catch (e) {
      out.push({ id: src.id, ok: false, error: String(e.message || e) })
    }
  }
  writeRegistry(reg) // grava etags novos mesmo quando nada mudou
  invalidarIndice()
  _ultimoArquivo = { id: "", data: null }
  return { ok: true, results: out }
}

// Índice leve: title/size/data + referência (fonte:pos). ~100 bytes por jogo
// contra MBs do JSON. Persistido em disco (sources_index.json) para a PRIMEIRA
// busca de cada abertura do app não refazer o download das fontes — a Steam
// também mantém o catálogo local, só revalida em segundo plano.
const INDEX_FILE = () => path.join(DATA_DIR, "sources_index.json")
let _index = null

// Quando as fontes mudam (add/remove/sync), o índice fica obsoleto: zera a
// memória e apaga o arquivo em disco para a próxima busca reconstruir.
function invalidarIndice() {
  _index = null
  try {
    fs.rmSync(INDEX_FILE(), { force: true })
  } catch {
    // se não der pra apagar, o loadIndex revalida em background
  }
}

async function carregarFonte(src) {
  const remoto = await catalogGet(`/catalog/v1/sources/${src.id}/games`)
  const data = remoto.data?.data
  if (data && Array.isArray(data.downloads)) return data
  try {
    return JSON.parse(fs.readFileSync(cachePath(src.id), "utf-8"))
  } catch {
    return null
  }
}

// Monta o índice a partir das fontes (servidor + cache local). Retorna o array.
async function construirIndex() {
  const index = []
  for (const src of readRegistry()) {
    const data = await carregarFonte(src)
    if (!data) continue
    const downloads = data?.downloads || []
    for (let i = 0; i < downloads.length; i++) {
      const d = downloads[i]
      if (!d?.title) continue
      index.push({
        ref: `${src.id}:${i}`,
        title: String(d.title),
        lower: String(d.title).toLowerCase(),
        fileSize: String(d.fileSize || "").trim(),
        uploadDate: String(d.uploadDate || "").trim(),
        src: src.name || data.name || src.id,
      })
    }
  }
  return index
}

// Lê o índice persistido em disco, se existir. Devolve null se ausente/velho.
function lerIndexDisco() {
  try {
    return JSON.parse(fs.readFileSync(INDEX_FILE(), "utf-8"))
  } catch {
    return null
  }
}

function gravarIndexDisco(index) {
  try {
    fs.mkdirSync(DATA_DIR, { recursive: true })
    const tmp = `${INDEX_FILE()}.tmp`
    fs.writeFileSync(tmp, JSON.stringify(index))
    fs.renameSync(tmp, INDEX_FILE())
  } catch {
    // índice em disco é otimização; falhar não quebra (reconstrói em memória)
  }
}

async function loadIndex() {
  if (_index) return _index
  // Se há um índice persistido, usa já (instantâneo) e revalida em background.
  const disco = lerIndexDisco()
  if (disco && Array.isArray(disco) && disco.length) {
    _index = disco
    // revalida em background SEM travar a busca: se as fontes mudaram,
    // a próxima busca pega o índice novo.
    construirIndex()
      .then((novo) => {
        if (novo.length) {
          _index = novo
          gravarIndexDisco(novo)
        }
      })
      .catch(() => {})
    return _index
  }
  _index = await construirIndex()
  if (_index.length) gravarIndexDisco(_index)
  return _index
}

// Casamento de título da loja PC. A release ("Far Cry 3 Free Download
// [Build-...]") e o título da loja ("Far Cry 3") casam por PALAVRA, não por
// substring: "ark" não pode casar "dark"/"shark". Sequência/numeral tem de
// bater (Far Cry 3 != Far Cry 2; Portal != Portal 2) e, quando o alvo é
// DLC/pack, o sufixo do DLC é exigido (todas as palavras do alvo presentes).
// Acento é dobrado (NFD: "Ragnarök" -> "ragnarok"), nunca removido, para
// "Ragnarok" achar "Ragnarök".
const STOPWORDS_TITULO = new Set([
  "the", "a", "an", "of", "and", "or", "to", "in", "on", "at", "for", "with",
  "from", "de", "da", "do", "das", "dos", "e", "y", "la", "el", "los", "las",
  "del", "le", "les", "des", "du",
])
const ROMANOS_TITULO = {
  i: 1, ii: 2, iii: 3, iv: 4, v: 5, vi: 6, vii: 7, viii: 8, ix: 9, x: 10,
  xi: 11, xii: 12, xiii: 13,
}
const NUMEROS_TITULO = {
  one: 1, two: 2, three: 3, four: 4, five: 5, six: 6, seven: 7, eight: 8,
  nine: 9, ten: 10,
}
const MARCA_VERSAO_TITULO = /^(?:v|ver|versao|version|build|patch|update|hotfix|rev|revision)\d*$/
const CONTEXTO_NUMERO_TITULO = /^(?:episode|ep|part|chapter|act|book|vol|volume|disc|disk)$/

function foldTitulo(value) {
  return String(value || "")
    .toLowerCase()
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/[\u00f8]/g, "o")
    .replace(/[\u00e6]/g, "ae")
    .replace(/[\u0153]/g, "oe")
    .replace(/[\u00df]/g, "ss")
    .replace(/[\u0142]/g, "l")
    .replace(/[\u0111]/g, "d")
    .replace(/['\u2019`\u00b4]/g, "")
}

function palavrasTitulo(value) {
  return foldTitulo(value)
    .split(/[^a-z0-9]+/)
    .filter(Boolean)
}

// Palavras exigidas + sequência do título. Numerais viram "#n" (romano e
// por-extenso contam como o mesmo número), versões ("v1.0.6",
// "(1.112.48699928)") são ignoradas e anos soltos seguem como palavra.
function canonicoFolded(fold) {
  const palavras = fold.split(/[^a-z0-9]+/).filter(Boolean)
  const seq = new Set()
  const exigidas = []
  let versao = false
  for (let i = 0; i < palavras.length; i++) {
    const palavra = palavras[i]
    const anterior = palavras[i - 1] || ""
    const proxima = palavras[i + 1] || ""
    // "v3.20" vira um token só ("v3" + "20"): marca de versão com dígitos
    // dentro também engole os números seguintes.
    if (MARCA_VERSAO_TITULO.test(palavra) && (/\d$/.test(palavra) || /^\d/.test(proxima))) {
      versao = true
      continue
    }
    if (versao && /^\d/.test(palavra)) continue
    versao = false
    let numero = 0
    if (/^\d+$/.test(palavra)) {
      // Números vizinhos ("1.112.48699928") são versão, não sequência.
      if (/^\d/.test(anterior) || /^\d/.test(proxima)) continue
      const n = Number(palavra)
      if (n >= 1 && n <= 99) numero = n
      else {
        exigidas.push(palavra)
        continue
      }
    } else if (ROMANOS_TITULO[palavra]) {
      // "I" solto é pronome ("I am Bread"), não numeral; só conta com
      // contexto de parte/episódio. "V"/"X" são sequência sempre.
      if (palavra.length > 1 || palavra === "v" || palavra === "x" || CONTEXTO_NUMERO_TITULO.test(anterior))
        numero = ROMANOS_TITULO[palavra]
    } else if (NUMEROS_TITULO[palavra] && CONTEXTO_NUMERO_TITULO.test(anterior)) {
      numero = NUMEROS_TITULO[palavra]
    }
    if (numero) {
      seq.add(numero)
      exigidas.push(`#${numero}`)
      continue
    }
    if (palavra.length < 2 || STOPWORDS_TITULO.has(palavra)) continue
    exigidas.push(palavra)
  }
  return { palavras, seq, exigidas, compacto: palavras.join("") }
}

function tituloCanonico(value) {
  return canonicoFolded(foldTitulo(value))
}

// null = não é o mesmo jogo. score: 5 exato cru, 4 exato dobrado, 3 prefixo,
// 2 contém a frase, 1 só as palavras; `extra` (palavras a mais) desempata.
// `alvoCanon`/`candFold` opcionais evitam refoldar em loop de busca.
function matchTitulo(alvo, candidato, alvoCanon, candFold) {
  const a = alvoCanon || tituloCanonico(alvo)
  const fold = candFold || foldTitulo(candidato)
  const c = canonicoFolded(fold)
  if (!a.palavras.length || !c.palavras.length) return null
  if (a.seq.size !== c.seq.size) return null
  for (const n of a.seq) if (!c.seq.has(n)) return null
  const candidatas = new Set(c.exigidas)
  for (const exigida of a.exigidas) if (!candidatas.has(exigida)) return null
  const cru = String(alvo || "").trim().toLowerCase() === String(candidato || "").trim().toLowerCase()
  let score = 1
  if (cru) score = 5
  else if (c.compacto === a.compacto) score = 4
  else if (a.compacto && c.compacto.startsWith(a.compacto)) score = 3
  else if (a.compacto && c.compacto.includes(a.compacto)) score = 2
  return { score, extra: Math.max(0, c.palavras.length - a.palavras.length) }
}

// Busca no índice com ranking ANTES do corte: a release certa pode estar em
// qualquer posição do índice (não depende do teto de resultados). Quem não é
// match de verdade entra só como fallback da busca livre (aba Fontes).
function buscarNoIndice(index, query, limit = 40) {
  const q = String(query || "").trim()
  if (!q) return []
  const qFold = foldTitulo(q)
  const qCompacto = qFold.replace(/[^a-z0-9]/g, "")
  const qCanon = canonicoFolded(qFold)
  const tokens = palavrasTitulo(q).filter((token) => token.length >= 3)
  const casados = []
  for (let i = 0; i < index.length; i++) {
    const g = index[i]
    const tituloFold = foldTitulo(g.title)
    const match = matchTitulo(q, g.title, qCanon, tituloFold)
    if (match) {
      casados.push({ g, score: match.score, extra: match.extra, i })
      continue
    }
    const tituloCompacto = tituloFold.replace(/[^a-z0-9]/g, "")
    const coincidentes = tokens.filter((token) => tituloCompacto.includes(token)).length
    const parcial =
      tituloFold.includes(qFold) ||
      (qCompacto && tituloCompacto.includes(qCompacto)) ||
      (tokens.length >= 2 && coincidentes >= Math.min(2, tokens.length))
    if (parcial) casados.push({ g, score: 0, extra: Number.MAX_SAFE_INTEGER, i })
  }
  casados.sort((a, b) => b.score - a.score || a.extra - b.extra || a.i - b.i)
  return casados.slice(0, limit).map(({ g }) => {
    const { lower, ...leve } = g
    return leve
  })
}

async function search(query, limit = 40) {
  const q = String(query || "").trim()
  if (!q) return []
  return buscarNoIndice(await loadIndex(), q, limit)
}

// Dados completos de um jogo (uris inclusas) — lê do disco só quando pedido.
// Cache da última fonte aberta: a busca de candidatos da loja vem agrupada
// por fonte (ordem do índice), então 1 slot pega quase todos os hits e
// evita re-parsear 10-15MB por candidato.
let _ultimoArquivo = { id: "", data: null }
async function getGame(ref) {
  const [id, i] = String(ref || "").split(":")
  try {
    if (_ultimoArquivo.id !== id) {
      const src = readRegistry().find((item) => item.id === id)
      if (!src) return { ok: false, error: "fonte nao encontrada" }
      const data = await carregarFonte(src)
      if (!data) return { ok: false, error: "catalogo da fonte indisponivel" }
      _ultimoArquivo = { id, data }
    }
    const d = _ultimoArquivo.data?.downloads?.[Number(i)]
    return d
      ? { ok: true, game: d, source: _ultimoArquivo.data.name || "" }
      : { ok: false, error: "jogo não encontrado" }
  } catch (e) {
    return { ok: false, error: String(e.message || e) }
  }
}

// escrita local usada pelo pull do sync (supabase/sources.js)
function _writeRegistryLocal(list) {
  writeRegistry(list)
}

module.exports = {
  addSource,
  removeSource,
  syncSources,
  search,
  getGame,
  list: readRegistry,
  _writeRegistryLocal,
  _foldTitulo: foldTitulo,
  _matchTitulo: matchTitulo,
  _buscarNoIndice: buscarNoIndice,
}
