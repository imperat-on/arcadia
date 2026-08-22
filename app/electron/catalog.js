"use strict"

// Cliente do catalogo servido pelo backend. A resposta mais recente fica em
// disco e e usada quando o servidor esta indisponivel.
const fs = require("fs")
const crypto = require("crypto")
const path = require("path")
const { fetchRede } = require("./httpfetch")
const config = require("./supabase/config")
const { getDataDir } = require("./runtime-paths")

const DATA_DIR = getDataDir()
const ESPELHO_DIR = path.join(DATA_DIR, "catalog_espelho")

function espelhoPath(pathname) {
  const url = new URL(String(pathname || "/"), "http://catalog.local")
  const partes = url.pathname.split("/").filter(Boolean)
  if (partes[0] === "catalog" && partes[1] === "v1") partes.splice(0, 2)
  const nome =
    partes
      .map((parte) => parte.replace(/[^a-zA-Z0-9_-]/g, "_"))
      .filter(Boolean)
      .join("_") || "catalog"
  const query = [...url.searchParams.entries()]
    .sort(([ak, av], [bk, bv]) => ak.localeCompare(bk) || av.localeCompare(bv))
    .map(([k, v]) => `${k}=${v}`)
    .join("&")
  const sufixo = query
    ? `_${crypto.createHash("sha256").update(query).digest("hex").slice(0, 12)}`
    : ""
  return path.join(ESPELHO_DIR, `${nome}${sufixo}.json`)
}

function catalogGetEspelho(pathname) {
  try {
    return JSON.parse(fs.readFileSync(espelhoPath(pathname), "utf-8"))
  } catch {
    return null
  }
}

// Guarda o ETag junto do espelho (arquivo `.etag`), para enviar If-None-Match
// na proxima chamada e receber 304 quando nada mudou.
function etagPath(pathname) {
  return `${espelhoPath(pathname)}.etag`
}

function catalogGetEtag(pathname) {
  try {
    return fs.readFileSync(etagPath(pathname), "utf-8").trim()
  } catch {
    return null
  }
}

function gravarEtag(pathname, etag) {
  try {
    fs.writeFileSync(etagPath(pathname), String(etag || ""), "utf-8")
  } catch {
    // etag e otimizacao; falhar em gravar nao quebra nada
  }
}

function gravarEspelho(pathname, data) {
  try {
    fs.mkdirSync(ESPELHO_DIR, { recursive: true })
    const destino = espelhoPath(pathname)
    const tmp = `${destino}.tmp`
    fs.writeFileSync(tmp, JSON.stringify(data))
    fs.renameSync(tmp, destino)
  } catch {
    // O catalogo continua utilizavel em rede mesmo se o espelho nao puder ser salvo.
  }
}

// Dedupe em voo: quando a loja abre, varias abas podem pedir o mesmo catalogo
// quase ao mesmo tempo. Compartilham a mesma Promise em vez de disparar N
// requisicoes iguais ao servidor.
const emVoo = new Map()

function authHeaders() {
  try {
    return require("./supabase/client").getClient()._authHeaders()
  } catch {
    return {}
  }
}

async function catalogGet(pathname, opts = {}) {
  const caminho = String(pathname || "")
  if (!caminho.startsWith("/catalog/v1/")) {
    return { data: null, error: { message: "caminho de catalogo invalido" } }
  }

  // Dedupe em voo: reusa a requisicao pendente se outra chamada ja pediu a
  // mesma rota agora.
  if (emVoo.has(caminho)) return emVoo.get(caminho)

  // Faz UMA chamada HTTP ao catalogo com o token atual e trata o resultado
  // (espelho, 304, erro). Devolve { data, error, fallback, notModified }.
  async function _buscar() {
    const url = `${String(config.url).replace(/\/$/, "")}${caminho}`
    const headers = {
      ...authHeaders(),
      accept: "application/json",
      "accept-encoding": "gzip",
    }
    // If-None-Match: se o espelho ja tem este etag, o servidor devolve 304
    // e nao re-baixamos o JSON.
    const etag = opts.noMirror ? null : catalogGetEtag(caminho)
    if (etag) headers["if-none-match"] = etag

    const res = await fetchRede(url, {
      method: "GET",
      headers,
      signal: AbortSignal.timeout(opts.timeoutMs || 30000),
    })

    // 304: nada mudou — usa o espelho local (sem re-baixar).
    if (res.status === 304) {
      const espelho = opts.noMirror ? null : catalogGetEspelho(caminho)
      if (espelho !== null) return { data: espelho, error: null, fallback: true, notModified: true }
      return { data: null, error: { message: "HTTP 304 sem espelho" } }
    }

    if (!res.ok) {
      // 401: token expirado/invalido — renova a sessao (refresh) e retorna
      // um sinal para tentar de novo, em vez de cair no espelho imediatamente.
      if (res.status === 401) return { _renova: true }
      const espelho = opts.noMirror ? null : catalogGetEspelho(caminho)
      if (espelho !== null) return { data: espelho, error: null, fallback: true }
      return { data: null, error: { message: `HTTP ${res.status}` } }
    }

    const novoEtag = res.headers?.get?.("etag")
    if (novoEtag && !opts.noMirror) gravarEtag(caminho, novoEtag)
    const data = await res.json()
    if (!opts.noMirror) gravarEspelho(caminho, data)
    return { data, error: null, fallback: false }
  }

  const promessa = (async () => {
    try {
      const r1 = await _buscar()
      if (r1._renova) {
        // Token expirado: renova via setSession (faz refresh se preciso) e
        // tenta de novo UMA vez. Se ainda falhar, cai no espelho.
        try {
          const { getClient } = require("./supabase/client")
          const cliente = getClient()
          const sessao = cliente.auth._session
          if (sessao) {
            await cliente.auth.setSession(sessao)
          }
        } catch {}
        const r2 = await _buscar()
        if (!r2._renova) return r2
      }
      if (r1._renova) {
        // renovacao nao resolveu — fallback pro espelho
        const espelho = opts.noMirror ? null : catalogGetEspelho(caminho)
        if (espelho !== null) return { data: espelho, error: null, fallback: true }
        return { data: null, error: { message: "HTTP 401" } }
      }
      return r1
    } catch (e) {
      const espelho = opts.noMirror ? null : catalogGetEspelho(caminho)
      if (espelho !== null) return { data: espelho, error: null, fallback: true }
      return { data: null, error: { message: String(e?.message || e) } }
    }
  })()

  emVoo.set(caminho, promessa)
  try {
    return await promessa
  } finally {
    emVoo.delete(caminho)
  }
}

module.exports = { catalogGet, catalogGetEspelho, espelhoPath }
