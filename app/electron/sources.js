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
const os = require("os")
const crypto = require("crypto")
const { fetchRede } = require("./httpfetch")
const { catalogGet } = require("./catalog")

const { caminhoArquivoConta } = require("./supabase/conta")
const DATA_DIR = process.env.ARCADIA_DATA_DIR || path.join(os.homedir(), ".local/share/arcadia")
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
  const ok = downloads.filter((d) => d && d.title && (d.uris || d.uri))
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
  _index = null // invalida índice para reconstruir com a fonte nova
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
  _index = null
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
  _index = null
  _ultimoArquivo = { id: "", data: null }
  return { ok: true, results: out }
}

// Índice leve em RAM: title/size/data + referência (fonte:pos). Montado uma
// vez a partir dos caches em disco; ~100 bytes por jogo contra MBs do JSON.
let _index = null

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

async function loadIndex() {
  if (_index) return _index
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
  _index = index
  return _index
}

async function search(query, limit = 40) {
  const q = String(query || "")
    .trim()
    .toLowerCase()
  if (!q) return []
  const out = []
  for (const g of await loadIndex()) {
    if (g.lower.includes(q)) {
      const { lower, ...leve } = g
      out.push(leve)
      if (out.length >= limit) break
    }
  }
  return out
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
}
