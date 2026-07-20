// Loja Steam (estilo Acella): catálogo Hubcap + download via DepotDownloader
// (.NET) + registro do jogo na Steam (appmanifest.acf + SLSsteam config).
// Tudo é subprocess + parsing de texto — sem dependências novas de npm.

const fs = require("fs")
const path = require("path")
const os = require("os")
const { spawn, execFile } = require("child_process")

const DATA_DIR = path.join(os.homedir(), ".local", "share", "arcadia")
const BIN_DIR = path.join(DATA_DIR, "bin")
const DEPS_DIR = path.join(BIN_DIR, "deps", "depotdownloader")
const TMP_DIR = path.join(BIN_DIR, "tmp")
const LOG_DIR = path.join(DATA_DIR, "logs")
const CONFIG = path.join(DATA_DIR, "config.json")

const HUBCAP_BASE = "https://hubcapmanifest.com/api/v1"
// URLs dos provedores de manifesto, num lugar só: a busca (para saber se o
// jogo existe) e o download usam exatamente as mesmas — se divergirem, a busca
// promete um jogo que o download não consegue trazer.
const RYUU_URL = (appid) => `http://167.235.229.108/${appid}`
const SUSHI_URL = (appid) =>
  `https://raw.githubusercontent.com/sushi-dev55-alt/sushitools-games-repo-alt/refs/heads/main/${appid}.zip`
const TWENTYTWO_URL = (appid) => `https://api.twentytwocloud.com/download?appid=${appid}`

// Log diagnóstico da loja (restart de Steam, etc) em logs/store.log.
function storeLog(msg) {
  try {
    fs.mkdirSync(path.join(BIN_DIR, "..", "logs"), { recursive: true })
    fs.appendFileSync(path.join(BIN_DIR, "..", "logs", "store.log"), `[${new Date().toISOString()}] ${msg}\n`)
  } catch {}
}

function readConfig() {
  try {
    return JSON.parse(fs.readFileSync(CONFIG, "utf-8"))
  } catch {
    return {}
  }
}

// Todo acesso de rede da loja passa por aqui. O timeout padrão é a rede de
// segurança: sem ele um provedor que aceita a conexão e nunca responde
// pendura o handler IPC e a tela fica esperando para sempre. Quem precisa de
// mais tempo (download de zip) passa o próprio signal.
async function gh(url, opts = {}) {
  return fetch(url, {
    signal: AbortSignal.timeout(30000),
    ...opts,
    headers: { "User-Agent": "arcadia", ...(opts.headers || {}) },
  })
}

// ---------- .NET / DepotDownloader ----------

function dotnetBin() {
  const local = path.join(BIN_DIR, "dotnet", "dotnet")
  if (fs.existsSync(local)) return local
  return "dotnet" // PATH do sistema
}

async function ensureDotnet(onProgress) {
  try {
    execFile("dotnet", ["--version"], (e, stdout) => {})
    const ok = await new Promise((res) => execFile("dotnet", ["--version"], (e) => res(!e)))
    if (ok) return { ok: true, path: "dotnet" }
  } catch {}
  // Instala .NET 9 localmente via script oficial.
  const dir = path.join(BIN_DIR, "dotnet")
  fs.mkdirSync(dir, { recursive: true })
  const script = path.join(TMP_DIR, "dotnet-install.sh")
  fs.mkdirSync(TMP_DIR, { recursive: true })
  const r = await fetch("https://dot.net/v1/dotnet-install.sh")
  if (!r.ok) return { ok: false, error: `dotnet-install.sh HTTP ${r.status}` }
  fs.writeFileSync(script, Buffer.from(await r.arrayBuffer()))
  fs.chmodSync(script, 0o755)
  onProgress?.("Instalando .NET 9 (pode demorar)…")
  const code = await new Promise((res) => {
    const c = spawn("bash", [script, "--channel", "9.0", "--install-dir", dir], { stdio: "ignore" })
    c.on("close", res)
    c.on("error", () => res(1))
  })
  const bin = path.join(dir, "dotnet")
  return code === 0 && fs.existsSync(bin) ? { ok: true, path: bin } : { ok: false, error: "falha ao instalar o .NET" }
}

function depsOk() {
  return fs.existsSync(path.join(DEPS_DIR, "DepotDownloader.dll"))
}

// ---------- Disponibilidade entre provedores ----------
// A busca antes só enxergava o catálogo do Hubcap: um jogo que existe no Ryuu
// ou no Sushi aparecia como indisponível (ou nem aparecia), mesmo com o
// download funcionando. Aqui descobrimos em QUAIS provedores cada appid existe.

// O repositório do Sushi é um repo git plano de <appid>.zip: uma única chamada
// à árvore lista os ~5.800 de uma vez, muito mais barato que sondar um a um.
const SUSHI_TREE = "https://api.github.com/repos/sushi-dev55-alt/sushitools-games-repo-alt/git/trees/main"
const SUSHI_TTL = 6 * 60 * 60 * 1000 // 6h: o repo muda devagar
let sushiCache = { at: 0, ids: null }

async function sushiIds() {
  if (sushiCache.ids && Date.now() - sushiCache.at < SUSHI_TTL) return sushiCache.ids
  try {
    const r = await gh(SUSHI_TREE)
    if (!r.ok) throw new Error(`HTTP ${r.status}`)
    const d = await r.json()
    const ids = new Set()
    for (const n of d.tree || []) {
      const m = /^(\d+)\.zip$/.exec(n.path || "")
      if (m) ids.add(m[1])
    }
    // Árvore truncada devolveria um índice incompleto, marcando jogos que
    // existem como indisponíveis. Melhor não cachear e sondar por HEAD.
    if (d.truncated) throw new Error("árvore truncada")
    sushiCache = { at: Date.now(), ids }
    return ids
  } catch (e) {
    storeLog(`sushi: falha ao indexar (${e.message}) — caindo para HEAD`)
    return null
  }
}

// Sonda barata: HEAD devolve 200/404 sem baixar o zip. Cacheada por processo
// e em disco: a sondagem é o gargalo da loja e raramente muda.
const MANIFEST_CACHE = path.join(DATA_DIR, "store_manifest_cache.json")
const MANIFEST_TTL = 7 * 24 * 60 * 60 * 1000 // 7 dias: um jogo não muda de provedor da noite pro dia
const headCache = new Map()

function lerManifestCache() {
  try {
    const c = JSON.parse(fs.readFileSync(MANIFEST_CACHE, "utf-8"))
    if (c && typeof c === "object") return c
  } catch {}
  return {}
}

function gravarManifestCache(dados) {
  try {
    fs.mkdirSync(DATA_DIR, { recursive: true })
    // Limpa entradas vencidas para o arquivo não crescer para sempre.
    const agora = Date.now()
    for (const k of Object.keys(dados)) {
      if (agora - (dados[k].at || 0) > MANIFEST_TTL) delete dados[k]
    }
    const tmp = `${MANIFEST_CACHE}.tmp`
    fs.writeFileSync(tmp, JSON.stringify(dados))
    fs.renameSync(tmp, MANIFEST_CACHE)
  } catch {}
}

const manifestDiskCache = { at: 0, dados: {} }
function getManifestCache() {
  // Recarrega do disco a cada minuto para absorver outras instâncias/processos.
  if (Date.now() - manifestDiskCache.at > 60_000) {
    manifestDiskCache.dados = lerManifestCache()
    manifestDiskCache.at = Date.now()
  }
  return manifestDiskCache.dados
}

async function existe(url, timeoutMs = 6000) {
  if (headCache.has(url)) return headCache.get(url)
  const cache = getManifestCache()
  const guardado = cache[url]
  if (guardado && Date.now() - (guardado.at || 0) < MANIFEST_TTL) {
    headCache.set(url, guardado.ok)
    return guardado.ok
  }
  let ok = false
  let cacheavel = false
  try {
    const r = await gh(url, { method: "HEAD", signal: AbortSignal.timeout(timeoutMs) })
    ok = r.ok
    cacheavel = true
  } catch {
    ok = false
  }
  headCache.set(url, ok)
  if (cacheavel) {
    cache[url] = { at: Date.now(), ok }
    gravarManifestCache(cache)
  }
  return ok
}

// Roda as tarefas com concorrência limitada — 12 resultados × N provedores em
// paralelo total estouraria o servidor do Ryuu e travaria a busca.
async function emLotes(itens, limite, fn) {
  const out = []
  for (let i = 0; i < itens.length; i += limite) {
    out.push(...(await Promise.all(itens.slice(i, i + limite).map(fn))))
  }
  return out
}

// ── Tipo e capa oficiais (IStoreBrowseService) ─────────────────────────────
// É o endpoint que a própria loja da Steam usa. Público, aceita centenas de
// appids numa chamada e devolve, por item, o tipo (jogo, DLC, demo…) e o bloco
// de assets — inclusive a capa retrato COM o hash do caminho novo, que não dá
// para montar só com o appid.
const ITENS_URL = "https://api.steampowered.com/IStoreBrowseService/GetItems/v1/"
const ITENS_ASSETS = "https://shared.akamai.steamstatic.com/store_item_assets/"
const ITENS_CACHE = path.join(DATA_DIR, "store_items_cache.json")
const ITENS_TTL = 7 * 24 * 60 * 60 * 1000
const ITENS_MAX = 4000
const ITENS_LOTE = 100

// Enumeração da Steam. Só o 0 é um jogo que se instala; o resto é DLC (4),
// demo (1), software (6), trilha sonora (11), vídeo (12) e afins.
const TIPO_JOGO = 0

function capaDeAssets(a) {
  if (!a?.asset_url_format || !a.library_capsule) return ""
  return ITENS_ASSETS + a.asset_url_format.replace("${FILENAME}", a.library_capsule)
}

