// Resolvedores de hosters (port simplificado do Hydra, GPL-3.0).
// Cada um transforma a URL da PÁGINA do hoster em URL DIRETA de download.
// Hosters que exigem execução de JS (datanodes, vikingfile, buzzheavier,
// akirabox) ficam de fora: esses precisam da janela oculta (fase 2).
const fs = require("fs")
const path = require("path")
const { fetchRede } = require("./httpfetch")
const { dataPath } = require("./runtime-paths")

// Debrids (aba Integrações): desbloqueiam hosters que exigem JS/captcha
// (datanodes, buzzheavier, 1fichier, vikingfile, mega, mediafire...). São
// FALLBACK: resolvedores gratuitos primeiro, depois cada debrid configurado
// na ordem RD → TorBox → AllDebrid → Premiumize.
function lerConfig() {
  try {
    return JSON.parse(
      fs.readFileSync(dataPath("config.json"), "utf-8"),
    )
  } catch {
    return {}
  }
}

async function realDebrid(url, token) {
  const r = await fetchRede("https://api.real-debrid.com/rest/1.0/unrestrict/link", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/x-www-form-urlencoded",
    },
    body: `link=${encodeURIComponent(url)}`,
    signal: AbortSignal.timeout(15000),
  })
  if (!r.ok) throw new Error(`Real-Debrid: HTTP ${r.status} (token inválido ou conta expirada?)`)
  const j = await r.json()
  if (!j?.download)
    throw new Error("Real-Debrid não liberou este link (hoster fora do catálogo deles)")
  return { url: j.download }
}

async function allDebrid(url, token) {
  const r = await fetchRede(
    `https://api.alldebrid.com/v4/link/unlock?agent=arcadia&apikey=${encodeURIComponent(token)}`,
    {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: `link=${encodeURIComponent(url)}`,
      signal: AbortSignal.timeout(15000),
    },
  )
  const j = await r.json()
  if (j?.status !== "success" || !j?.data?.link) {
    const msg = j?.error?.message || `HTTP ${r.status}`
    throw new Error(`AllDebrid: ${msg}`)
  }
  return { url: j.data.link }
}

async function premiumize(url, token) {
  const r = await fetchRede(
    `https://www.premiumize.me/api/transfer/directdl?apikey=${encodeURIComponent(token)}`,
    {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: `src=${encodeURIComponent(url)}`,
      signal: AbortSignal.timeout(15000),
    },
  )
  const j = await r.json()
  if (j?.status !== "success" || !j?.content?.[0]?.link) {
    throw new Error(`Premiumize: ${j?.message || `HTTP ${r.status}`}`)
  }
  return { url: j.content[0].link }
}

// TorBox: hoster HTTP vira "web download" — cria, aguarda cachear (curto
// para links populares), pega URL direta. Para link já cacheado o retorno é
// quase imediato. Prazo máximo 30s: se cachear mais do que isso, o usuário
// tenta de novo mais tarde (ou pega outra fonte).
async function torBox(url, token) {
  const auth = { Authorization: `Bearer ${token}`, "User-Agent": "arcadia" }
  const form = new FormData()
  form.append("link", url)
  const cria = await fetchRede("https://api.torbox.app/v1/api/webdl/createwebdownload", {
    method: "POST",
    headers: auth,
    body: form,
    signal: AbortSignal.timeout(15000),
  })
  const jc = await cria.json()
  if (!jc?.success || !jc?.data?.webdownload_id) {
    throw new Error(`TorBox: ${jc?.detail || jc?.error || `HTTP ${cria.status}`}`)
  }
  const webId = jc.data.webdownload_id

  // Aguarda cachear. Requests já cacheados voltam completed na 1ª iteração.
  // mylist devolve a LISTA inteira (o ?id= não filtra em toda conta) — por
  // isso achamos o nosso item pelo id, igual ao Hydra.
  const limite = Date.now() + 30000
  let fileId = null
  while (Date.now() < limite) {
    const lista = await fetchRede(`https://api.torbox.app/v1/api/webdl/mylist?id=${webId}`, {
      headers: auth,
      signal: AbortSignal.timeout(15000),
    })
    const jl = await lista.json()
    const dados = jl?.data
    const info = Array.isArray(dados) ? dados.find((x) => x?.id === webId) : dados
    if (
      info?.download_finished ||
      info?.download_state === "completed" ||
      info?.download_state === "cached"
    ) {
      const files = info.files || []
      // Pega o maior (evita samples/nfo dentro de zips estranhos).
      fileId = files.sort((a, b) => (b.size || 0) - (a.size || 0))[0]?.id
      break
    }
    await new Promise((r) => setTimeout(r, 3000))
  }
  if (fileId == null)
    throw new Error("TorBox: link ainda não cacheado (tente de novo em alguns minutos)")

  const link = await fetchRede(
    `https://api.torbox.app/v1/api/webdl/requestdl?token=${encodeURIComponent(token)}&web_id=${webId}&file_id=${fileId}`,
    {
      headers: auth,
      signal: AbortSignal.timeout(15000),
    },
  )
  const jd = await link.json()
  if (!jd?.data) throw new Error(`TorBox: ${jd?.detail || "requestdl sem URL"}`)
  return { url: String(jd.data) }
}

