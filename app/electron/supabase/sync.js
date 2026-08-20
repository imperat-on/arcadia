// Sync de conquistas (backend proprio) — MAIN PROCESS.
// Offline-first: desbloqueios vão pra uma fila local e sobem quando der.
// Merge: "quem desbloqueou PRIMEIRO vence" (earliest wins) — no servidor via
// RPC sync_achievements (LEAST) e localmente no applyPulled.
"use strict"

const fs = require("fs")
const path = require("path")
const { getClient } = require("./client")
const { caminhoArquivoConta, conta: contaAtual } = require("./conta")
const {
  normalizeSyncTimestamp,
  resolveAchievementConflict,
} = require("../../../contracts")

// Fila, estado e metadados são POR CONTA (conta.js escopa por username)
const QUEUE_PATH = () => caminhoArquivoConta("sync_queue.json")
const STATE_PATH = () => caminhoArquivoConta("sync_state.json")
const ACH_PATH = () => caminhoArquivoConta("achievements.json")

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
  return normalizeSyncTimestamp(v)
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
  // A fila pode conter operações de outros domínios no futuro. Elas não são
  // interpretadas aqui, mas também não são descartadas ao atualizar conquistas.
  for (const it of [...loadQueue(), ...items]) {
    if (it?.kind && it.kind !== "achievement") continue
    if (!it || !it.appid || !it.apiname || normalizeTs(it.unlocked_at) == null) continue
    const key = `${it.appid}|${it.apiname}`
    map.set(key, resolveAchievementConflict(map.get(key), it))
  }
  const achievements = [...map.values()].sort((a, b) =>
    `${a.appid}|${a.apiname}`.localeCompare(`${b.appid}|${b.apiname}`),
  )
  const other = loadQueue().filter((it) => it?.kind && it.kind !== "achievement")
  const merged = [...other, ...achievements]
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

async function requireSyncContext() {
  const { data, error } = await getClient().auth.getUser()
  if (error || !data?.user?.id) return null
  return { userId: String(data.user.id), account: contaAtual() }
}

function contextStillActive(context) {
  return !context || contaAtual() === context.account
}

async function requireUserId() {
  const context = await requireSyncContext()
  return context?.userId || null
}

/** Sobe a fila de conquistas (RPC idempotente: re-enviar é no-op no servidor). */
async function pushDelta(context = null) {
  const ctx = context || await requireSyncContext()
  if (!ctx) return { ok: false, error: "nao_logado", retryable: false }
  if (!contextStillActive(ctx)) return { ok: false, error: "conta_trocada", retryable: false }

  const q = loadQueue().filter((item) =>
    (!item?.kind || item.kind === "achievement") &&
    item?.appid && item?.apiname && normalizeTs(item.unlocked_at) != null,
  )
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
  // Nunca grava o escopo novo se logout/login ocorreu enquanto o RPC estava
  // em voo. Operações de outros domínios permanecem na fila.
  if (!contextStillActive(ctx)) return { ok: false, error: "conta_trocada", retryable: false }

  const enviados = new Set(q.map((item) => `${item.appid}|${item.apiname}`))
  const restante = loadQueue().filter((item) => {
    if (item?.kind && item.kind !== "achievement") return true
    return !enviados.has(`${item?.appid}|${item?.apiname}`)
  })
  saveQueue(restante)
  saveState({ lastSyncAt: Math.floor(Date.now() / 1000), lastError: null })
  return { ok: true, pushed: q.length }
}

/** Baixa o delta desde o último pull e aplica no achievements.json. */
async function pullDelta(context = null) {
  const ctx = context || await requireSyncContext()
  if (!ctx) return { ok: false, error: "nao_logado", retryable: false }
  if (!contextStillActive(ctx)) return { ok: false, error: "conta_trocada", retryable: false }

  const st = loadState()
  const p_since = st.lastPullAt ? new Date(st.lastPullAt * 1000).toISOString() : null
  const { data, error } = await getClient().rpc("pull_achievements", { p_since })
  if (error) return { ok: false, error: error.message, retryable: isRetryable(error) }
  if (!contextStillActive(ctx)) return { ok: false, error: "conta_trocada", retryable: false }

  const rows = Array.isArray(data) ? data : []
  if (rows.length) applyPulled(rows, ctx)
  if (!contextStillActive(ctx)) return { ok: false, error: "conta_trocada", retryable: false }
  saveState({ lastPullAt: Math.floor(Date.now() / 1000), lastSyncAt: Math.floor(Date.now() / 1000), lastError: null })
  return { ok: true, pulled: rows.length }
}

