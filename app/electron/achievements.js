// Vigia de conquistas: observa os bins locais do Steam (appcache/stats) e
// emite "achievement:unlocked" quando surge uma conquista nova — funciona em
// jogos legítimos e injetados (via schema do SLScheevo).

const fs = require("fs")
const path = require("path")
const os = require("os")

const STATS_DIR = path.join(os.homedir(), ".local/share/Steam/appcache/stats")
const ACHIEVEMENTS_FILE = path.join(os.homedir(), ".local/share/arcadia/achievements.json")

// --- Parser mínimo de KeyValues binário do Steam ---------------------------
function readKv(buf, pos) {
  const t = buf[pos]
  pos += 1
  let end = buf.indexOf(0, pos)
  const name = buf.toString("utf8", pos, end)
  pos = end + 1
  if (t === 0x00) {
    const val = {}
    while (buf[pos] !== 0x08) {
      const r = readKv(buf, pos)
      val[r[0]] = r[1]
      pos = r[2]
    }
    return [name, val, pos + 1]
  }
  if (t === 0x01) {
    end = buf.indexOf(0, pos)
    return [name, buf.toString("utf8", pos, end), end + 1]
  }
  if (t === 0x02) return [name, buf.readInt32LE(pos), pos + 4]
  if (t === 0x07) return [name, Number(buf.readBigUInt64LE(pos)), pos + 8]
  if (t === 0x0a) return [name, Number(buf.readBigInt64LE(pos)), pos + 8]
  if (t === 0x0b) return [name, buf.readFloatLE(pos), pos + 4]
  throw new Error("tipo KV desconhecido: " + t)
}

function loadKvBin(file) {
  try {
    const buf = fs.readFileSync(file)
    return readKv(buf, 0)[1]
  } catch {
    return null
  }
}

// (block, bit) -> epoch, lido do bin de progresso (formato novo e antigo).
function progressMap(file) {
  const kv = loadKvBin(file)
  if (!kv || typeof kv !== "object") return {}
  const out = {}
  for (const [blk, bval] of Object.entries(kv)) {
    if (!bval || typeof bval !== "object" || bval.data === undefined) continue
    const times = bval.AchievementTimes
    if (times && typeof times === "object") {
      for (const [idx, ts] of Object.entries(times)) {
        out[`${blk}|${idx}`] = Number(ts) || 0
      }
    }
    const bits = bval.bits
    if (bits && typeof bits === "object") {
      for (const [idx, binfo] of Object.entries(bits)) {
        if (binfo && typeof binfo === "object" && (Number(binfo.bits) & 1)) {
          const k = `${blk}|${idx}`
          if (!(k in out)) out[k] = Number(binfo.unlock_time) || 0
        }
      }
    }
  }
  return out
}

// appid -> (block|bit -> item do achievements.json)
function loadItemIndex() {
  try {
    const store = JSON.parse(fs.readFileSync(ACHIEVEMENTS_FILE, "utf-8"))
    const idx = {}
    for (const [appid, ent] of Object.entries(store)) {
      const map = {}
      for (const it of ent.items || []) {
        if (it.block !== undefined && it.bit !== undefined) {
          map[`${it.block}|${it.bit}`] = it
        }
      }
      if (Object.keys(map).length) idx[appid] = map
    }
    return idx
  } catch {
    return {}
  }
}

// Inicia o vigia. onUnlock(payload) recebe {appid,title,desc,icon,percent,unlock}.
function startAchievementWatcher(onUnlock) {
  let itemIndex = loadItemIndex()
  const snapshots = new Map() // appid -> Set("block|bit")
  const fileRe = /^UserGameStats_(\d+)_(\d+)\.bin$/

  const snap = (appid, file) => {
    const set = new Set(Object.keys(progressMap(file)))
    snapshots.set(appid, set)
    return set
  }

  // Snapshot inicial: não dispara toast para o que já estava desbloqueado.
  try {
    for (const f of fs.readdirSync(STATS_DIR)) {
      const m = fileRe.exec(f)
      if (m) snap(m[2], path.join(STATS_DIR, f))
    }
  } catch {
    return () => {}
  }

  const check = (fname) => {
    const m = fileRe.exec(fname || "")
    if (!m) return
    const appid = m[2]
    const file = path.join(STATS_DIR, fname)
    const prev = snapshots.get(appid) || snap(appid, file)
    const curMap = progressMap(file)
    const cur = new Set(Object.keys(curMap))
    // índice pode ter mudado (reindexação) — recarrega leve
    itemIndex = loadItemIndex()
    for (const k of cur) {
      if (prev.has(k)) continue
      prev.add(k)
      const it = itemIndex[appid]?.[k]
      if (it && curMap[k] > 0) {
        onUnlock({
          appid,
          title: it.title,
          desc: it.desc,
          icon: it.icon,
          percent: it.percent,
          unlock: curMap[k],
        })
      }
    }
  }

  let debounce = null
  let watcher = null
  try {
    watcher = fs.watch(STATS_DIR, (_ev, fname) => {
      if (!fileRe.test(fname || "")) return
      clearTimeout(debounce)
      debounce = setTimeout(() => check(fname), 3000) // Steam grava com atraso
    })
  } catch {}

  // Fallback: alguns jogos só gravam o bin ao fechar/mudar de foco.
  const poll = setInterval(() => {
    try {
      for (const f of fs.readdirSync(STATS_DIR)) {
        if (fileRe.test(f)) check(f)
      }
    } catch {}
  }, 15000)

  return () => {
    watcher?.close()
    clearInterval(poll)
    clearTimeout(debounce)
  }
}

module.exports = { startAchievementWatcher, progressMap }