/**
 * Tipo e capa retrato de vários appids, em lote.
 *
 * Devolve `{ mapa, respondidos }`. `respondidos` são os ids sobre os quais
 * temos uma resposta confiável — os que estão nele mas fora do mapa são os que
 * a Steam não reconhece (jogo removido da loja). A distinção existe porque um
 * lote que falhou por rede não pode ser confundido com "não existe".
 */
async function itensDaLoja(appids) {
  const ids = [...new Set(appids.map((a) => String(a)).filter(Boolean))]
  const mapa = new Map()
  const respondidos = new Set()
  if (!ids.length) return { mapa, respondidos }

  const cache = lerCache(ITENS_CACHE) || {}
  const agora = Date.now()
  const faltando = []
  for (const id of ids) {
    const it = cache[id]
    if (it && agora - it.at < ITENS_TTL) {
      respondidos.add(id)
      if (typeof it.tipo === "number") mapa.set(id, { tipo: it.tipo, capa: it.capa || "" })
    } else faltando.push(id)
  }
  if (!faltando.length) return { mapa, respondidos }

  let mudou = false
  for (let i = 0; i < faltando.length; i += ITENS_LOTE) {
    const lote = faltando.slice(i, i + ITENS_LOTE)
    try {
      const input = {
        ids: lote.map((id) => ({ appid: Number(id) })),
        context: { language: "portuguese", country_code: "BR", steam_realm: 1 },
        data_request: { include_assets: true },
      }
      const r = await gh(`${ITENS_URL}?input_json=${encodeURIComponent(JSON.stringify(input))}`, {
        signal: AbortSignal.timeout(20000),
      })
      if (!r.ok) continue
      const j = await r.json()
      // O lote respondeu: tudo que pedimos nele tem veredito, inclusive quem
      // voltou sem tipo (fora da loja) e quem não voltou.
      for (const id of lote) {
        respondidos.add(id)
        cache[id] = { at: agora }
      }
      for (const it of j?.response?.store_items || []) {
        const id = String(it.appid || "")
        if (!id || typeof it.type !== "number") continue
        const dado = { tipo: it.type, capa: capaDeAssets(it.assets) }
        mapa.set(id, dado)
        cache[id] = { ...dado, at: agora }
      }
      mudou = true
    } catch {
      // Rede fora ou timeout: estes ids ficam SEM veredito e passam pelo
      // filtro. É melhor deixar um DLC escapar do que esvaziar a tela porque
      // uma consulta não respondeu.
    }
  }

  if (mudou) {
    const chaves = Object.keys(cache)
    if (chaves.length > ITENS_MAX) {
      chaves
        .sort((a, b) => (cache[a].at || 0) - (cache[b].at || 0))
        .slice(0, chaves.length - ITENS_MAX)
        .forEach((k) => delete cache[k])
    }
    gravarCache(ITENS_CACHE, cache)
  }
  return { mapa, respondidos }
}

/**
 * Prepara uma página de resultados: completa a capa retrato, tira o que não é
 * jogo e sonda os manifestos. A ordem importa — sondar antes de filtrar
 * gastaria uma requisição de manifesto por DLC.
 *
 * Só remove quem a Steam classificou explicitamente como outra coisa. Se a
 * consulta falhar, a lista passa inteira.
 */
async function preparar(jogos, jaTem = new Set()) {
  const { mapa, respondidos } = await itensDaLoja(jogos.map((g) => g.appid))
  const filtrados = []
  for (const g of jogos) {
    const id = String(g.appid)
    const it = mapa.get(id)
    if (it) {
      if (it.tipo !== TIPO_JOGO) continue // DLC, demo, trilha sonora, software…
      if (it.capa) g.capa = it.capa
    } else if (respondidos.has(id)) {
      continue // a Steam respondeu e não conhece: removido da loja
    }
    filtrados.push(g)
  }
  await marcarDisponibilidade(filtrados, jaTem)
  return filtrados
}

// Marca cada jogo com os provedores onde o manifesto existe.
// `jaTem` traz os appids que o Hubcap já confirmou (não precisam de sonda).
async function marcarDisponibilidade(jogos, jaTem = new Set()) {
  const sushi = await sushiIds()
  await emLotes(jogos, 6, async (g) => {
    const fontes = []
    if (jaTem.has(g.appid)) fontes.push("Morrenus")
    if (sushi ? sushi.has(g.appid) : await existe(SUSHI_URL(g.appid))) fontes.push("Sushi")
    if (await existe(RYUU_URL(g.appid))) fontes.push("Ryuu")
    g.fontes = fontes
    g.manifest = fontes.length > 0
    return g
  })
  return jogos
}

// ---------- Hubcap (catálogo + manifestos) ----------

function mapJogos(data) {
  return (data.games || [])
    .map((g) => ({
      appid: String(g.game_id || ""),
      title: g.game_name || "",
      cover: g.header_image || "",
      manifest: Boolean(g.manifest_available),
    }))
    .filter((g) => g.appid && g.title)
}

// Relevância: a Steam devolve por popularidade, o que joga o jogo exato para
// baixo quando ele tem DLCs/trilhas. Ordenamos por quão bem o título casa com
// o que foi digitado, e só então por disponibilidade de manifesto.
function relevancia(titulo, q) {
  const t = titulo.toLowerCase().trim()
  const s = q.toLowerCase().trim()
  if (t === s) return 0
  if (t.startsWith(s)) return 1
  if (new RegExp(`\\b${s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`).test(t)) return 2
  if (t.includes(s)) return 3
  return 4
}

// Conteúdo que não é jogo jogável: some no meio dos resultados e raramente é o
// que se procura, então vai para o fim (nunca é escondido).
const ACESSORIO =
  /\b(ost|soundtrack|artbook|art book|digital (comic|artbook|deluxe)|wallpaper|demo|playtest|beta|trilha sonora|skin|costume|avatar|emote|upgrade pack)\b/i

function ordenar(jogos, q) {
  return jogos.sort((a, b) => {
    const ac = ACESSORIO.test(a.title) ? 1 : 0
    const bc = ACESSORIO.test(b.title) ? 1 : 0
    if (ac !== bc) return ac - bc
    const ar = relevancia(a.title, q)
    const br = relevancia(b.title, q)
    if (ar !== br) return ar - br
    return Number(b.manifest) - Number(a.manifest)
  })
}

// Sugestões enquanto digita: SÓ a lista de títulos da Steam, sem sondar
// provedor nenhum. A busca completa leva 1–2s porque confere a disponibilidade
// de cada resultado; usá-la a cada tecla disparava dezenas de requisições ao
// Ryuu e as respostas chegavam fora de ordem. Aqui é uma chamada só.
const sugCache = new Map()
async function suggest(query) {
  const q = query.trim()
  if (q.length < 2) return { ok: true, jogos: [] }
  const chave = q.toLowerCase()
  if (sugCache.has(chave)) return { ok: true, jogos: sugCache.get(chave) }
  try {
    const r = await gh(
      `https://store.steampowered.com/api/storesearch/?term=${encodeURIComponent(q)}&cc=br&l=portuguese`,
      { signal: AbortSignal.timeout(6000) },
    )
    if (!r.ok) return { ok: false, error: `Steam HTTP ${r.status}` }
    const data = await r.json()
    const jogos = ordenar(
      (data.items || [])
        .map((g) => ({ appid: String(g.id || ""), title: g.name || "" }))
        .filter((g) => g.appid && g.title),
      q,
    ).slice(0, 8)
    // Digitar volta atrás (backspace) e repetir termos é comum; o cache evita
    // repetir a chamada. Limitado para não crescer sem fim numa sessão longa.
    if (sugCache.size > 100) sugCache.clear()
    sugCache.set(chave, jogos)
    return { ok: true, jogos }
  } catch (e) {
    return { ok: false, error: String(e.message || e) }
  }
}

// Busca unindo TODAS as fontes. Antes o catálogo do Hubcap era a única lista
// consultada, então um jogo presente no Ryuu/Sushi não aparecia — mesmo com o
// download funcionando perfeitamente por eles. Agora a Steam dá a lista de
// títulos (catálogo completo, sem key) e cada resultado é conferido contra
// todos os provedores.
async function search(query) {
  const cfg = readConfig()
  const porId = new Map()
  const erros = []
  const comHubcap = new Set()

  // 1) Hubcap: melhores metadados (capa oficial) e já diz o que ele tem.
  if (cfg.hubcap_api_key) {
    try {
      const r = await gh(`${HUBCAP_BASE}/library?search=${encodeURIComponent(query)}`, {
        headers: { Authorization: `Bearer ${cfg.hubcap_api_key}` },
      })
      if (r.ok) {
        for (const g of mapJogos(await r.json())) {
          if (g.manifest) comHubcap.add(g.appid)
          porId.set(g.appid, g)
        }
      } else {
        erros.push(`Hubcap HTTP ${r.status}`)
      }
    } catch (e) {
      erros.push(`Hubcap: ${e.message}`)
    }
  }

  // 2) Steam: catálogo completo e sem chave. É o que garante encontrar jogos
  // que o Hubcap não indexa mas os outros provedores servem.
  try {
    const r = await gh(
      `https://store.steampowered.com/api/storesearch/?term=${encodeURIComponent(query)}&cc=br&l=portuguese`,
    )
    if (r.ok) {
      const data = await r.json()
      for (const g of data.items || []) {
        const appid = String(g.id || "")
        if (!appid || !g.name || porId.has(appid)) continue
        porId.set(appid, {
          appid,
          title: g.name,
          // O `tiny_image` é pequeno, mas vem com o hash do asset — para jogos
          // do esquema novo é a ÚNICA arte alcançável sem uma chamada extra.
          // O caminho antigo, montado só com o appid, fica de reserva.
          cover: g.tiny_image || `https://cdn.cloudflare.steamstatic.com/steam/apps/${appid}/header.jpg`,
          preco: precoBusca(g.price),
          manifest: false,
        })
      }
    } else {
      erros.push(`Steam HTTP ${r.status}`)
    }
  } catch (e) {
    erros.push(`Steam: ${e.message}`)
  }

  const jogos = [...porId.values()]
  if (!jogos.length) {
    return { ok: false, error: erros.join(" · ") || "nenhum resultado" }
  }
  const encontrados = await preparar(jogos, comHubcap)
  ordenar(encontrados, query)
  return { ok: true, jogos: encontrados, fonte: "multi", avisos: erros }
}