// Ordem dos debrids: RD primeiro (mais popular), TorBox (o usuário pediu),
// AllDebrid, Premiumize. Só entra na fila quem tem token.
const DEBRIDS_HTTP = [
  ["realdebrid_token", realDebrid],
  ["torbox_token", torBox],
  ["alldebrid_token", allDebrid],
  ["premiumize_token", premiumize],
]

async function tentarDebrids(url) {
  const cfg = lerConfig()
  const erros = []
  for (const [chave, fn] of DEBRIDS_HTTP) {
    const token = String(cfg[chave] || "").trim()
    if (!token) continue
    try {
      const r = await fn(url, token)
      if (r?.url) return r
    } catch (e) {
      erros.push(String(e.message || e))
    }
  }
  if (erros.length) throw new Error(erros.join(" | "))
  return null
}

// --- Magnet via debrid -------------------------------------------------------
// O debrid baixa o torrent no servidor dele e devolve link direto: sem P2P,
// velocidade máxima, torrent morto cacheado. Poll de até 2min esperando
// cachear; estourou → erro (quem chama cai no P2P local).
// Cache de torrent no debrid pode levar MUITO tempo (dezenas de minutos em
// repack grande): quem paga debrid quer esperar, não cair no P2P. O wait
// aceita AbortSignal (cancel/pause pelo usuário).
const MAGNET_POLL_LIMITE = 30 * 60 * 1000

async function pollDebrid(limite, fn, signal) {
  const fim = Date.now() + limite
  while (Date.now() < fim) {
    if (signal?.aborted) return null
    const r = await fn()
    if (r) return r
    await new Promise((r2) => setTimeout(r2, 4000))
  }
  return null
}

async function rdMagnet(magnet, token, signal) {
  const H = {
    Authorization: `Bearer ${token}`,
    "Content-Type": "application/x-www-form-urlencoded",
  }
  const add = await fetchRede("https://api.real-debrid.com/rest/1.0/torrents/addMagnet", {
    method: "POST",
    headers: H,
    body: `magnet=${encodeURIComponent(magnet)}`,
    signal: AbortSignal.timeout(15000),
  })
  const j = await add.json()
  if (!j?.id) throw new Error(`Real-Debrid addMagnet: HTTP ${add.status}`)
  await fetchRede(`https://api.real-debrid.com/rest/1.0/torrents/selectFiles/${j.id}`, {
    method: "POST",
    headers: H,
    body: "files=all",
    signal: AbortSignal.timeout(15000),
  })
  const info = await pollDebrid(
    MAGNET_POLL_LIMITE,
    async () => {
      const r = await fetchRede(`https://api.real-debrid.com/rest/1.0/torrents/info/${j.id}`, {
        headers: H,
        signal: AbortSignal.timeout(15000),
      })
      const d = await r.json()
      return d?.status === "downloaded" ? d : null
    },
    signal,
  )
  if (!info) throw new Error("Real-Debrid: torrent não cacheou em 30min")
  // links[] segue a ordem dos arquivos SELECIONADOS: pega o maior (o jogo).
  const sel = (info.files || []).filter((f) => f.selected).sort((a, b) => b.bytes - a.bytes)
  if (!sel.length) throw new Error("Real-Debrid: nenhum arquivo no torrent")
  const idx = (info.files || []).filter((f) => f.selected).indexOf(sel[0])
  const link = info.links?.[idx] || info.links?.[0]
  if (!link) throw new Error("Real-Debrid: sem link para o arquivo")
  return realDebrid(link, token)
}

