// Vigia de conquistas: observa os bins locais do Steam (appcache/stats) e
// emite "achievement:unlocked" quando surge uma conquista nova — funciona em
// jogos legítimos e injetados (via schema do SLScheevo). Só Steam nativo bin.
//
// Além do vigia de bins, este módulo traz o scrape sob demanda da página
// pública da Steam (fetchAchievementsForApp), usado pela loja.

const fs = require("fs")
const path = require("path")
const os = require("os")
const { loadItemIndex, ACHIEVEMENTS_STORE_FILE, STORE_TTL_MS } = require("./schema")

const { findSteamDir } = require("./../steam-path")
const STATS_DIR = path.join(findSteamDir(), "appcache", "stats")

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
        if (binfo && typeof binfo === "object" && Number(binfo.bits) & 1) {
          const k = `${blk}|${idx}`
          if (!(k in out)) out[k] = Number(binfo.unlock_time) || 0
        }
        // A primeira vez que este appid aparece (bin criado depois do boot),
        // guarda o estado atual como snapshot pra evitar re-notificar.
        if (!snapshots.has(appid)) snapshots.set(appid, prev)
      }
    }
  }
  return out
}

// --- Escrita de .bin (KeyValues binário do Steam) ----------------------------
// Steam lê o .bin a cada save do jogo. O Arcadia escreve nele pra forçar
// desbloqueio sem cliente Steam rodando. Formato idêntico ao que o Steam gera:
// byte tag + nome C-string (null-terminated) + valor binário da tag. Mapa abre
// com 0x00 (se tiver nome) e fecha com 0x08.

function writeString(name, value) {
  return Buffer.concat([
    Buffer.from([0x01]), // tag string
    Buffer.from(name + "\0", "utf-8"),
    Buffer.from(String(value), "utf-8"),
    Buffer.from([0x00]), // terminator da string
  ])
}

function writeInt32(name, value) {
  const v = Buffer.alloc(4)
  v.writeInt32LE(Number(value) | 0)
  return Buffer.concat([Buffer.from([0x02]), Buffer.from(name + "\0", "utf-8"), v])
}

function writeUInt64(name, value) {
  const v = Buffer.alloc(8)
  v.writeBigUInt64LE(BigInt(value))
  return Buffer.concat([Buffer.from([0x07]), Buffer.from(name + "\0", "utf-8"), v])
}

function writeKV(obj, name = "") {
  // obj é objeto plain. Valor objeto vira mapa (tag 0x00); nativo vira o tipo
  // da tag. Booleano vira string "1"/"0". Todo mapa fecha com 0x08 — a raiz
  // também (Steam termina o .bin com 0x08 depois do último bloco).
  const parts = []
  if (name) parts.push(Buffer.from([0x00]), Buffer.from(name + "\0", "utf-8"))
  for (const [k, v] of Object.entries(obj)) {
    if (v !== null && typeof v === "object") {
      parts.push(writeKV(v, k))
    } else if (typeof v === "string") {
      parts.push(writeString(k, v))
    } else if (typeof v === "number" && Number.isInteger(v)) {
      parts.push(writeInt32(k, v))
    } else if (typeof v === "number") {
      const f = Buffer.alloc(4)
      f.writeFloatLE(v)
      parts.push(Buffer.from([0x0b]), Buffer.from(k + "\0", "utf-8"), f)
    } else if (typeof v === "bigint") {
      parts.push(writeUInt64(k, v))
    } else if (typeof v === "boolean") {
      parts.push(writeString(k, v ? "1" : "0"))
    }
  }
  parts.push(Buffer.from([0x08])) // fim de mapa
  return Buffer.concat(parts)
}

// Força o unlock de UMA conquista no .bin local do Steam: lê o .bin atual,
// seta o bit na máscara data do bloco e grava AchievementTimes[block][bit] =
// epoch, depois regrava (com backup .arcadia.bak antes). Formato real do
// Steam: um único mapa raiz "cache" com crc, PendingChanges e um mapa por
// bloco ("0", "1", ...) com "data" (máscara de bits int32) e
// "AchievementTimes" (bit -> epoch). O parser achata a raiz — o que
// loadKvBin devolve é { crc, PendingChanges, <bloco>: ... }. block/bit vêm
// do schema (0-31 por bloco). Sem o .bin (ou inválido), lança.
function writeAchievementUnlock(file, block, bit, epoch = Math.floor(Date.now() / 1000)) {
  const kv = loadKvBin(file)
  if (!kv || typeof kv !== "object") throw new Error("bin não existe ou é inválido")

  if (kv.crc === undefined) kv.crc = 0
  if (kv.PendingChanges === undefined) kv.PendingChanges = 0

  const blk = String(block)
  const b = String(bit)
  const blkMap = kv[blk] && typeof kv[blk] === "object" ? kv[blk] : (kv[blk] = {})
  // data é máscara de bits int32; bit >= 31 vira negativo — writeInt32 preserva.
  blkMap.data = Number(blkMap.data) | (1 << (Number(bit) & 31))
  blkMap.AchievementTimes =
    blkMap.AchievementTimes && typeof blkMap.AchievementTimes === "object"
      ? blkMap.AchievementTimes
      : {}
  blkMap.AchievementTimes[b] = epoch

  // Regrava embrulhado no mapa "cache" (único mapa da raiz — como a Steam
  // gera). Chaves numéricas dos blocos vêm antes das strings no objeto, mas
  // o parser lê o primeiro mapa da raiz como raiz achatada, então ok.
  const buf = writeKV({ cache: kv })
  try {
    fs.copyFileSync(file, file + ".arcadia.bak")
  } catch {}
  const tmp = file + ".tmp"
  fs.writeFileSync(tmp, buf)
  fs.renameSync(tmp, file)
  return { ok: true, epoch }
}