// Lançamentos/adicionados recentemente no catálogo (home da aba Lojas).
async function recent(limit = 24) {
  const cfg = readConfig()
  if (!cfg.hubcap_api_key) return { ok: false, error: "sem API key" }
  const r = await gh(`${HUBCAP_BASE}/library?limit=${limit}&sort_by=updated`, {
    headers: { Authorization: `Bearer ${cfg.hubcap_api_key}` },
  })
  if (!r.ok) return { ok: false, error: `Hubcap HTTP ${r.status}` }
  return { ok: true, jogos: mapJogos(await r.json()).filter((g) => g.manifest) }
}

// "Mais baixados/em alta" (o Hubcap não tem ranking): SteamSpy top 100 das
// últimas 2 semanas — sem API key.
// Cache em disco do "Em alta": a lista custa ~3s (SteamSpy + 24 sondagens de
// disponibilidade) e mudava a cada abertura da aba, deixando a tela vazia. Em
// disco, e não em memória, para sobreviver ao fechar o app — é justamente a
// primeira abertura que doía.
const POPULAR_CACHE = path.join(DATA_DIR, "store_popular_cache.json")
const POPULAR_TTL = 6 * 60 * 60 * 1000 // 6h: "top da quinzena" muda devagar

function lerPopularCache() {
  try {
    const c = JSON.parse(fs.readFileSync(POPULAR_CACHE, "utf-8"))
    if (Array.isArray(c.jogos) && c.jogos.length) return c
  } catch {}
  return null
}

async function buscarPopular(lista = "top100in2weeks") {
  const r = await gh(`https://steamspy.com/api.php?request=${lista}`, {
    signal: AbortSignal.timeout(15000),
  })
  if (!r.ok) throw new Error(`SteamSpy HTTP ${r.status}`)
  const data = await r.json()
  const completa = ordenarPorPopularidade(Object.values(data))
    .map((g) => ({
      appid: String(g.appid || ""),
      title: g.name || "",
      cover: `https://cdn.akamai.steamstatic.com/steam/apps/${g.appid}/header.jpg`,
      manifest: false,
    }))
    .filter((g) => g.appid && g.title)
  // Cache guarda a lista COMPLETA (dump SteamSpy inteiro). A sondagem —
  // parte cara — acontece só na fatia servida por popular(), não aqui.
  try {
    fs.mkdirSync(DATA_DIR, { recursive: true })
    fs.writeFileSync(POPULAR_CACHE, JSON.stringify({ at: Date.now(), completa }))
  } catch {}
  return completa
}

// Entrega o cache na hora e, se estiver velho, atualiza em segundo plano
// (stale-while-revalidate): a aba abre instantânea e o conteúdo se renova
// sozinho para a próxima vez, em vez de fazer o usuário esperar.
// `lista` escolhe entre a quinzena (Em alta) e o acumulado (Mais jogados).
let popularEmVoo = null
async function popular(lista = "top100in2weeks", limite = 40, offset = 0) {
  const off = Math.max(0, Number(offset) | 0)
  const lim = Math.max(1, Number(limite) | 0)
  // As listas alternativas (top100forever, etc.) vivem no cache genérico,
  // uma entrada por lista, com prefixo "__" para não colidir com gêneros.
  if (lista !== "top100in2weeks") {
    const cache = lerCache(GENERO_CACHE) || {}
    const chave = `__${lista}`
    const guardado = cache[chave]
    let completa =
      guardado && Date.now() - (guardado.at || 0) < GENERO_TTL && Array.isArray(guardado.completa)
        ? guardado.completa
        : null
    if (!completa) {
      try {
        completa = await buscarSteamSpyCompleta(`https://steamspy.com/api.php?request=${lista}`)
        cache[chave] = { at: Date.now(), completa }
        gravarCache(GENERO_CACHE, cache)
      } catch (e) {
        if (guardado && Array.isArray(guardado.jogos)) {
          const fatia = guardado.jogos.slice(off, off + lim)
          return { ok: true, jogos: fatia, offset: off, total: guardado.jogos.length, cache: true, velho: true }
        }
        return { ok: false, error: String(e.message || e) }
      }
    }
    const fatia = await preparar(completa.slice(off, off + lim))
    return { ok: true, jogos: fatia, offset: off, total: completa.length }
  }
  // "Em alta": cache dedicado com stale-while-revalidate. Guardamos a lista
  // completa e paginamos aqui; a revalidação em voo continua invisível.
  const c = lerPopularCache()
  const velho = !c || Date.now() - (c.at || 0) > POPULAR_TTL
  const servir = async (completa) => {
    const fatia = await preparar(completa.slice(off, off + lim))
    return { jogos: fatia, offset: off, total: completa.length }
  }
  const completaDo = (c) => (Array.isArray(c?.completa) ? c.completa : Array.isArray(c?.jogos) ? c.jogos : null)
  if (c && !velho) {
    const completa = completaDo(c)
    if (completa) {
      const s = await servir(completa)
      return { ok: true, ...s, cache: true }
    }
  }
  if (!popularEmVoo) {
    popularEmVoo = buscarPopular().finally(() => {
      popularEmVoo = null
    })
  }
  const completaCache = completaDo(c)
  if (completaCache) {
    popularEmVoo.catch(() => {}) // renova em segundo plano; erro não interessa
    const s = await servir(completaCache)
    return { ok: true, ...s, cache: true, revalidando: true }
  }
  try {
    const completa = await popularEmVoo
    const s = await servir(completa)
    return { ok: true, ...s }
  } catch (e) {
    return { ok: false, error: String(e.message || e) }
  }
}

// O SteamSpy devolve um OBJETO indexado por appid. Chaves que parecem inteiro
// são reordenadas numericamente pelo JS, então Object.values() entregava a
// lista em ordem de appid — o "Em alta" abria com Counter-Strike (10) e
// Half-Life (70), não com o que está sendo jogado. `ccu` é o número de
// jogadores simultâneos, que é exatamente o critério que se quer.
function ordenarPorPopularidade(lista) {
  return [...lista].sort((a, b) => (Number(b.ccu) || 0) - (Number(a.ccu) || 0))
}

// ---------- Loja do modo console ----------
// Cache em disco parametrizado. O "Em alta" acima já tinha o seu; com três
// caches diferentes (populares, detalhes, gêneros), copiar a lógica em cada um
// só convida a divergirem.
function lerCache(arquivo) {
  try {
    return JSON.parse(fs.readFileSync(arquivo, "utf-8"))
  } catch {
    return null
  }
}

function gravarCache(arquivo, dados) {
  try {
    fs.mkdirSync(DATA_DIR, { recursive: true })
    const tmp = `${arquivo}.tmp`
    fs.writeFileSync(tmp, JSON.stringify(dados))
    fs.renameSync(tmp, arquivo) // atômico: nunca deixa o cache pela metade
  } catch {}
}

// ── Capa alternativa (SteamGridDB) ─────────────────────────────────────────
// Só é consultada para os jogos cuja arte retrato a Steam não publica — o que
// hoje significa quase todo lançamento recente e DLC. A busca é por appid da
// Steam, sem procurar por título, então não há risco de trazer a capa do jogo
// errado.
const CAPA_CACHE = path.join(DATA_DIR, "store_cover_cache.json")
const CAPA_TTL = 30 * 24 * 60 * 60 * 1000 // 30 dias: arte da comunidade muda devagar
// Quem não tem capa hoje provavelmente não terá amanhã, mas pode ganhar uma
// depois do lançamento. Um TTL curto para o "não achei" evita repetir a
// chamada a cada rolagem sem congelar a ausência para sempre.
const CAPA_TTL_VAZIO = 24 * 60 * 60 * 1000
const CAPA_MAX = 800

// Pedidos em voo, por appid: a mesma capa costuma ser pedida por vários
// ladrilhos ao mesmo tempo (grade + trilho), e sem isto viraria uma chamada
// por ladrilho.
const capasEmVoo = new Map()

async function capaAlternativa(appid) {
  const id = String(appid || "").trim()
  if (!id) return { ok: false, error: "appid vazio" }

  const cache = lerCache(CAPA_CACHE) || {}
  const item = cache[id]
  if (item) {
    const ttl = item.url ? CAPA_TTL : CAPA_TTL_VAZIO
    if (Date.now() - item.at < ttl) return { ok: true, url: item.url, cache: true }
  }

  const chave = String(readConfig().steamgriddb_api_key || "").trim()
  if (!chave) return { ok: true, url: "", semChave: true }

  if (capasEmVoo.has(id)) return capasEmVoo.get(id)

  const pedido = (async () => {
    let url = ""
    try {
      const p = new URLSearchParams({ dimensions: "600x900,660x930", types: "static", nsfw: "false" })
      const r = await fetch(`https://www.steamgriddb.com/api/v2/grids/steam/${id}?${p}`, {
        headers: { Authorization: `Bearer ${chave}` },
        signal: AbortSignal.timeout(12000),
      })
      // 404 = "essa Steam appid não existe na SGDB". É resposta legítima, não
      // erro: guardamos o vazio para não perguntar de novo a cada rolagem.
      if (r.ok) {
        const j = await r.json()
        url = (j?.data || []).find((g) => g.url)?.url || ""
      }
    } catch {
      // Rede fora ou timeout: não grava nada, para tentar de novo depois.
      capasEmVoo.delete(id)
      return { ok: true, url: "" }
    }

    const atual = lerCache(CAPA_CACHE) || {}
    atual[id] = { url, at: Date.now() }
    const chaves = Object.keys(atual)
    if (chaves.length > CAPA_MAX) {
      // Descarta as mais antigas primeiro: o arquivo é lido inteiro a cada uso.
      chaves
        .sort((a, b) => (atual[a].at || 0) - (atual[b].at || 0))
        .slice(0, chaves.length - CAPA_MAX)
        .forEach((k) => delete atual[k])
    }
    gravarCache(CAPA_CACHE, atual)
    capasEmVoo.delete(id)
    return { ok: true, url }
  })()

  capasEmVoo.set(id, pedido)
  return pedido
}

