// Constrói (ou atualiza) os itens do achievements.json lendo o
// UserGameStatsSchema_<appid>.bin do Steam. O schema mapeia (block, bit) ->
// { apiname, título/desc localizados, hash dos ícones } — tudo que é preciso
// para montar os itens do zero, mesmo com achievements.json vazio. Appids sem
// schema (.bin ausente/inválido) ficam exatamente como estão (no-op).
// Ícones: copia o .jpg do cache do Steam pro cache local do Arcadia e devolve
// file:// pro renderer poder servir sem depender da Steam. Se o cache da Steam
// não existir, cai pro CDN da Steam (https://cdn.../apps/<appid>/<hash>) — o
// hash do schema já vem com extensão .jpg.

const fs = require("fs")
const path = require("path")
const os = require("os")
const { loadKvBin, progressMap } = require("./steam_bin")
const { loadAchievements, saveAchievements } = require("./schema")

const STATS_DIR = path.join(os.homedir(), ".local/share/Steam/appcache/stats")
const ICON_DIR = path.join(STATS_DIR, "achievement_images")
const ICON_CACHE_DIR = path.join(os.homedir(), ".local/share/arcadia/achievement_icons")

// Idioma preferido: pt-BR, en-US, en, fallback no primeiro string. Se nada bate, usa inglês.
const LOCALE_CHAIN = ["brazilian", "english", "spanish"]

function pickLocalized(map) {
  if (!map || typeof map !== "object") return ""
  for (const k of LOCALE_CHAIN) {
    if (typeof map[k] === "string" && map[k].trim()) return map[k]
  }
  // fallback: primeira string do objeto que não seja "token"
  for (const [k, v] of Object.entries(map)) {
    if (k !== "token" && typeof v === "string" && v.trim()) return v
  }
  return ""
}

// Ícone do Steam é arquivo .jpg em achievement_images/<hash>.jpg. Cópia pro cache
// local achievement_icons/<appid>_<hash>.jpg e devolve o caminho "file://" pra usar
// em <img src> no Electron. Se não achar original, devolve null (mantém URL da Steam).
function copyIconToCache(appid, hash) {
  if (!hash) return null
  const src = path.join(ICON_DIR, hash)
  if (!fs.existsSync(src)) return null
  try {
    fs.mkdirSync(ICON_CACHE_DIR, { recursive: true })
  } catch {}
  const dst = path.join(ICON_CACHE_DIR, `${appid}_${hash}`)
  if (!fs.existsSync(dst)) {
    try {
      fs.copyFileSync(src, dst)
    } catch {
      return null
    }
  }
  return `file://${dst}`
}

// Fallback quando o cache local da Steam não tem a imagem: CDN da Steam no
// formato <host>/steamcommunity/public/images/apps/<appid>/<hash>. O hash já
// vem do schema com extensão .jpg. Host validado com HTTP 200 (cloudflare);
// cdn.akamai.steamstatic.com serve o mesmo conteúdo como alternativa.
const STEAM_ICON_HOST = "https://cdn.cloudflare.steamstatic.com"

function steamIconUrl(appid, hash) {
  if (!hash) return null
  return `${STEAM_ICON_HOST}/steamcommunity/public/images/apps/${appid}/${hash}`
}

// Lê o schema de UM appid e devolve o índice block|bit → { apiname, name, desc,
// icon_hash, icongray_hash }. Devolve {} se o schema não existe ou é inválido.
function loadSchemaForAppid(appid) {
  const file = path.join(STATS_DIR, `UserGameStatsSchema_${appid}.bin`)
  if (!fs.existsSync(file)) return {}
  const kv = loadKvBin(file)
  if (!kv || !kv.stats) return {}
  const out = {}
  for (const [blkKey, blk] of Object.entries(kv.stats)) {
    if (!blk || typeof blk !== "object" || !blk.bits) continue
    for (const [bitKey, item] of Object.entries(blk.bits)) {
      if (!item || typeof item !== "object") continue
      const apiname = item.name
      if (!apiname) continue
      const display = item.display || {}
      out[`${blkKey}|${bitKey}`] = {
        apiname,
        name: pickLocalized(display.name),
        desc: pickLocalized(display.desc),
        icon_hash: display.icon || "",
        icongray_hash: display.icon_gray || display.icon || "",
      }
    }
  }
  return out
}

