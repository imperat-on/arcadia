// Sync de conquistas (Supabase) — MAIN PROCESS.
// Offline-first: desbloqueios vão pra uma fila local e sobem quando der.
// Merge: "quem desbloqueou PRIMEIRO vence" (earliest wins) — no servidor via
// RPC sync_achievements (LEAST) e localmente no applyPulled.
"use strict"

const fs = require("fs")
const path = require("path")
const { getClient } = require("./client")
const { caminhoConta } = require("./conta")

const DATA_DIR =
  process.env.ARCADIA_DATA_DIR ||
  path.join(process.env.HOME || process.env.USERPROFILE || ".", ".local", "share", "arcadia")
// Fila, estado e metadados são POR CONTA (conta.js escopa por username)
const QUEUE_PATH = () => caminhoConta(path.join(DATA_DIR, "sync_queue.json"))
const STATE_PATH = () => caminhoConta(path.join(DATA_DIR, "sync_state.json"))
const ACH_PATH = () => caminhoConta(path.join(DATA_DIR, "achievements.json"))

// ---------- util ----------

function writeAtomic(file, content) {
  fs.mkdirSync(path.dirname(file), { recursive: true })
  const tmp = `${file}.tmp-${process.pid}-${Date.now()}`
  fs.writeFileSync(tmp, content, { encoding: "utf8" })
  fs.renameSync(tmp, file)
}

function readJson(file, fallback) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"))
  } catch {
    return fallback
  }
}

/**
 * Normaliza timestamp para epoch SEGUNDOS (formato que o RPC espera).
 * Aceita 10 dígitos (segundos, formato do .bin da Steam) e 13 (ms,
 * formato do `at` do achievements_store).
 */
function normalizeTs(v) {
  if (v == null) return null
  const n = Number(v)
  if (!Number.isFinite(n) || n <= 0) return null
  return n > 1e12 ? Math.floor(n / 1000) : Math.floor(n)
}

// ---------- fila ----------

function loadQueue() {
  return readJson(QUEUE_PATH(), [])
}

function saveQueue(q) {
  writeAtomic(QUEUE_PATH(), JSON.stringify(q))
}

/** Enfileira desbloqueios com dedupe por (appid, apiname) — earliest wins. */
function enqueue(items) {
  if (!Array.isArray(items) || !items.length) return loadQueue()
  const map = new Map()
  for (const it of [...loadQueue(), ...items]) {
    if (!it || !it.appid || !it.apiname) continue
    const key = `${it.appid}|${it.apiname}`
    const prev = map.get(key)
    if (!prev || (normalizeTs(it.unlocked_at) ?? Infinity) < (normalizeTs(prev.unlocked_at) ?? Infinity)) {
      map.set(key, it)
    }
  }
  const merged = [...map.values()]
  saveQueue(merged)
  return merged
}

function queueLength() {
  return loadQueue().length
}

// ---------- estado ----------

function loadState() {
  return readJson(STATE_PATH(), { lastPullAt: null, lastSyncAt: null, lastError: null })
}

function getState() {
  const st = loadState()
  return { ...st, queueLen: queueLength() }
}

function saveState(patch) {
  writeAtomic(STATE_PATH(), JSON.stringify({ ...loadState(), ...patch }))
}

// ---------- erros retryáveis ----------

function isRetryable(error) {
  const msg = String(error?.message || "")
  const code = String(error?.code || "")
  if (/permission denied|42501/i.test(`${msg} ${code}`)) return false // RLS: bug de código, não retry
  if (/^Failed to fetch|fetch failed|ECONN|ENOTFOUND|ETIMEDOUT|network|timeout|socket/i.test(msg)) return true
  if (error?.status === 429 || error?.code === "429") return true
  return true
}

// ---------- push / pull ----------

async function requireUserId() {
  const { data, error } = await getClient().auth.getUser()
  if (error || !data?.user) return null
  return data.user.id
}

/** Sobe a fila inteira (RPC idempotente: re-enviar é no-op no servidor). */
async function pushDelta() {
  const me = await requireUserId()
  if (!me) return { ok: false, error: "nao_logado", retryable: false }

  const q = loadQueue()
  if (!q.length) return { ok: true, pushed: 0 }

  const p_items = q.map((i) => ({
    appid: i.appid,
    apiname: i.apiname,
    unlocked_at: normalizeTs(i.unlocked_at),
    title: i.title ?? null,
    icon: i.icon ?? null,
    percent: i.percent ?? null,
  }))
  const { error } = await getClient().rpc("sync_achievements", { p_items })
  if (error) return { ok: false, error: error.message, retryable: isRetryable(error) }

  saveQueue([]) // enviou (ou já existia) → drena
  saveState({ lastSyncAt: Math.floor(Date.now() / 1000), lastError: null })
  return { ok: true, pushed: q.length }
}