const DETALHES_CACHE = path.join(DATA_DIR, "store_details_cache.json")
const DETALHES_TTL = 24 * 60 * 60 * 1000
const DETALHES_MAX = 300 // teto de entradas: o arquivo é lido inteiro a cada uso

// A Steam migrou os `movies` do appdetails para DASH/HLS, que o Chromium não
// reproduz. Os MP4 legados do CDN continuam sendo servidos e tocam num <video>
// comum — é por eles que o trailer da loja funciona sem biblioteca extra.
const TRAILER_URL = (movieId, alta) =>
  `https://cdn.akamai.steamstatic.com/steam/apps/${movieId}/${alta ? "movie_max" : "movie480"}.mp4`

// pc_requirements vem como objeto com HTML, ou como array vazio quando o jogo
// não declara nada — daí a checagem de Array.
function requisito(reqs, chave) {
  if (!reqs || Array.isArray(reqs)) return ""
  return String(reqs[chave] || "")
}

function normalizaDetalhes(appid, d) {
  const filme = (d.movies || [])[0]
  return {
    appid: String(appid),
    nome: d.name || "",
    descricao: d.short_description || "",
    header: d.header_image || "",
    fundo: d.background_raw || d.background || "",
    screenshots: (d.screenshots || []).map((s) => s.path_full).filter(Boolean).slice(0, 12),
    trailer: filme
      ? { url: TRAILER_URL(filme.id, false), alta: TRAILER_URL(filme.id, true), poster: filme.thumbnail || "" }
      : null,
    generos: (d.genres || []).map((g) => g.description).filter(Boolean),
    lancamento: d.release_date?.date || "",
    devs: d.developers || [],
    publishers: d.publishers || [],
    preco: d.price_overview?.final_formatted || (d.is_free ? "Gratuito" : ""),
    // Só faz sentido mostrar o preço cheio riscado quando há desconto de fato.
    precoOriginal: d.price_overview?.discount_percent ? d.price_overview.initial_formatted || "" : "",
    desconto: Number(d.price_overview?.discount_percent) || 0,
    metacritic: Number(d.metacritic?.score) || 0,
    reqMin: requisito(d.pc_requirements, "minimum"),
    reqRec: requisito(d.pc_requirements, "recommended"),
  }
}

// Ficha do jogo para a página da loja. O appdetails tem limite de requisições
// (~200 a cada 5 min), então nunca deve ser chamado para uma linha inteira —
// só ao abrir a página ou ao entrar no destaque.
async function detalhes(appid) {
  const id = String(appid || "")
  if (!id) return { ok: false, error: "appid ausente" }
  const cache = lerCache(DETALHES_CACHE) || {}
  const guardado = cache[id]
  if (guardado && Date.now() - (guardado.at || 0) < DETALHES_TTL) {
    return { ok: true, jogo: guardado.jogo, cache: true }
  }
  try {
    const r = await gh(
      `https://store.steampowered.com/api/appdetails?appids=${id}&cc=br&l=portuguese`,
      { signal: AbortSignal.timeout(20000) },
    )
    if (!r.ok) throw new Error(`Steam HTTP ${r.status}`)
    const data = await r.json()
    const d = data?.[id]?.data
    if (!d) throw new Error("sem dados para este appid")
    const jogo = normalizaDetalhes(id, d)
    cache[id] = { at: Date.now(), jogo }
    // Poda pelas entradas mais antigas quando passa do teto.
    const ids = Object.keys(cache)
    if (ids.length > DETALHES_MAX) {
      ids.sort((a, b) => (cache[a].at || 0) - (cache[b].at || 0))
      for (const velho of ids.slice(0, ids.length - DETALHES_MAX)) delete cache[velho]
    }
    gravarCache(DETALHES_CACHE, cache)
    return { ok: true, jogo }
  } catch (e) {
    // Cache vencido ainda serve: melhor uma ficha de ontem que uma tela vazia.
    if (guardado) return { ok: true, jogo: guardado.jogo, cache: true, velho: true }
    return { ok: false, error: String(e.message || e) }
  }
}

const DESTAQUE_CACHE = path.join(DATA_DIR, "store_featured_cache.json")
const DESTAQUE_TTL = 3 * 60 * 60 * 1000 // 3h: lançamentos e promoções giram rápido

// Seções da vitrine oficial da Steam. Vêm todas numa resposta só, então uma
// chamada abastece as quatro linhas.
const SECOES = new Set(["new_releases", "top_sellers", "specials", "coming_soon"])

// Os itens do featuredcategories usam `id`/`name`, e não `appid`/`title` como o
// resto da loja. Normalizar aqui evita que o carrossel receba capa vazia.
// Preço em centavos + moeda ISO, como o featuredcategories devolve. Vem na
// moeda da região, então não precisamos converter nada — só formatar.
function precoDestaque(centavos, moeda) {
  if (typeof centavos !== "number" || !moeda) return ""
  if (centavos === 0) return "Gratuito"
  try {
    return new Intl.NumberFormat(undefined, { style: "currency", currency: moeda }).format(centavos / 100)
  } catch {
    return ""
  }
}

// O storesearch usa outro formato de preço: { currency, initial, final }.
function precoBusca(p) {
  if (!p) return ""
  return precoDestaque(p.final, p.currency)
}

function mapDestaque(itens) {
  return (itens || [])
    .map((g) => ({
      appid: String(g.id || ""),
      title: g.name || "",
      // A URL vem PRONTA da API, com o hash do asset no caminho. Jogos
      // publicados no esquema novo (/store_item_assets/steam/apps/<id>/<hash>/)
      // não são alcançáveis pelo caminho antigo montado só com o appid — era
      // por isso que quase todo lançamento recente aparecia sem capa.
      cover: g.header_image || g.large_capsule_image || `https://cdn.akamai.steamstatic.com/steam/apps/${g.id}/header.jpg`,
      manifest: false,
      desconto: Number(g.discount_percent) || 0,
      // A vitrine mostra preço por capa; esta é a única fonte que o entrega
      // sem uma chamada de appdetails por jogo (que estouraria o limite).
      preco: precoDestaque(g.final_price, g.currency),
      precoOriginal: g.discount_percent ? precoDestaque(g.original_price, g.currency) : "",
    }))
    .filter((g) => g.appid && g.title)
}

// Complemento SteamSpy por seção da Featured. Quando o cliente pede offset
// além dos ~20 que a Steam devolve, servimos jogos populares da SteamSpy
// para o scroll infinito continuar. coming_soon não tem equivalente
// natural (jogos futuros); esgota naturalmente.
const COMPLEMENTO = {
  top_sellers: "top100forever",
  new_releases: "top100forever",
  specials: "top100forever",
}

// Uma seção da vitrine (lançamentos, mais vendidos, promoções, em breve).
// Cacheamos a lista COMPLETA da Steam em `.completa` e um complemento
// SteamSpy em `.complemento`; paginamos aqui. Cliente recebe uma fatia
// contígua — a transição Steam→SteamSpy é transparente.
async function destaques(secao, limite = 40, offset = 0) {
  const chave = String(secao || "")
  if (!SECOES.has(chave)) return { ok: false, error: `seção inválida: ${chave}` }
  const off = Math.max(0, Number(offset) | 0)
  const lim = Math.max(1, Number(limite) | 0)
  const cache = lerCache(DESTAQUE_CACHE) || {}
  let guardado = cache[chave]
  const validoSteam = (g) => g && Date.now() - (g.at || 0) < DESTAQUE_TTL && Array.isArray(g.completa)
  if (!validoSteam(guardado)) {
    try {
      const r = await gh("https://store.steampowered.com/api/featuredcategories?cc=br&l=portuguese", {
        signal: AbortSignal.timeout(20000),
      })
      if (!r.ok) throw new Error(`Steam HTTP ${r.status}`)
      const data = await r.json()
      const agora = Date.now()
      // A resposta traz todas as seções de uma vez; preenchemos o cache de
      // todas para não pagar essa chamada de novo ao trocar de filtro. Cada
      // entrada guarda a lista completa (sem slice); sondagem só na fatia
      // servida abaixo.
      for (const s of SECOES) {
        const itens = mapDestaque(data?.[s]?.items)
        const anterior = cache[s] || {}
        cache[s] = { at: agora, completa: itens, complemento: anterior.complemento }
      }
      gravarCache(DESTAQUE_CACHE, cache)
      guardado = cache[chave]
      if (!guardado) throw new Error("seção vazia na resposta")
    } catch (e) {
      // Cache velho ainda serve: melhor uma tela pronta que um erro. Aceita
      // formato novo (`.completa`) ou antigo (`.jogos`).
      const fallback = guardado?.completa || guardado?.jogos
      if (Array.isArray(fallback)) {
        const fatia = fallback.slice(off, Math.min(fallback.length, off + lim))
        return { ok: true, jogos: fatia, offset: off, total: fallback.length, cache: true, velho: true }
      }
      return { ok: false, error: String(e.message || e) }
    }
  }
  const steamCompleta = guardado.completa
  const fimSteam = steamCompleta.length
  const nomeComp = COMPLEMENTO[chave]
  // Se o cliente ainda está dentro da faixa da Steam e não precisa transbordar,
  // servimos direto.
  if (off + lim <= fimSteam || !nomeComp) {
    const fatia = await preparar(steamCompleta.slice(off, Math.min(fimSteam, off + lim)))
    // Sem complemento OU pedido exatamente dentro: `total` é o que temos hoje.
    // Se há complemento e ele ainda não foi carregado, avisamos com hasMore.
    const total = nomeComp ? fimSteam + (guardado.complemento?.length || 0) : fimSteam
    return { ok: true, jogos: fatia, offset: off, total, hasMoreLazy: Boolean(nomeComp && !guardado.complemento) }
  }
  // Cliente pediu além da Steam — precisa complementar com SteamSpy. Buscamos
  // o dump completo do complemento uma vez, cacheamos em `.complemento`.
  let complemento = Array.isArray(guardado.complemento) ? guardado.complemento : null
  if (!complemento) {
    try {
      complemento = await buscarSteamSpyCompleta(`https://steamspy.com/api.php?request=${nomeComp}`)
      // Remove appids já presentes na parte Steam para o cliente não ver o
      // mesmo card duas vezes na transição.
      const jaVi = new Set(steamCompleta.map((g) => g.appid))
      complemento = complemento.filter((g) => !jaVi.has(g.appid))
      cache[chave] = { ...guardado, complemento }
      gravarCache(DESTAQUE_CACHE, cache)
    } catch (e) {
      // Complemento falhou: entrega o que tem da Steam (potencialmente vazio)
      // e sinaliza total=fimSteam para o cliente parar de pedir.
      const fatia = steamCompleta.slice(off, Math.min(fimSteam, off + lim))
      return { ok: true, jogos: fatia, offset: off, total: fimSteam, cache: true, velho: true, erroComplemento: String(e.message || e) }
    }
  }
  // Fatia contígua atravessando os dois arrays: pega o que ainda cabe da Steam
  // (se houver) e o resto do complemento, ajustando o índice do segundo.
  const jogos = []
  if (off < fimSteam) jogos.push(...steamCompleta.slice(off, fimSteam))
  const inicioComp = Math.max(0, off - fimSteam)
  const restante = lim - jogos.length
  if (restante > 0) jogos.push(...complemento.slice(inicioComp, inicioComp + restante))
  const prontos = await preparar(jogos)
  return { ok: true, jogos: prontos, offset: off, total: fimSteam + complemento.length }
}