// Reconstrói o achievements.json a partir dos UserGameStatsSchema_*.bin da Steam:
// cria itens com block/bit/apiname/title/desc/ícones e SINCRONIZA o achieved/unlock
// com o progresso real do bin (UserGameStats_*.bin) — conquistas já ganhas na Steam
// real ou em sessões anteriores do Arcadia são marcadas como achieved automaticamente,
// sem depender do watcher ter rodado naquela sessão.
function loadAllSchemas() {
  const loaded = loadAchievements()
  const store = loaded && typeof loaded === "object" ? loaded : {}

  // Appids = união do que já está no achievements.json + todo schema .bin da Steam
  const appids = new Set(Object.keys(store))
  try {
    for (const f of fs.readdirSync(STATS_DIR)) {
      const m = /^UserGameStatsSchema_(\d+)\.bin$/.exec(f)
      if (m) appids.add(m[1])
    }
  } catch {}

  let updated = 0
  let iconsCopied = 0
  for (const appid of appids) {
    const idx = loadSchemaForAppid(appid)
    if (!idx || !Object.keys(idx).length) continue

    // Itens antigos: índice pra preservar achieved/unlock/percent/ícones
    const old = new Map()
    for (const it of (store[appid] && store[appid].items) || []) {
      if (!it) continue
      if (it.apiname) old.set("apiname:" + it.apiname, it)
      if (it.block != null && it.bit != null) old.set("bb:" + it.block + "|" + it.bit, it)
    }

    // Progresso real do bin: bits já setados pela Steam (sessões anteriores).
    // Sincroniza o achieved/unlock com o estado real — sem depender do watcher
    // ter rodado antes (bits já estavam setados no boot).
    let progress = {}
    try {
      for (const f of fs.readdirSync(STATS_DIR)) {
        const pm = /^UserGameStats_(\d+)_(\d+)\.bin$/.exec(f)
        if (pm && pm[2] === appid) {
          progress = progressMap(path.join(STATS_DIR, f))
          break
        }
      }
    } catch {}

    const items = []
    for (const [k, sch] of Object.entries(idx)) {
      const [blk, bit] = k.split("|")
      const prev = old.get("apiname:" + sch.apiname) || old.get("bb:" + k)
      const binTs = progress[`${blk}|${bit}`] || 0
      const icon = copyIconToCache(appid, sch.icon_hash) || steamIconUrl(appid, sch.icon_hash)
      const icongray =
        copyIconToCache(appid, sch.icongray_hash) || steamIconUrl(appid, sch.icongray_hash)
      if (icon && (!prev || prev.icon !== icon)) iconsCopied++
      items.push({
        apiname: sch.apiname,
        title: sch.name,
        desc: sch.desc,
        block: Number(blk),
        bit: Number(bit),
        icon: icon || (prev && prev.icon) || "",
        icongray: icongray || (prev && prev.icongray) || "",
        achieved: binTs > 0 ? true : prev ? Boolean(prev.achieved) : false,
        unlock: binTs > 0 ? binTs : prev && prev.unlock ? prev.unlock : 0,
        percent: prev && prev.percent ? prev.percent : 0,
      })
    }
    items.sort((a, b) => a.block - b.block || a.bit - b.bit)
    store[appid] = Object.assign({}, store[appid], { items })
    updated++
  }

  saveAchievements(store)
  return { updated, iconsCopied, total: Object.keys(store).length }
}

module.exports = {
  loadSchemaForAppid,
  loadAllSchemas,
  pickLocalized,
  copyIconToCache,
  steamIconUrl,
  STATS_DIR,
  ICON_DIR,
  ICON_CACHE_DIR,
}