async function tbMagnet(magnet, token, signal) {
  const H = { Authorization: `Bearer ${token}`, "User-Agent": "arcadia" }
  const form = new FormData()
  form.append("magnet", magnet)
  const add = await fetchRede("https://api.torbox.app/v1/api/torrents/createtorrent", {
    method: "POST",
    headers: H,
    body: form,
    signal: AbortSignal.timeout(15000),
  })
  const j = await add.json()
  if (!j?.success || !j?.data?.torrent_id)
    throw new Error(`TorBox createtorrent: ${j?.detail || "falha"}`)
  const id = j.data.torrent_id
  const ok = await pollDebrid(
    MAGNET_POLL_LIMITE,
    async () => {
      // ?id= É necessário: sem filtro a mylist só traz os ~60 mais recentes,
      // e um torrent cacheado há meses (o TorBox reuso o cache antigo) nunca
      // apareceria — foi o bug do "cacheado mas nunca inicia".
      const r = await fetchRede(`https://api.torbox.app/v1/api/torrents/mylist?id=${id}`, {
        headers: H,
        signal: AbortSignal.timeout(15000),
      })
      const dados = (await r.json())?.data
      // Com id vem objeto único; defensivamente aceita lista também.
      const d = Array.isArray(dados) ? dados.find((x) => x?.id === id) : dados
      return d &&
        (d.download_finished || d.download_state === "completed" || d.download_state === "cached")
        ? true
        : null
    },
    signal,
  )
  if (!ok) throw new Error("TorBox: torrent não cacheou em 30min")
  const dl = await fetchRede(
    `https://api.torbox.app/v1/api/torrents/requestdl?token=${encodeURIComponent(token)}&torrent_id=${id}&zip_link=true`,
    {
      headers: H,
      signal: AbortSignal.timeout(15000),
    },
  )
  const jd = await dl.json()
  if (!jd?.data) throw new Error(`TorBox requestdl: ${jd?.detail || "sem URL"}`)
  return { url: String(jd.data) }
}

// AllDebrid: POSTs form-encoded no v4.1 (mesmo formato validado do Hydra).
async function adPost(endpoint, token, params) {
  const r = await fetchRede(
    `https://api.alldebrid.com/${endpoint}?agent=arcadia&apikey=${encodeURIComponent(token)}`,
    {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({ agent: "arcadia", ...params }).toString(),
      signal: AbortSignal.timeout(15000),
    },
  )
  const j = await r.json()
  if (j?.status !== "success")
    throw new Error(`AllDebrid: ${j?.error?.message || `HTTP ${r.status}`}`)
  return j.data
}

// A resposta de files é ÁRVORE (pastas têm filhos .e; arquivos têm link .l,
// nome .n e tamanho .s). Coleta recursiva.
function adColetarArquivos(nodes) {
  const out = []
  for (const n of nodes || []) {
    if (n?.l) out.push({ link: n.l, size: n.s || 0, name: n.n || "" })
    if (n?.e?.length) out.push(...adColetarArquivos(n.e))
  }
  return out
}

async function adMagnet(magnet, token, signal) {
  const up = await adPost("v4.1/magnet/upload", token, { "magnets[]": magnet })
  const m0 = Array.isArray(up?.magnets) ? up.magnets[0] : up?.magnets
  const id = m0?.id
  if (!id) throw new Error("AllDebrid: upload sem id")

  const pronto = await pollDebrid(
    MAGNET_POLL_LIMITE,
    async () => {
      const st = await adPost("v4.1/magnet/status", token, { id: String(id) })
      const m = Array.isArray(st?.magnets) ? st.magnets[0] : st?.magnets
      return m?.status === "Ready" || m?.statusCode === 4 ? m : null
    },
    signal,
  )
  if (!pronto) throw new Error("AllDebrid: torrent não cacheou em 30min")

  let arquivos = []
  try {
    const f = await adPost("v4.1/magnet/files", token, { "id[]": String(id) })
    const magnets = Array.isArray(f?.magnets) ? f.magnets : [f?.magnets]
    arquivos = adColetarArquivos(magnets[0]?.files)
  } catch {
    const f = await adPost("v4/magnet/files", token, { "id[]": String(id) })
    const magnets = Array.isArray(f?.magnets) ? f.magnets : [f?.magnets]
    arquivos = adColetarArquivos(magnets[0]?.files)
  }
  if (!arquivos.length) throw new Error("AllDebrid: sem arquivos no torrent")
  // Maior arquivo = o jogo (samples/nfo ficam de fora).
  const maior = arquivos.sort((a, b) => b.size - a.size)[0]
  return allDebrid(maior.link, token)
}

async function pzMagnet(magnet, token) {
  // directdl resolve magnet já cacheado de uma vez; não cacheado → erro e
  // quem chama tenta o próximo debrid/P2P.
  return premiumize(magnet, token)
}