const GENERO_CACHE = path.join(DATA_DIR, "store_genre_cache.json")
const GENERO_TTL = 12 * 60 * 60 * 1000

// Busca o dump inteiro de um endpoint SteamSpy e retorna a lista completa
// (sem sondar). Reusado por porGenero e popular para separar "busca +
// cache" de "sondagem da fatia servida" — a sondagem é o custo real
// (um HEAD por jogo em cada provedor), então só rodamos na página pedida.
async function buscarSteamSpyCompleta(url) {
  const r = await gh(url, { signal: AbortSignal.timeout(25000) })
  if (!r.ok) throw new Error(`SteamSpy HTTP ${r.status}`)
  const data = await r.json()
  return ordenarPorPopularidade(Object.values(data))
    .map((g) => ({
      appid: String(g.appid || ""),
      title: g.name || "",
      cover: `https://cdn.akamai.steamstatic.com/steam/apps/${g.appid}/header.jpg`,
      manifest: false,
    }))
    .filter((g) => g.appid && g.title)
}

// Uma linha da home, por gênero. Cacheamos a lista COMPLETA do SteamSpy
// (dezenas de milhares) e paginamos aqui — assim scroll infinito pede a
// próxima fatia sem tocar na rede, e só a fatia servida paga o custo de
// sondagem. Caches antigos (só `.jogos`) são migrados sob demanda.
async function porGenero(genero, limite = 40, offset = 0) {
  const chave = String(genero || "").trim()
  if (!chave) return { ok: false, error: "gênero ausente" }
  const off = Math.max(0, Number(offset) | 0)
  const lim = Math.max(1, Number(limite) | 0)
  const cache = lerCache(GENERO_CACHE) || {}
  const guardado = cache[chave]
  let completa =
    guardado && Date.now() - (guardado.at || 0) < GENERO_TTL && Array.isArray(guardado.completa)
      ? guardado.completa
      : null
  if (!completa) {
    try {
      completa = await buscarSteamSpyCompleta(
        `https://steamspy.com/api.php?request=genre&genre=${encodeURIComponent(chave)}`,
      )
      cache[chave] = { at: Date.now(), completa }
      gravarCache(GENERO_CACHE, cache)
    } catch (e) {
      // Cai no cache antigo se existir (formato `.jogos`) — melhor uma fatia
      // parcial que uma tela vazia. Se nem isso, propaga o erro.
      if (guardado && Array.isArray(guardado.jogos)) {
        const fatia = guardado.jogos.slice(off, off + lim)
        return { ok: true, jogos: fatia, offset: off, total: guardado.jogos.length, cache: true, velho: true }
      }
      return { ok: false, error: String(e.message || e) }
    }
  }
  const fatia = await preparar(completa.slice(off, off + lim))
  return { ok: true, jogos: fatia, offset: off, total: completa.length }
}

// ---------- Fixes de jogos (estilo luatools: GameBypass/OnlineFix) ----------
// Índice: index.luatools.work/fixes-index.json (lista appids). Zips:
// files.luatools.work/GameBypasses/<appid>.zip e OnlineFix1/<appid>.zip.
const FIXES_INDEX = "https://index.luatools.work/fixes-index.json"
let fixesCache = null

async function fixesIndex() {
  if (fixesCache && Date.now() - fixesCache.ts < 6 * 3600_000) return fixesCache.data
  const r = await gh(FIXES_INDEX, { headers: { "User-Agent": "luatools" } })
  if (!r.ok) throw new Error(`índice de fixes HTTP ${r.status}`)
  const data = await r.json()
  fixesCache = { ts: Date.now(), data }
  return data
}

async function checkFixes(appid) {
  try {
    const idx = await fixesIndex()
    const id = Number(appid)
    const has = (arr) => (arr || []).some((v) => Number(v) === id)
    return {
      ok: true,
      generic: has(idx.genericFixes),
      online: has(idx.onlineFixes),
    }
  } catch (e) {
    return { ok: false, error: String(e) }
  }
}

// Baixa e extrai o fix na pasta de instalação do jogo.
async function applyFix(appid, type, installPath) {
  try {
    const base = type === "online" ? "OnlineFix1" : "GameBypasses"
    const url = `https://files.luatools.work/${base}/${appid}.zip`
    const r = await gh(url, { headers: { "User-Agent": "luatools" } })
    if (!r.ok) return { ok: false, error: `fix HTTP ${r.status}` }
    const zipPath = path.join(TMP_DIR, `fix_${appid}.zip`)
    fs.mkdirSync(TMP_DIR, { recursive: true })
    fs.writeFileSync(zipPath, Buffer.from(await r.arrayBuffer()))
    if (!fs.existsSync(installPath)) return { ok: false, error: `pasta não existe: ${installPath}` }
    await new Promise((res) => execFile("python3", ["-m", "zipfile", "-e", zipPath, installPath], res))
    fs.rmSync(zipPath, { force: true })
    return { ok: true }
  } catch (e) {
    return { ok: false, error: String(e) }
  }
}

// Pasta de instalação do jogo (para onde o fix vai).
function gameInstallDir(g) {
  const home = os.homedir()
  const appid = String(g?.id || "").replace(/^steam:/, "")
  if (g?.launcher === "custom" && g.exe) return path.dirname(g.exe)
  if (g?.launcher === "steam" || String(g?.id || "").startsWith("steam:")) {
    for (const dir of [
      path.join(home, ".steam", "steam", "steamapps"),
      path.join(home, ".local", "share", "Steam", "steamapps"),
      path.join(home, ".var", "app", "com.valvesoftware.Steam", ".local", "share", "Steam", "steamapps"),
    ]) {
      const acf = path.join(dir, `appmanifest_${appid}.acf`)
      if (fs.existsSync(acf)) {
        const m = /"installdir"\s+"([^"]+)"/.exec(fs.readFileSync(acf, "utf-8"))
        if (m) return path.join(dir, "common", m[1])
      }
    }
  }
  if (g?.launcher === "epic") {
    try {
      const inst = JSON.parse(fs.readFileSync(path.join(home, ".config", "legendary", "installed.json"), "utf-8"))
      const app = String(g.id).replace(/^epic:/, "")
      if (inst[app]?.install_path) return inst[app].install_path
    } catch {}
  }
  return ""
}

// Provedores de manifesto (estilo luatools): tentados EM ORDEM até um
// devolver depots. Todos devolvem um zip no formato SteamTools (.lua+manifest).
const PROVEDORES = [
  { nome: "Morrenus", url: (appid, cfg) => `${HUBCAP_BASE}/manifest/${appid}?api_key=${cfg.hubcap_api_key || ""}`, headers: (cfg) => ({ Authorization: `Bearer ${cfg.hubcap_api_key}` }), precisaKey: true },
  { nome: "Ryuu", url: (appid) => RYUU_URL(appid), headers: () => ({}) },
  { nome: "Sushi", url: (appid) => SUSHI_URL(appid), headers: () => ({}) },
  // Último da fila: o host não respondia nos testes (timeout, sem HTTP algum).
  // Fica como último recurso para o caso de voltar, nunca atrasando os outros.
  { nome: "TwentyTwo Cloud", url: (appid) => TWENTYTWO_URL(appid), headers: () => ({}) },
]

