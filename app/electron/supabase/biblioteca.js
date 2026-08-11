"use strict"

// Sync de BIBLIOTECA (jogos custom) + HORAS jogadas — MAIN PROCESS.
// Cada conta tem a própria coleção no servidor (user_library/user_playtime).
//
// PUSH (2s após mudanças locais, debounce):
//   - jogos custom: diff local vs watermark (libPush) → upsert/removidos
//   - horas: delta = total local - último enviado → servidor ACUMULA
// PULL (no login): traz os jogos que faltam (exe vazio — usuário configura)
//   e sobe o display de horas pro total da conta (maior vence localmente).
//
// Watermarks ficam no sync_state.json (por conta, via conta.js) sob as chaves
// libPush / playtimePush — mesmas chaves não usadas pelo sync de conquistas.

const fs = require("fs")
const { getClient } = require("./client")
const { caminhoArquivoConta } = require("./conta")
const { ownedSet, readOwned } = require("../owned")
const { conta } = require("./conta")

const CUSTOM = () => caminhoArquivoConta("custom_games.json")
const PENDING = () => caminhoArquivoConta("pending_games.json")
const OVERRIDES = () => caminhoArquivoConta("overrides.json")
const STATE = () => caminhoArquivoConta("sync_state.json")
const OWNED = () => caminhoArquivoConta("owned_games.json")

let listeners = []
function onChanged(cb) {
  listeners.push(cb)
}
function avisar(mudou) {
  if (mudou) for (const cb of listeners) cb("library:changed")
}

// ---------- util ----------
function readJson(file, def) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"))
  } catch {
    return def
  }
}
function writeJson(file, obj) {
  const tmp = file + ".tmp"
  fs.writeFileSync(tmp, JSON.stringify(obj, null, 2))
  fs.renameSync(tmp, file)
}
function loadState() {
  return readJson(STATE(), {})
}
function saveState(st) {
  writeJson(STATE(), st)
}

async function usuarioAtual() {
  const { data } = await getClient().auth.getUser()
  return data?.user ?? null
}

// ---------- PUSH ----------
async function push() {
  const user = await usuarioAtual()
  if (!user) return

  const st = loadState()
  const enviados = st.libPush || {}
  const wp = st.playtimePush || {}

  // Jogos custom: diff local vs watermark
  const lib = readJson(CUSTOM(), [])
  const ids = new Set(lib.map((g) => g.id))
  const p_lib = []
  for (const g of lib) {
    const prev = enviados[g.id]
    const platform = g.platform || "windows"
    if (!prev || prev.title !== (g.title || "") || prev.platform !== platform) {
      p_lib.push({ appid: g.id, title: g.title || g.id, platform })
    }
  }
  // Removidos localmente (sumiram do arquivo, mas já tinham sido enviados)
  for (const id of Object.keys(enviados)) {
    if (!ids.has(id)) p_lib.push({ appid: id, removed: true })
  }

  // Jogos possuídos (owned.js) que não são custom e nunca subiram: sobem
  // com o título real (do pending_games ou do id) pra não virar nome feio
  // (ex: "steam:3240220") na outra máquina. Ids já cobertos pelo diff de
  // custom acima ou que já têm watermark ficam de fora.
  const pendentes = readJson(PENDING(), [])
  const tituloDe = (id) => {
    const p = pendentes.find((x) => x.id === id)
    return p?.title || id
  }
  for (const id of ownedSet()) {
    if (ids.has(id) || enviados[id]) continue
    p_lib.push({ appid: id, title: tituloDe(id), platform: "windows" })
  }

  // Horas: delta acumulado desde o último push
  const overrides = readJson(OVERRIDES(), {})
  const p_playtime = []
  for (const [gid, data] of Object.entries(overrides)) {
    const total = Number(data?.playtime_added_minutes) || 0
    const base = Number(wp[gid]) || 0
    if (total > base) p_playtime.push({ appid: gid, minutes: total - base })
  }

  if (!p_lib.length && !p_playtime.length) return

  const { error } = await getClient().rpc("push_library", { p_lib, p_playtime })
  if (error) {
    console.error("[biblioteca] push falhou:", error.message)
    return
  }

  // Atualiza watermarks
  for (const g of p_lib) {
    if (g.removed) delete enviados[g.appid]
    else enviados[g.appid] = { title: g.title, platform: g.platform || "windows" }
  }
  for (const p of p_playtime) wp[p.appid] = (Number(wp[p.appid]) || 0) + p.minutes
  st.libPush = enviados
  st.playtimePush = wp
  saveState(st)
}

// ---------- PULL ----------
async function pull() {
  const user = await usuarioAtual()
  if (!user) return false

  const { data, error } = await getClient().rpc("pull_library")
  if (error || !Array.isArray(data)) {
    console.error("[biblioteca] pull falhou:", error?.message || "sem dados")
    return false
  }

  let mudou = false
  const st = loadState()
  const wp = st.playtimePush || {}

  // Posse: owned_games.json ausente (null) significa "possui tudo" (ainda
  // nao migrou, ver constraint 7). Nesse caso o pull nao mexe na posse, so
  // quando ja existe arquivo real e que o appid novo entra.
  const rawOwned = readOwned()
  const owned = rawOwned === null ? null : new Set(rawOwned)
  let ownedMudou = false

  // Jogos que faltam localmente entram como custom (exe vazio — usuário
  // configura na máquina nova; título/plataforma vêm do servidor)
  const lib = readJson(CUSTOM(), [])
  const ids = new Set(lib.map((g) => g.id))
  for (const row of data) {
    if (owned !== null && !owned.has(row.appid)) {
      owned.add(row.appid)
      ownedMudou = true
    }
    if (!ids.has(row.appid)) {
      lib.push({
        id: row.appid,
        title: row.title || row.appid,
        launcher: "custom",
        platform: row.platform || "windows",
        exe: "",
        installed: false,
      })
      mudou = true
    } else {
      const g = lib.find((x) => x.id === row.appid)
      if (row.title && (!g.title || g.title === g.id)) {
        g.title = row.title
        mudou = true
      }
    }
  }
  if (mudou) writeJson(CUSTOM(), lib)
  if (ownedMudou) {
    writeJson(OWNED(), [...owned])
    mudou = true
  }

  // Horas: total da conta > local → display local sobe + watermark acompanha
  const overrides = readJson(OVERRIDES(), {})
  for (const row of data) {
    const total = Number(row.minutes) || 0
    const local = Number(overrides[row.appid]?.playtime_added_minutes) || 0
    if (total > local) {
      overrides[row.appid] = { ...(overrides[row.appid] || {}), playtime_added_minutes: total }
      wp[row.appid] = total
      mudou = true
    }
  }
  if (mudou) writeJson(OVERRIDES(), overrides)

  st.playtimePush = wp
  saveState(st)
  return mudou
}

// ---------- RECONCILE (login / boot) ----------
async function reconcile() {
  await push()
  const mudou = await pull()
  avisar(mudou)
}

// ---------- PUSH agendado (debounce pós-mudança local) ----------
let timer = null
function agendarPush() {
  if (timer) clearTimeout(timer)
  // Captura a conta no momento do agendamento. Se o usuário trocar de conta
  // antes do debounce (2s) disparar, o push não deve subir os dados da conta
  // antiga na conta nova (vazamento por timing).
  const contaNoAgendamento = conta()
  timer = setTimeout(() => {
    timer = null
    if (conta() !== contaNoAgendamento) return
    push().catch((e) => console.error("[biblioteca] push agendado falhou:", e?.message))
  }, 2000)
}

module.exports = { push, pull, reconcile, agendarPush, onChanged }
