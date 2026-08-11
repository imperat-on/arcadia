"use strict"

// Cliente do catalogo servido pelo backend. A resposta mais recente fica em
// disco e e usada quando o servidor esta indisponivel.
const fs = require("fs")
const crypto = require("crypto")
const os = require("os")
const path = require("path")
const { fetchRede } = require("./httpfetch")
const config = require("./supabase/config")

const DATA_DIR = process.env.ARCADIA_DATA_DIR || path.join(os.homedir(), ".local/share/arcadia")
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
  const url = `${String(config.url).replace(/\/$/, "")}${caminho}`
  try {
    const res = await fetchRede(url, {
      method: "GET",
      headers: { ...authHeaders(), accept: "application/json" },
      signal: AbortSignal.timeout(opts.timeoutMs || 30000),
    })
    if (!res.ok) {
      const espelho = catalogGetEspelho(caminho)
      if (espelho !== null) return { data: espelho, error: null, fallback: true }
      return { data: null, error: { message: `HTTP ${res.status}` } }
    }
    const data = await res.json()
    gravarEspelho(caminho, data)
    return { data, error: null, fallback: false }
  } catch (e) {
    const espelho = catalogGetEspelho(caminho)
    if (espelho !== null) return { data: espelho, error: null, fallback: true }
    return { data: null, error: { message: String(e?.message || e) } }
  }
}

module.exports = { catalogGet, catalogGetEspelho, espelhoPath }