// Baixa o zip do appid (provedor com fallback) e extrai depots/keys/token do .lua.
async function getManifest(appid) {
  const cfg = readConfig()
  const zipPath = path.join(TMP_DIR, `manifest_${appid}.zip`)
  fs.mkdirSync(TMP_DIR, { recursive: true })

  const outDir = path.join(TMP_DIR, `manifest_${appid}`)
  const erros = []

  // A cascata só pode parar quando um provedor entrega um zip COM depots. Antes
  // ela parava no primeiro que entregasse um zip qualquer, e a extração vinha
  // depois do laço: um zip vazio (acontece no Sushi) matava o pedido inteiro
  // sem nunca tentar o Ryuu, que tinha o jogo.
  for (const p of PROVEDORES) {
    if (p.precisaKey && !cfg.hubcap_api_key) continue
    try {
      // Sem teto de tempo, um provedor lento (o Ryuu chegou a 100s nos testes)
      // segura o pedido inteiro e o usuário fica olhando para uma tela parada.
      // Estourando o prazo, passamos ao próximo em vez de esperar sem fim.
      const r = await gh(p.url(appid, cfg), {
        headers: { "User-Agent": "arcadia", ...p.headers(cfg) },
        signal: AbortSignal.timeout(45000),
      })
      if (!r.ok) {
        erros.push(`${p.nome}: HTTP ${r.status}`)
        continue
      }
      const buf = Buffer.from(await r.arrayBuffer())
      // Zip válido começa com PK — alguns provedores devolvem HTML/erro com 200.
      if (buf.length < 4 || buf[0] !== 0x50 || buf[1] !== 0x4b) {
        erros.push(`${p.nome}: resposta não é zip`)
        continue
      }
      fs.writeFileSync(zipPath, buf)

      fs.rmSync(outDir, { recursive: true, force: true })
      fs.mkdirSync(outDir, { recursive: true })
      await new Promise((res) => execFile("python3", ["-m", "zipfile", "-e", zipPath, outDir], res))

      const dados = lerLuas(outDir, appid)
      if (!dados.depots.length) {
        erros.push(`${p.nome}: zip sem depots`)
        continue
      }
      storeLog(`manifesto ${appid}: ${p.nome} (${dados.depots.length} depots)`)
      return { ok: true, appid: String(appid), ...dados, outDir, fonte: p.nome }
    } catch (e) {
      erros.push(`${p.nome}: ${e}`)
    }
  }
  return { ok: false, error: erros.join(" · ") || "nenhum provedor disponível" }
}

// Lê os .lua extraídos: addappid(id, ..., "depotkey"), setManifestid(depot,
// "id"), addtoken("..."). Separado de getManifest para a cascata poder validar
// cada provedor antes de aceitá-lo.
function lerLuas(outDir, appid) {
  const depots = []
  const dlcs = []
  let token = ""
  for (const f of fs.readdirSync(outDir)) {
    if (!f.endsWith(".lua")) continue
    const lua = fs.readFileSync(path.join(outDir, f), "utf-8")
    const keyRe = /addappid\(\s*(\d+)\s*,\s*\d+\s*,\s*"([0-9a-f]+)"\s*\)/g
    let m
    const keys = {}
    while ((m = keyRe.exec(lua))) keys[m[1]] = m[2]
    const manRe = /setManifestid\(\s*(\d+)\s*,\s*"(\d+)"\s*(?:,\s*(\d+))?/g
    while ((m = manRe.exec(lua))) {
      depots.push({ depotId: m[1], manifestId: m[2], key: keys[m[1]] || "", size: Number(m[3] || 0) })
    }
    const tokRe = /addtoken\(\s*"([^"]+)"\s*\)/
    const t = tokRe.exec(lua)
    if (t) token = t[1]
    const dlcRe = /addappid\(\s*(\d+)\s*\)/g
    while ((m = dlcRe.exec(lua))) {
      if (m[1] !== String(appid) && !dlcs.includes(m[1])) dlcs.push(m[1])
    }
  }
  return { depots, token, dlcs }
}

// ---------- Download via DepotDownloader ----------

// Monta o spawn do download de um appid. Retorna { cmd, args, env }.
async function prepareDownload({ appid, installdir, depots, steamDir }) {
  if (!depsOk()) return { ok: false, error: "DepotDownloader não encontrado em bin/deps" }
  const keysFile = path.join(TMP_DIR, `keys_${appid}.vdf`)
  const linhas = depots.filter((d) => d.key).map((d) => `${d.depotId};${d.key}`)
  fs.writeFileSync(keysFile, linhas.join("\n"))
  const dest = path.join(steamDir, "steamapps", "common", installdir)
  fs.mkdirSync(dest, { recursive: true })
  // UM comando por depot, como o Acella faz. Antes empilhávamos todos os
  // depots num único comando, repetindo -manifestfile: esse parâmetro do
  // DepotDownloader aceita um só valor, então todos os depots menos um
  // acabavam apontando para o manifesto errado. Com jogos de 1 depot passava
  // despercebido; com os de 8, 13 ou 59 o download quebrava.
  const cmds = []
  const pulados = []
  for (const d of depots) {
    if (!d.manifestId) {
      pulados.push(String(d.depotId))
      continue
    }
    const args = [
      path.join(DEPS_DIR, "DepotDownloader.dll"),
      "-app", String(appid),
      "-depot", String(d.depotId),
      "-manifest", String(d.manifestId),
    ]
    // Sem o .manifest local o DepotDownloader precisa pedir um "manifest
    // request code" à Steam, e a conta anônima não recebe código para
    // manifesto antigo: responde 401 e aborta. Esses depots (em geral OST,
    // artbook e afins, que vêm no .lua mas não no zip) são pulados — tentá-los
    // é falha garantida.
    const manFile = path.join(TMP_DIR, `manifest_${appid}`, `${d.depotId}_${d.manifestId}.manifest`)
    if (!fs.existsSync(manFile)) {
      pulados.push(String(d.depotId))
      continue
    }
    args.push("-manifestfile", manFile)
    args.push("-depotkeys", keysFile, "-max-downloads", "20", "-dir", dest, "-validate")
    cmds.push({ cmd: dotnetBin(), args, depotId: String(d.depotId) })
  }
  if (!cmds.length) {
    return {
      ok: false,
      error: "nenhum depot com .manifest local — o provedor entregou o .lua sem os manifestos",
    }
  }
  if (pulados.length) storeLog(`download ${appid}: ${pulados.length} depot(s) pulado(s) sem .manifest: ${pulados.join(",")}`)
  return { ok: true, cmds, dest, pulados }
}

// ---------- Registro na Steam (acf + SLSsteam) ----------

function findSteamDir() {
  const home = os.homedir()
  const candidatos = [
    path.join(home, ".steam", "steam"),
    path.join(home, ".local", "share", "Steam"),
    path.join(home, ".var", "app", "com.valvesoftware.Steam", ".local", "share", "Steam"),
  ]
  for (const c of candidatos) {
    if (fs.existsSync(path.join(c, "steamapps"))) return c
  }
  return candidatos[0]
}

function acfEscape(s) {
  return String(s).replace(/\\/g, "\\\\").replace(/"/g, '\\"')
}

// Escreve steamapps/appmanifest_<appid>.acf mínimo (port do Acella).
// A marca "ArcadiaDownload" distingue downloads NOSSOS de jogos owned — é ela
// que habilita o Remover seguro (nunca apaga acf de jogo comprado).
function writeAcf({ appid, title, installdir, steamDir }) {
  const apps = path.join(steamDir, "steamapps")
  const agora = Math.floor(Date.now() / 1000)
  const conteudo = [
    `"AppState"`,
    `{`,
    `\t"appid"\t\t"${acfEscape(appid)}"`,
    `\t"Universe"\t\t"1"`,
    `\t"name"\t\t"${acfEscape(title)}"`,
    `\t"StateFlags"\t\t"4"`,
    `\t"installdir"\t\t"${acfEscape(installdir)}"`,
    `\t"LastUpdated"\t\t"${agora}"`,
    `\t"LastPlayed"\t\t"0"`,
    `\t"SizeOnDisk"\t\t"1"`,
    `\t"buildid"\t\t"1"`,
    `\t"ArcadiaDownload"\t\t"1"`,
    `}`,
    ``,
  ].join("\n")
  fs.writeFileSync(path.join(apps, `appmanifest_${appid}.acf`), conteudo)
}

// Acfs de downloads feitos pelo Arcadia (com a marca), em todas as bibliotecas.
function arcadiaDownloaded() {
  const out = []
  for (const lib of steamLibraries()) {
    try {
      for (const f of fs.readdirSync(lib.path)) {
        const m = /^appmanifest_(\d+)\.acf$/.exec(f)
        if (!m) continue
        const txt = fs.readFileSync(path.join(lib.path, f), "utf-8")
        if (txt.includes('"ArcadiaDownload"')) {
          const dir = /"installdir"\s+"([^"]+)"/.exec(txt)
          out.push({ appid: m[1], steamapps: lib.path, installdir: dir?.[1] || "" })
        }
      }
    } catch {}
  }
  return out
}

// Remove um jogo BAIXADO pela loja: apaga pasta, appmanifest marcado e
// registro na SLSsteam. Nunca toca em acf sem a marca (jogos owned).
function removeDownloaded(appid) {
  const id = String(appid)
  const achados = arcadiaDownloaded().filter((a) => a.appid === id)
  for (const a of achados) {
    try { fs.rmSync(path.join(a.steamapps, `appmanifest_${id}.acf`), { force: true }) } catch {}
    if (a.installdir) {
      const dir = path.join(a.steamapps, "common", a.installdir)
      if (dir.startsWith(path.join(a.steamapps, "common") + path.sep) && fs.existsSync(dir)) {
        try { fs.rmSync(dir, { recursive: true, force: true }) } catch {}
      }
    }
  }
  removeFromSteam(id)
  return { ok: true, removidos: achados.length }
}