const DEBRIDS_MAGNET = [
  ["realdebrid_token", rdMagnet],
  ["torbox_token", tbMagnet],
  ["alldebrid_token", adMagnet],
  ["premiumize_token", pzMagnet],
]

// Magnet via debrid. null = nenhum token configurado OU abortado; throw =
// todos falharam.
async function resolverMagnet(magnet, { signal } = {}) {
  const cfg = lerConfig()
  const erros = []
  for (const [chave, fn] of DEBRIDS_MAGNET) {
    const token = String(cfg[chave] || "").trim()
    if (!token) continue
    try {
      const r = await fn(magnet, token, signal)
      if (r?.url) return r
    } catch (e) {
      erros.push(String(e.message || e))
    }
    if (signal?.aborted) return null
  }
  if (erros.length) throw new Error(erros.join(" | "))
  return null
}

const UA =
  "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"

// Sonda uma URL candidata: true se responde binário (não HTML/erro).
async function ehBinario(url, headers = {}) {
  try {
    const r = await fetchRede(url, {
      headers: { "User-Agent": UA, Range: "bytes=0-0", ...headers },
      redirect: "follow",
      signal: AbortSignal.timeout(8000),
    })
    await r.body?.cancel().catch(() => {})
    if (!r.ok && r.status !== 206) return false
    const ct = String(r.headers.get("content-type") || "").toLowerCase()
    return Boolean(ct) && !ct.includes("html") && !ct.includes("json") && !ct.includes("text/plain")
  } catch {
    return false
  }
}

// gofile.io/d/<id>: CDN alternativo direto (sem token de API).
async function gofile(url) {
  const id = new URL(url).pathname.split("/").filter(Boolean)[1]
  if (!id) return null
  const cdn = `https://gofilecdn.eu.cc/${encodeURIComponent(id)}`
  return (await ehBinario(cdn)) ? { url: cdn } : null
}

// pixeldrain.com/u/<id>: bypass CDN primeiro; API oficial de fallback.
async function pixeldrain(url) {
  const partes = new URL(url).pathname.split("/").filter(Boolean)
  if (partes[0] !== "u" || !partes[1]) return null
  const bypass = `https://cdn.pixeldrain.eu.cc/${partes[1]}`
  if (await ehBinario(bypass)) return { url: bypass }
  return { url: `https://pixeldrain.com/api/file/${partes[1]}?download` }
}

// rootz.so/d/<id>: token da página + API -> proxy-download.
async function rootz(url) {
  const u = new URL(url)
  const partes = u.pathname.split("/").filter(Boolean)
  if (partes[0] !== "d" || !partes[1]) return null
  const id = partes[1]
  const pageUrl = `https://www.rootz.so/d/${id}`
  const pagina = await fetchRede(pageUrl, {
    headers: { "User-Agent": UA, Accept: "text/html" },
    signal: AbortSignal.timeout(15000),
  })
  const html = await pagina.text()
  const token = /\\?"pageToken\\?"\s*:\s*\\?"([^"\\]+)/.exec(html)?.[1]
  if (!token) throw new Error("rootz: pageToken não encontrado")
  const r = await fetchRede(
    `https://www.rootz.so/api/files/download-by-short?shortId=${encodeURIComponent(id)}`,
    {
      headers: {
        "User-Agent": UA,
        Accept: "application/json",
        Referer: pageUrl,
        "X-Page-Token": token,
      },
      signal: AbortSignal.timeout(15000),
    },
  )
  const j = await r.json()
  const data = j?.data
  if (!j?.success || !data?.fileId) throw new Error(j?.error || "rootz: arquivo não encontrado")
  if (data.status && data.status !== "active") throw new Error(`rootz: arquivo ${data.status}`)
  if (data.downloadAllowed === false) throw new Error("rootz: download não permitido")
  return {
    url: `https://www.rootz.so/api/files/proxy-download/${data.fileId}`,
    headers: { Referer: pageUrl },
  }
}

const RESOLVEDORES = [
  [/gofile\.io\/d\//, gofile],
  [/pixeldra\.?in\.com\/u\/|pixeldrain\.com\/u\//, pixeldrain],
  [/rootz\.so\/d\//, rootz],
]

// Devolve { url, headers? } ou null se nenhum resolvedor conhece o hoster.
// Ordem: gratuito primeiro; debrids configurados como fallback.
async function resolverHoster(url) {
  for (const [re, fn] of RESOLVEDORES) {
    if (re.test(url)) {
      const r = await fn(url)
      if (r?.url) return r
      break // gratuito falhou: cai nos debrids antes de desistir
    }
  }
  return tentarDebrids(url)
}

module.exports = { resolverHoster, resolverMagnet }