/**
 * Merge local: usa a mesma regra pura do enqueue (earliest-wins). O contexto
 * opcional impede que um pull iniciado antes de trocar de conta grave dados
 * no diretório da conta seguinte.
 */
function applyPulled(rows, context = null) {
  if (!Array.isArray(rows) || !contextStillActive(context)) return false
  const store = readJson(ACH_PATH(), {})
  let mudou = false
  for (const r of rows) {
    if (!contextStillActive(context)) return false
    const remote = resolveAchievementConflict(null, r)
    if (!remote || remote.unlocked_at == null) continue
    const appid = remote.appid
    const app = store[appid] || (store[appid] = { at: Date.now(), items: [] })
    if (!Array.isArray(app.items)) app.items = []
    const item = app.items.find((i) => i && i.apiname === remote.apiname)
    const local = item
      ? { appid, apiname: item.apiname, achieved: item.achieved === true, unlocked_at: item.unlock }
      : null
    const merged = resolveAchievementConflict(local, remote)
    if (!merged || merged.unlocked_at == null) continue
    if (item) {
      const nextUnlock = merged.unlocked_at
      if (item.achieved !== true || normalizeTs(item.unlock) !== nextUnlock) {
        item.achieved = true
        item.unlock = nextUnlock
        mudou = true
      }
    } else {
      // apiname desconhecido localmente (schema ainda não carregado): cria o
      // item mínimo — o reloader de schema preenche título/ícone depois.
      app.items.push({
        apiname: remote.apiname,
        title: remote.title || remote.apiname,
        desc: "",
        icon: remote.icon || "",
        icongray: "",
        achieved: true,
        unlock: merged.unlocked_at,
        percent: remote.percent ?? 100,
      })
      mudou = true
    }
  }
  if (mudou && contextStillActive(context)) writeAtomic(ACH_PATH(), JSON.stringify(store))
  return mudou
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
    const context = await requireSyncContext()
    if (!context) {
      const result = { ok: false, error: "nao_logado", retryable: false }
      if (context && contextStillActive(context)) saveState({ lastError: result.error })
      emit()
      return result
    }
    const push = await pushDelta(context)
    if (!push.ok) {
      if (push.retryable) scheduleRetry()
      if (contextStillActive(context)) saveState({ lastError: push.error })
      emit()
      return push
    }
    const pull = await pullDelta(context)
    if (!pull.ok) {
      if (pull.retryable) scheduleRetry()
      if (contextStillActive(context)) saveState({ lastError: pull.error })
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

/** Sincroniza na hora apos desbloqueio (sem debounce). */
function scheduleNow(delayMs = 0) {
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

// ---------- REALTIME (canal achievements-<me>) ----------
// Sem isto, conquista desbloqueada numa maquina só aparecia na outra no
// próximo boot/login — podia demorar horas. O servidor avisa via WebSocket
// assim que sync_achievements grava algo novo, e aqui a gente puxa na hora.
// Mesmo padrão de friends.watchRequests()/biblioteca.watchChanges().
function watchChanges() {
  let channel = null
  let iniciando = null

  const stop = async () => {
    if (channel) {
      try {
        await getClient().removeChannel(channel)
      } catch {
        /* ignore */
      }
      channel = null
    }
  }

  const start = () => {
    if (iniciando) return iniciando
    iniciando = (async () => {
      await stop()
      const me = await requireUserId()
      if (!me) return
      channel = getClient().channel(`achievements-${me}`)
      channel.on(
        "postgres_changes",
        { event: "*", schema: "public", table: "user_achievements" },
        () => {
          syncNow().catch((e) => console.error("[sync] pull via realtime falhou:", e?.message))
        },
      )
      channel.subscribe()
    })().finally(() => {
      iniciando = null
    })
    return iniciando
  }

  return { start, stop }
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
  watchChanges,
}