// Inicia o vigia de bins da Steam. onUnlock(payload) recebe
// {appid,title,desc,icon,percent,unlock,key}. key = "block|bit" — permite ao
// caller marcar o item no achievements.json.
function startSteamBinWatcher(onUnlock) {
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
    const prev = snapshots.get(appid) || new Set()
    const curMap = progressMap(file)
    const cur = new Set(Object.keys(curMap))
    // O snapshot de conquistas pode ter mudado — recarrega leve.
    itemIndex = loadItemIndex()
    for (const k of cur) {
      if (prev.has(k)) continue
      prev.add(k)
      const it = itemIndex[appid]?.[k]
      if (it && curMap[k] > 0) {
        onUnlock({
          appid,
          key: k,
          title: it.title,
          desc: it.desc,
          icon: it.icon,
          percent: it.percent,
          unlock: curMap[k],
        })
      } else if (!it && curMap[k] > 0) {
        // Progresso novo sem entrada no índice: achievements.json ausente ou
        // sem esta conquista. Antes falhava em silêncio (sem toast, sem pista).
        console.warn(
          `[achievements] desbloqueio ${k} p/ appid ${appid} sem índice (achievements.json ausente/incompleto)`,
        )
      }
    }
    // Bin criado depois do boot (não estava no snapshot inicial):
    // guarda o estado atual p/ não re-notificar no próximo poll.
    if (!snapshots.has(appid)) snapshots.set(appid, prev)
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

// --- Loja: scrape sob demanda da página pública -----------------------------
// Mesma lógica do parser da página pública da Steam: bloco achieveRow,
// primeira img do bloco (sem classe), título h3 e descrição h5. A página não
// expõe o apiname — o chamador gera sintético.
function parseAchievementsHtml(page) {
  const out = []
  const rowRe = /<div class="achieveRow[^"]*"[^>]*>([\s\S]*?)<div style="clear: both;">/g
  let m
  while ((m = rowRe.exec(page))) {
    const b = m[1]
    const iconM = /<img src="([^"]+)"/.exec(b)
    const titleM = /<h3>([\s\S]*?)<\/h3>/.exec(b)
    if (!iconM || !titleM) continue
    const descM = /<h5>([\s\S]*?)<\/h5>/.exec(b)
    const unesc = (s) =>
      s
        .replace(/&amp;/g, "&")
        .replace(/&quot;/g, '"')
        .replace(/&#39;/g, "'")
        .replace(/&lt;/g, "<")
        .replace(/&gt;/g, ">")
    out.push({
      title: unesc(titleM[1]).trim(),
      desc: descM ? unesc(descM[1]).trim() : "",
      icon: iconM[1],
      icongray: iconM[1],
    })
  }
  return out
}

// Conquistas de QUALQUER jogo Steam (loja incluso), fetch direto da página
// pública — Steam pede consentimento de idade pros maiores de 18.
function fetchConsentUrl(appid) {
  return (
    "https://steamcommunity.com/stats/" + appid + "/achievements?l=brazilian&gid=0&birthyear=2005"
  )
}

// Conquistas do jogo (não precisa estar na biblioteca). Cache próprio em
// achievements_store.json (30d) — jogos da loja ficam fora do achievements.json.
async function fetchAchievementsForApp(appid) {
  const storePath = ACHIEVEMENTS_STORE_FILE()
  let store = {}
  try {
    store = JSON.parse(fs.readFileSync(storePath, "utf-8"))
  } catch {}

  const ent = store[appid]
  if (ent && ent.items?.length && Date.now() - ent.at < STORE_TTL_MS) {
    return ent.items
  }

  const ctl = AbortSignal.timeout(15000)
  const headers = { "User-Agent": "Mozilla/5.0", "Accept-Language": "pt-BR,pt;q=0.9" }
  const res = await fetch(fetchConsentUrl(appid), { headers, signal: ctl })
  if (!res.ok) throw new Error("HTTP " + res.status)
  const page = await res.text()

  // Fallback: página pede confirmação de idade — refaz com consentimento.
  let items = parseAchievementsHtml(page)
  if (!items.length && page.includes("app_agegate")) {
    const res2 = await fetch(fetchConsentUrl(appid) + "&agree_to_agegate=1", {
      headers,
      signal: ctl,
    })
    if (res2.ok) items = parseAchievementsHtml(await res2.text())
  }

  const out = items.map((it, i) => ({
    apiname: "ach_" + String(i + 1).padStart(2, "0"),
    title: it.title,
    desc: it.desc,
    icon: it.icon,
    icongray: it.icongray,
    block: null,
    bit: null,
    achieved: false,
    unlock: 0,
    percent: 0,
  }))

  // Vazio não vai pro cache: pode ser falha de parse transitória, e um cache
  // vazio fresco bloquearia a loja por 30d. Jogo sem conquistas refaz (1 fetch
  // por abertura, barato) em vez de envenenar o store.
  if (!out.length) return out
  store[appid] = { at: Date.now(), items: out }
  const tmp = storePath + ".tmp"
  fs.writeFileSync(tmp, JSON.stringify(store))
  fs.renameSync(tmp, storePath)

  return out
}

module.exports = {
  readKv,
  loadKvBin,
  progressMap,
  writeString,
  writeInt32,
  writeUInt64,
  writeKV,
  writeAchievementUnlock,
  startSteamBinWatcher,
  fetchAchievementsForApp,
  STATS_DIR,
}
