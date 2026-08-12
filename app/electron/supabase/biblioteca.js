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

  // Jogos possuídos (owned.js) que não são custom: sobem com o título real
  // (do pending_games ou do id) pra não virar nome feio (ex: "steam:3240220")
  // na outra máquina. Reenvia quando o título mudou em relação ao watermark —
  // sem isto, um jogo que subiu com título feio ("Steam <appid>") nunca era
  // corrigido nas outras máquinas.
  const owned = ownedSet()
  const pendentes = readJson(PENDING(), [])
  const tituloDe = (id) => {
    const p = pendentes.find((x) => x.id === id)
    return p?.title || id
  }
  for (const id of owned) {
    if (ids.has(id)) continue
    const prev = enviados[id]
    const titulo = tituloDe(id)
    if (!prev || prev.title !== titulo) {
      p_lib.push({ appid: id, title: titulo, platform: "windows" })
    }
  }

  // Removidos localmente (sumiram do arquivo, mas já tinham sido enviados).
  // Cobre custom (fora do custom_games.json) E steam (fora do owned_games):
  // sem isto, remover um jogo steam numa maquina nao sumia na outra — o
  // watermark ficava e o push nunca marcava removed.
  for (const id of Object.keys(enviados)) {
    if (!ids.has(id) && !owned.has(id)) p_lib.push({ appid: id, removed: true })
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
  // Stubs pending existentes (para nao duplicar jogos steam: vindos do servidor)
  const pendentes = readJson(PENDING(), [])
  const pendentesIds = new Set(pendentes.map((p) => p && p.id))
  let pendentesMudou = false
  for (const row of data) {
    if (owned !== null && !owned.has(row.appid)) {
      owned.add(row.appid)
      ownedMudou = true
    }
    const ehSteam = String(row.appid).startsWith("steam:")
    if (ehSteam) {
      // Jogos steam:* do servidor entram na POSSE e ganham um stub PENDING se
      // nao existirem localmente. Sem o stub, o jogo adicionado na loja de
      // OUTRA maquina nao aparecia na biblioteca aqui (o library.json global
      // so tem o que a Steam local indexou). O stub sera limpo pelo
      // limparPendentesIndexados no proximo reindex se o jogo for instalado.
      if (!ids.has(row.appid) && !pendentesIds.has(row.appid)) {
        const appid = String(row.appid).replace(/^steam:/, "")
        const base = "https://cdn.cloudflare.steamstatic.com/steam/apps/" + appid
        pendentes.push({
          id: row.appid,
          title: (row.title && row.title !== row.appid && String(row.title).trim()) || `Steam ${appid}`,
          launcher: "steam",
          launch_cmd: ["steam", `steam://rungameid/${appid}`],
          installed: false,
          cover: `${base}/library_600x900.jpg`,
          hero: `${base}/library_hero.jpg`,
          logo: `${base}/logo.png`,
          pendente: true,
        })
        pendentesIds.add(row.appid)
        pendentesMudou = true
      } else if (pendentesIds.has(row.appid)) {
        // Stub pending já existe mas com título feio (ex: "Steam 12345" ou
        // "steam:12345"): atualiza quando o servidor manda um nome melhor.
        // O pull anterior podia ter criado o stub antes de o servidor ter o
        // título real (o push da outra máquina ainda não tinha reenviado).
        const p = pendentes.find((x) => x && x.id === row.appid)
        if (p) {
          const atual = String(p.title || "")
          const feio = atual === `Steam ${String(row.appid).replace(/^steam:/, "")}` || atual === row.appid
          const novo = row.title && row.title !== row.appid ? String(row.title).trim() : ""
          if (feio && novo && novo !== atual) {
            p.title = novo
            pendentesMudou = true
          }
        }
      }
    } else if (!ids.has(row.appid)) {
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
  if (pendentesMudou) writeJson(PENDING(), pendentes)
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

// ---------- PUSH imediato (sem debounce) ----------
// Sincroniza na hora após mudanças locais. A proteção de troca de conta
// fica no push() (que usa a sessão atual via requireUserId).
function agendarPush() {
  push().catch((e) => console.error("[biblioteca] push falhou:", e?.message))
}

module.exports = { push, pull, reconcile, agendarPush, onChanged }