// Registra appid/token/DLCs no ~/.config/SLSsteam/config.yaml (edição simples).
function registerSlssteam({ appid, token, dlcs }) {
  const cfgPath = path.join(os.homedir(), ".config", "SLSsteam", "config.yaml")
  if (!fs.existsSync(cfgPath)) return { ok: false, error: "config.yaml da SLSsteam não encontrado" }
  let y = fs.readFileSync(cfgPath, "utf-8")
  const appidStr = String(appid)
  if (!y.includes(appidStr)) {
    // AdditionalApps: lista de appids extras exibidos como owned.
    if (/^AdditionalApps:/m.test(y)) {
      y = y.replace(/^AdditionalApps:\s*$/m, `AdditionalApps:\n  - ${appidStr}`)
      y = y.replace(/^(AdditionalApps:\s*\[)([^\]]*)\]/m, (_m, a, b) => `${a}${b ? b + ", " : ""}${appidStr}]`)
    } else {
      y += `\nAdditionalApps:\n  - ${appidStr}\n`
    }
  }
  if (token && !y.includes(token)) {
    if (/^AppTokens:/m.test(y)) {
      y = y.replace(/^AppTokens:\s*$/m, `AppTokens:\n  ${appidStr}: ${token}`)
      y = y.replace(/^AppTokens:\s*\{\}\s*$/m, `AppTokens:\n  ${appidStr}: ${token}`)
    } else {
      y += `\nAppTokens:\n  ${appidStr}: ${token}\n`
    }
  }
  for (const dlc of dlcs || []) {
    if (!y.includes(dlc)) {
      // DlcData é mapa de mapas: <appid>: { <dlcId>: "nome" } — escrever como
      // lista (- dlc) ou chave duplicada CRASHA a Steam no boot (yaml-cpp).
      if (new RegExp(`^  ${appidStr}:`, "m").test(y)) {
        y = y.replace(new RegExp(`^(  ${appidStr}:\\s*\\n)`, "m"), `$1    ${dlc}: "DLC"\n`)
      } else if (/^DlcData:/m.test(y)) {
        y = y.replace(/^DlcData:\s*$/m, `DlcData:\n  ${appidStr}:\n    ${dlc}: "DLC"`)
      } else {
        y += `\nDlcData:\n  ${appidStr}:\n    ${dlc}: "DLC"\n`
      }
    }
  }
  fs.writeFileSync(cfgPath, y)
  return { ok: true }
}

// Reinicia a Steam com a SLSsteam carregada (jogos aparecem como owned).
// Prefere o wrapper do slsteam-moon (~/.local/share/SLSsteam/path/steam),
// que injeta LD_AUDIT do jeito certo; fallback: steam puro + LD_AUDIT.
function launchSteamWithSls(cfg = readConfig()) {
  const home = os.homedir()
  const wrapper = path.join(home, ".local", "share", "SLSsteam", "path", "steam")
  const inject = [
    path.join(home, ".local", "share", "SLSsteam", "library-inject.so"),
    path.join(home, ".local", "share", "SLSsteam", "SLSsteam.so"),
  ]
  if (cfg.slssteam_path) inject.push(cfg.slssteam_path)
  const validos = inject.filter((p) => fs.existsSync(p))
  const usarWrapper = fs.existsSync(wrapper)
  if (!usarWrapper && validos.length < 2) {
    return { ok: false, error: "SLSsteam não instalada (rode o setup em Configurações)" }
  }
  execFile("pkill", ["-x", "steam"], () => {
    // Espera a Steam morrer DE VERDADE antes de relançar — relançar cedo
    // demais batia no lock da instância antiga e a nova morria (parecia
    // "shutdown" e precisava de um segundo clique).
    const env = { ...process.env }
    delete env.LD_LIBRARY_PATH
    delete env.LD_PRELOAD
    delete env.LD_AUDIT
    delete env.STEAM_RUNTIME_LIBRARY_PATH
    let cmd = "steam"
    if (usarWrapper) {
      cmd = wrapper
    } else {
      env.LD_AUDIT = validos.join(":")
    }
    let tentativas = 0
    const t0 = Date.now()
    const esperar = setInterval(() => {
      execFile("pgrep", ["-x", "steam"], (e) => {
        const morreu = Boolean(e)
        const estourou = ++tentativas > 120
        if (morreu || estourou) {
          clearInterval(esperar)
          storeLog(`restart: steam ${morreu ? "morreu" : "timeout 120s"} em ${Date.now() - t0}ms — relançando via ${cmd}`)
          // Graça de 2s p/ o lock do cliente liberar antes de relançar.
          setTimeout(() => {
            const child = spawn(cmd, [], { detached: true, stdio: "ignore", env })
            child.unref()
          }, 2000)
        }
      })
    }, 1000)
  })
  return { ok: true }
}

// Instala a SLSsteam (slsteam-moon) do zero: baixa o release mais recente do
// GitHub, extrai e roda o setup.sh install. Retorna { ok, error? }.
async function installSlssteam(onProgress) {
  const home = os.homedir()
  const slsDir = path.join(home, ".local", "share", "SLSsteam")
  try {
    onProgress?.("Buscando release mais recente…")
    const rel = await gh("https://api.github.com/repos/swwayps/slsteam-moon/releases/latest").then((r) => r.json())
    const asset = (rel.assets || []).find((a) => /^slsteam-moon-linux-.*-lumen\.zip$/.test(a.name || ""))
    if (!asset) return { ok: false, error: "asset slsteam-moon-linux-*-lumen.zip não encontrado" }

    onProgress?.("Baixando slsteam-moon…")
    const zipPath = path.join(TMP_DIR, "slsteam-moon.zip")
    fs.mkdirSync(TMP_DIR, { recursive: true })
    const buf = await fetch(asset.browser_download_url).then((r) => {
      if (!r.ok) throw new Error(`download HTTP ${r.status}`)
      return r.arrayBuffer()
    })
    fs.writeFileSync(zipPath, Buffer.from(buf))

    const outDir = path.join(TMP_DIR, "slsteam-moon")
    fs.rmSync(outDir, { recursive: true, force: true })
    fs.mkdirSync(outDir, { recursive: true })
    await new Promise((res) => execFile("python3", ["-m", "zipfile", "-e", zipPath, outDir], res))

    // setup.sh fica na raiz da pasta extraída (slsteam-moon-<ver>-lumen/).
    const raiz = fs.readdirSync(outDir).map((d) => path.join(outDir, d)).find((p) => fs.existsSync(path.join(p, "setup.sh")))
    if (!raiz) return { ok: false, error: "setup.sh não encontrado no pacote" }

    // O setup.sh tenta um `sudo -v` para gravar o .desktop em /usr/share. Rodando
    // pelo app não há terminal, então o sudo falha e o script aborta ("password
    // not provided") — quebrava a instalação para todos. Esse passo é opcional: o
    // próprio script diz que as entradas por usuário já cobrem tudo via prioridade
    // XDG. Pior: ele grava o home de UM usuário num arquivo global, quebrando a
    // Steam das outras contas. Forçamos o ramo sem sudo (cobertura --user).
    const setupPath = path.join(raiz, "setup.sh")
    try {
      const src = fs.readFileSync(setupPath, "utf-8")
      const semSudo = src.replace("elif command -v sudo >/dev/null 2>&1; then", "elif false; then")
      if (semSudo !== src) fs.writeFileSync(setupPath, semSudo)
    } catch {}

    onProgress?.("Instalando (setup.sh install)…")
    // Captura a saída: sem isto o erro virava só "código 1", sem dizer o motivo
    // (dependência faltando, Steam ausente, permissão…). Grava tudo em log e
    // devolve as últimas linhas no erro, para o usuário ver o que falhou.
    let out = ""
    const code = await new Promise((res) => {
      const c = spawn("bash", [path.join(raiz, "setup.sh"), "install"], {
        cwd: raiz,
        stdio: ["ignore", "pipe", "pipe"],
      })
      c.stdout.on("data", (d) => (out += d))
      c.stderr.on("data", (d) => (out += d))
      c.on("close", res)
      c.on("error", (e) => { out += `\n[spawn] ${e.message}`; res(1) })
      setTimeout(() => { out += "\n[timeout 5min]"; res(1) }, 300000)
    })
    try {
      fs.mkdirSync(LOG_DIR, { recursive: true })
      fs.writeFileSync(path.join(LOG_DIR, "slssteam-setup.log"), out)
    } catch {}
    // Só o que importa para injetar: a lib e o wrapper. Se ambos existem, a
    // instalação serve — não falhamos por causa de um passo final cosmético.
    const instalado = fs.existsSync(path.join(slsDir, "SLSsteam.so"))
      && fs.existsSync(path.join(slsDir, "path", "steam"))
    if (instalado) return { ok: true }
    // Últimas ~6 linhas não vazias da saída — normalmente contêm a causa real.
    const tail = out.split("\n").map((l) => l.trim()).filter(Boolean).slice(-6).join(" · ")
    return {
      ok: false,
      error: `setup.sh saiu com código ${code}${tail ? ` — ${tail}` : ""}`
        + ` (log completo em ~/.local/share/arcadia/logs/slssteam-setup.log)`,
    }
  } catch (e) {
    return { ok: false, error: String(e) }
  }
}

// Raiz da instalação da Steam (o que o LuaTools chama de steam_path). É aqui
// que ficam config/stplug-in e depotcache — NÃO em ~/.config/SLSsteam.
function steamBasePath() {
  const home = os.homedir()
  const cands = [
    path.join(home, ".steam", "steam"),
    path.join(home, ".local", "share", "Steam"),
    path.join(home, ".steam", "debian-installation"),
    path.join(home, ".var", "app", "com.valvesoftware.Steam", ".local", "share", "Steam"),
  ]
  return cands.find((p) => fs.existsSync(path.join(p, "steamapps")) || fs.existsSync(path.join(p, "config"))) || ""
}