/** Baixa o delta desde o último pull e aplica no achievements.json. */
async function pullDelta() {
  const me = await requireUserId()
  if (!me) return { ok: false, error: "nao_logado", retryable: false }

  const st = loadState()
  const p_since = st.lastPullAt ? new Date(st.lastPullAt * 1000).toISOString() : null
  const { data, error } = await getClient().rpc("pull_achievements", { p_since })
  if (error) return { ok: false, error: error.message, retryable: isRetryable(error) }

  const rows = Array.isArray(data) ? data : []
  if (rows.length) applyPulled(rows)
  saveState({ lastPullAt: Math.floor(Date.now() / 1000), lastSyncAt: Math.floor(Date.now() / 1000), lastError: null })
  return { ok: true, pulled: rows.length }
}

/**
 * Merge local: desbloqueado no servidor e não local → marca; já local com
 * timestamp anterior → mantém local (o próximo push corrige o servidor).
 */
function applyPulled(rows) {
  const store = readJson(ACH_PATH(), {})
  let mudou = false
  for (const r of rows) {
    const app = store[r.appid] || (store[r.appid] = { at: Date.now(), items: [] })
    const item = app.items.find((i) => i.apiname === r.apiname)
    const ts = Math.floor(new Date(r.unlocked_at).getTime() / 1000)
    if (item) {
      const localTs = normalizeTs(item.unlock)
      if (!item.achieved || (localTs && localTs > ts)) {
        item.achieved = true
        item.unlock = ts
        mudou = true
      }
    } else {
      // apiname desconhecido localmente (schema ainda não carregado): cria o
      // item mínimo — o reloader de schema preenche título/ícone depois.
      app.items.push({
        apiname: r.apiname,
        title: r.apiname,
        desc: "",
        icon: "",
        icongray: "",
        achieved: true,
        unlock: ts,
        percent: 100,
      })
      mudou = true
    }
  }
  if (mudou) writeAtomic(ACH_PATH(), JSON.stringify(store))
}

// ---------- engine ----------

let sincronizando = false
let retryTimer = null
let retryDelay = 5000
let debounceTimer = null
const listeners = new Set()

function emit() {
  const st = getState()
  for (const l of listeners) {
    try {
      l(st)
    } catch {
      /* ignore */
    }
  }
}

function onSyncState(cb) {
  listeners.add(cb)
  return () => listeners.delete(cb)
}

/** Push + pull em sequência, com trava de concorrência. */
async function syncNow() {
  if (sincronizando) return { ok: true, ocupado: true }
  sincronizando = true
  emit()
  try {
    const push = await pushDelta()
    if (!push.ok) {
      if (push.retryable) scheduleRetry()
      saveState({ lastError: push.error })
      emit()
      return push
    }
    const pull = await pullDelta()
    if (!pull.ok) {
      if (pull.retryable) scheduleRetry()
      saveState({ lastError: pull.error })
      emit()
      return pull
    }
    retryDelay = 5000
    emit()
    return { ok: true, pushed: push.pushed, pulled: pull.pulled }
  } finally {
    sincronizando = false
  }
}

/** Agendamento com backoff exponencial (5s → 10s → … → 5min máx). */
function scheduleRetry() {
  if (retryTimer) return
  retryTimer = setTimeout(async () => {
    retryTimer = null
    const r = await syncNow()
    if (!r.ok && r.retryable) {
      retryDelay = Math.min(retryDelay * 2, 300000)
      scheduleRetry()
    } else {
      retryDelay = 5000
    }
  }, retryDelay)
}

/** Debounce curto para o hook de desbloqueio (não atrapalha o launch). */
function scheduleNow(delayMs = 2000) {
  if (debounceTimer) clearTimeout(debounceTimer)
  debounceTimer = setTimeout(() => {
    debounceTimer = null
    syncNow()
  }, delayMs)
}

/** Reconcile completo (login/boot): push da fila + pull do delta. */
async function reconcile() {
  return syncNow()
}

module.exports = {
  normalizeTs,
  enqueue,
  queueLength,
  getState,
  syncNow,
  scheduleNow,
  scheduleRetry,
  reconcile,
  onSyncState,
  isRetryable,
  applyPulled,
  pushDelta,
  pullDelta,
}
