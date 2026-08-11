"use strict"

// Sync de sources publicas com o servidor (reconcile no login).
// Espelha o padrao de biblioteca.js: push com watermark, pull no login,
// debounce 2s. Sincroniza SO o registro {source_id, url, name} - nunca
// etag/lastMod/count (estado de download local) nem API keys (locais).

const { caminhoArquivoConta } = require("./conta")
const { getClient } = require("./client")
const sourcesApp = require("../sources")

const REG = () => caminhoArquivoConta("sources.json")
const STATE = () => caminhoArquivoConta("sync_state.json")

function readJson(file, def) {
  try {
    return JSON.parse(require("node:fs").readFileSync(file, "utf-8"))
  } catch {
    return def
  }
}
function writeJson(file, obj) {
  const fs = require("node:fs")
  const tmp = `${file}.tmp`
  fs.writeFileSync(tmp, JSON.stringify(obj, null, 2))
  fs.renameSync(tmp, file)
}
function loadState() {
  return readJson(STATE(), {})
}
function saveState(st) {
  writeJson(STATE(), st)
}

// Registro local (array) de sources publicas
function registro() {
  return sourcesApp.list() || []
}

async function requireUserId() {
  const { data, error } = await getClient().auth.getUser()
  if (error || !data?.user) return null
  return data.user.id
}

// Push: sobe sources novas/removidas por watermark srcPush
async function push() {
  const me = await requireUserId()
  if (!me) return { ok: true, pushed: 0 }

  const st = loadState()
  const enviados = st.srcPush || {}
  const reg = registro()
  const ids = new Set(reg.map((s) => s.id))
  const p_sources = []

  for (const s of reg) {
    if (!enviados[s.id]) p_sources.push({ source_id: s.id, url: s.url, name: s.name })
  }
  for (const id of Object.keys(enviados)) {
    if (!ids.has(id)) p_sources.push({ source_id: id, removed: true })
  }
  if (!p_sources.length) return { ok: true, pushed: 0 }

  const { error } = await getClient().rpc("push_sources", { p_sources })
  if (error) return { ok: false, error: error.message }

  for (const s of reg) enviados[s.id] = { url: s.url, name: s.name }
  for (const p of p_sources) if (p.removed) delete enviados[p.source_id]
  st.srcPush = enviados
  saveState(st)
  return { ok: true, pushed: p_sources.length }
}

// Pull: fontes do servidor entram no registro local (sem baixar cache)
async function pull() {
  const me = await requireUserId()
  if (!me) return { ok: true, pulled: 0 }

  const { data, error } = await getClient().rpc("pull_sources")
  if (error || !Array.isArray(data)) return { ok: false, error: error?.message || "sem dados" }

  const reg = registro()
  const byId = new Map(reg.map((s) => [s.id, s]))
  let mudou = false
  for (const row of data) {
    if (!byId.has(row.source_id)) {
      reg.push({ id: row.source_id, url: row.url, name: row.name, etag: "", lastMod: "", addedAt: Date.now(), count: 0 })
      byId.set(row.source_id, row)
      mudou = true
    }
  }
  if (mudou) sourcesApp._writeRegistryLocal(reg)
  return { ok: true, pulled: data.length }
}

async function reconcile() {
  const r = await push()
  if (!r.ok) return r
  return pull()
}

// Sincroniza na hora após mudancas locais (sem debounce)
function agendarPush() {
  push().catch(() => {})
}

module.exports = { push, pull, reconcile, agendarPush, registro }