// Apaga o que versões antigas do Arcadia gravaram em ~/.config/SLSsteam/
// (local errado): .lua do app e .manifest dos depots dele.
function limpaLegado(appid) {
  const home = os.homedir()
  const id = String(appid)
  const luaDir = path.join(home, ".config", "SLSsteam", "config", "stplug-in")
  const manDir = path.join(home, ".config", "SLSsteam", "manifests")
  const depots = new Set()
  try {
    const lua = path.join(luaDir, `${id}.lua`)
    if (fs.existsSync(lua)) {
      for (const m of fs.readFileSync(lua, "utf-8").matchAll(/addappid\s*\(\s*(\d+)/g)) depots.add(m[1])
      fs.rmSync(lua, { force: true })
    }
    if (fs.existsSync(manDir)) {
      for (const f of fs.readdirSync(manDir)) {
        const m = /^(\d+)_.*\.manifest$/.exec(f)
        if (m && depots.has(m[1])) fs.rmSync(path.join(manDir, f), { force: true })
      }
    }
  } catch {}
}

// Adiciona o jogo à Steam SEM baixar (estilo LuaTools): copia o .lua para
// <steam>/config/stplug-in e os .manifest para <steam>/depotcache, e registra
// o appid em AdditionalApps. O download fica por conta da Steam.
function addToSteam(appid) {
  const outDir = path.join(TMP_DIR, `manifest_${appid}`)
  const base = steamBasePath()
  if (!base) return { ok: false, error: "instalação da Steam não encontrada" }
  const stplug = path.join(base, "config", "stplug-in")
  // Os manifestos vão para o depotcache da PRÓPRIA Steam — é de lá que ela lê
  // ao montar a lista de depots. Em ~/.config/SLSsteam/manifests/ ela não
  // enxerga: o download fica em "Download Queued" ou "content still encrypted".
  const depotCache = path.join(base, "depotcache")
  try {
    fs.mkdirSync(stplug, { recursive: true })
    fs.mkdirSync(depotCache, { recursive: true })
    // Versões antigas gravavam em ~/.config/SLSsteam/. Removemos o resto para
    // não ficar um .lua duplicado (com setManifestid ativo) concorrendo.
    limpaLegado(appid)
    let luas = 0
    if (fs.existsSync(outDir)) {
      for (const f of fs.readdirSync(outDir)) {
        if (f.endsWith(".lua")) {
          // setManifestid() prende a Steam a uma versão específica do depot;
          // se ela não bate com a do CDN, dá "content still encrypted". O
          // LuaTools comenta essas linhas para a Steam usar a versão atual.
          const txt = fs.readFileSync(path.join(outDir, f), "utf-8")
            .replace(/^(\s*)(setManifestid\()/gm, "$1-- $2")
          fs.writeFileSync(path.join(stplug, f), txt)
          luas++
        } else if (f.endsWith(".manifest")) {
          fs.copyFileSync(path.join(outDir, f), path.join(depotCache, f))
        }
      }
    }
    if (!luas) return { ok: false, error: "nenhum .lua no manifesto — baixe o manifesto antes (botão Baixar)" }
    return { ok: true, luas }
  } catch (e) {
    return { ok: false, error: String(e) }
  }
}

// Bibliotecas Steam (multi-drive): lê steamapps/libraryfolders.vdf.
// Cada biblioteca é uma pasta "SteamLibrary" com steamapps/ dentro.
function steamLibraries() {
  const home = os.homedir()
  const raizes = [
    path.join(home, ".steam", "steam", "steamapps"),
    path.join(home, ".local", "share", "Steam", "steamapps"),
    path.join(home, ".var", "app", "com.valvesoftware.Steam", ".local", "share", "Steam", "steamapps"),
  ]
  const libs = []
  for (const raiz of raizes) {
    const vdf = path.join(raiz, "libraryfolders.vdf")
    if (!fs.existsSync(vdf)) continue
    const txt = fs.readFileSync(vdf, "utf-8")
    // Formato: "0" { "path" "/x/SteamLibrary" ... } — pega todos os "path".
    for (const m of txt.matchAll(/"path"\s+"([^"]+)"/g)) {
      const p = m[1].replace(/\\\\/g, "/")
      const apps = path.join(p, "steamapps")
      if (!libs.some((l) => l.path === apps)) libs.push({ path: apps, steamDir: p })
    }
  }
  // Garante ao menos a principal.
  if (!libs.length) {
    const principal = findSteamDir()
    libs.push({ path: path.join(principal, "steamapps"), steamDir: principal })
  }
  // Espaço livre (df) por biblioteca.
  return libs.map((l) => {
    let free = 0
    try {
      const { execFileSync } = require("child_process")
      const out = execFileSync("df", ["-BG", "--output=avail", l.path], { encoding: "utf-8" })
      free = parseInt(out.trim().split("\n").pop(), 10) || 0
    } catch {}
    return { ...l, free }
  })
}

// Remove um jogo adicionado via "Add": apaga o .lua do stplug-in e tira o
// appid de AdditionalApps/AppTokens/DlcData no config.yaml da SLSsteam.
function removeFromSteam(appid) {
  const home = os.homedir()
  const id = String(appid)
  const base = steamBasePath()
  const stplug = base ? path.join(base, "config", "stplug-in") : ""
  limpaLegado(id)
  try {
    if (stplug && fs.existsSync(stplug)) {
      // lua do app + manifests dos depots dele. Como o nome do manifest é
      // <depotId>_<manifestId>.manifest, usamos os depots conhecidos do zip
      // em tmp (se existir) e, como fallback, o prefixo do appid.
      const depotIds = new Set()
      const outDir = path.join(TMP_DIR, `manifest_${id}`)
      if (fs.existsSync(outDir)) {
        for (const f of fs.readdirSync(outDir)) {
          const m = /^(\d+)_.*\.manifest$/.exec(f)
          if (m) depotIds.add(m[1])
        }
      }
      for (const f of fs.readdirSync(stplug)) {
        if (f === `${id}.lua` || f.startsWith(`${id}_`)) {
          fs.rmSync(path.join(stplug, f), { force: true })
        }
      }
      // Manifestos no depotcache da Steam + no ManifestStore do moon (que o
      // slsteam-moon arquiva por conta própria e deixaria lixo para trás).
      const stores = [
        path.join(base, "depotcache"),
        path.join(home, ".config", "SLSsteam", "manifests"),
      ]
      for (const store of stores) {
        if (!fs.existsSync(store)) continue
        for (const f of fs.readdirSync(store)) {
          const m = /^(\d+)_.*\.manifest$/.exec(f)
          if (m && depotIds.has(m[1])) fs.rmSync(path.join(store, f), { force: true })
        }
      }
    }
  } catch {}
  const cfgPath = path.join(home, ".config", "SLSsteam", "config.yaml")
  if (!fs.existsSync(cfgPath)) return { ok: true }
  let y = fs.readFileSync(cfgPath, "utf-8")
  // Linha "  - <appid>" em AdditionalApps.
  y = y.replace(new RegExp(`^\\s*-\\s*${id}\\s*(#.*)?$\\n?`, "m"), "")
  // Token em AppTokens — SÓ dentro da seção AppTokens (antes o regex apagava
  // também a chave-pai do appid no DlcData, órfãos que crasham a Steam).
  const partes = y.split(/^(?=AppTokens:)/m)
  if (partes.length > 1) {
    const resto = partes[1].split(/^(?=\S)/m)
    resto[0] = resto[0].replace(new RegExp(`^\\s*${id}:.*$\\n?`, "m"), "")
    y = partes[0] + resto.join("")
  }
  fs.writeFileSync(cfgPath, y)
  return { ok: true }
}

async function status() {
  const dotnetSys = await new Promise((res) => execFile("dotnet", ["--version"], (e, stdout) => res(e ? "" : String(stdout).trim())))
  // AppIds já registrados (AdditionalApps do config.yaml + .lua no stplug-in)
  // — cobre apps adicionados por qualquer ferramenta, a qualquer época.
  const adicionados = new Set()
  try {
    const y = fs.readFileSync(path.join(os.homedir(), ".config", "SLSsteam", "config.yaml"), "utf-8")
    const bloco = y.split(/^AdditionalApps:/m)[1] || ""
    for (const m of bloco.matchAll(/^\s*-\s*(\d+)/gm)) adicionados.add(m[1])
  } catch {}
  try {
    const stplug = path.join(os.homedir(), ".config", "SLSsteam", "config", "stplug-in")
    for (const f of fs.readdirSync(stplug)) {
      const m = /^(\d+)(?:_.*)?\.lua$/.exec(f)
      if (m) adicionados.add(m[1])
    }
  } catch {}
  // Downloads feitos pelo Arcadia (acf marcado) também contam como "adicionados".
  for (const a of arcadiaDownloaded()) adicionados.add(a.appid)
  return {
    dotnet: fs.existsSync(path.join(BIN_DIR, "dotnet", "dotnet")) || dotnetSys || undefined,
    depotdownloader: depsOk(),
    hubcapKey: Boolean(readConfig().hubcap_api_key),
    slssteam: fs.existsSync(path.join(os.homedir(), ".local", "share", "SLSsteam", "SLSsteam.so")),
    steamDir: findSteamDir(),
    adicionados: [...adicionados],
  }
}

module.exports = {
  search,
  capaAlternativa,
  preparar,
  itensDaLoja,
  suggest,
  detalhes,
  porGenero,
  destaques,
  recent,
  popular,
  checkFixes,
  applyFix,
  gameInstallDir,
  getManifest,
  prepareDownload,
  ensureDotnet,
  writeAcf,
  registerSlssteam,
  launchSteamWithSls,
  installSlssteam,
  addToSteam,
  removeFromSteam,
  removeDownloaded,
  arcadiaDownloaded,
  steamLibraries,
  findSteamDir,
  status,
  DEPS_DIR,
}
