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
const path = require("path")
const { getClient } = require("./client")
const { caminhoConta } = require("./conta")

const DATA_DIR =
  process.env.ARCADIA_DATA_DIR ||
  path.join(process.env.HOME || process.env.USERPROFILE || ".", ".local", "share", "arcadia")

const CUSTOM = () => caminhoConta(path.join(DATA_DIR, "custom_games.json"))
const OVERRIDES = () => caminhoConta(path.join(DATA_DIR, "overrides.json"))
const STATE = () => caminhoConta(path.join(DATA_DIR, "sync_state.json"))

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

  // Jogos que faltam localmente entram como custom (exe vazio — usuário
  // configura na máquina nova; título/plataforma vêm do servidor)
  const lib = readJson(CUSTOM(), [])
  const ids = new Set(lib.map((g) => g.id))
  for (const row of data) {
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
  timer = setTimeout(() => {
    timer = null
    push().catch((e) => console.error("[biblioteca] push agendado falhou:", e?.message))
  }, 2000)
}

module.exports = { push, pull, reconcile, agendarPush, onChanged }
