// Popula apiname nos itens do achievements.json lendo o UserGameStatsSchema_<appid>.bin
// do Steam. O schema mapeia (block, bit) -> { apiname, título/desc localizados, hash
// dos ícones }. Sem schema (jogo sem .bin de schema), é no-op — item fica como está.
// Ícones: copia o .jpg do cache do Steam pro cache local do Arcadia e devolve file://
// pro renderer poder servir sem depender da Steam.

const fs = require("fs")
const path = require("path")
const os = require("os")
const { loadKvBin } = require("./steam_bin")
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
  try { fs.mkdirSync(ICON_CACHE_DIR, { recursive: true }) } catch {}
  const dst = path.join(ICON_CACHE_DIR, `${appid}_${hash}`)
  if (!fs.existsSync(dst)) {
    try { fs.copyFileSync(src, dst) } catch { return null }
  }
  return `file://${dst}`
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

// Atualiza o achievements.json: pra cada item que tem block|bit, busca no schema e
// preenche apiname (e atualiza name/desc se o item estava vazio e o schema tem).
// Copia os ícones pro cache local e atualiza `icon`/`icongray` pro caminho file:// —
// só quando o hash do schema existe E o arquivo original existe; senão mantém o que
// tinha (URL da Steam). Devolve { updated, iconsCopied, total }.
function loadAllSchemas() {
  const store = loadAchievements()
  if (!store || typeof store !== "object") return { updated: 0, iconsCopied: 0, total: 0 }
  let updated = 0
  let iconsCopied = 0
  for (const [appid, ent] of Object.entries(store)) {
    const idx = loadSchemaForAppid(appid)
    if (!idx) continue
    let appidChanged = false
    for (const it of ent.items || []) {
      if (it.block == null || it.bit == null) continue
      const k = `${it.block}|${it.bit}`
      const sch = idx[k]
      if (!sch) continue
      if (!it.apiname && sch.apiname) { it.apiname = sch.apiname; appidChanged = true }
      if ((!it.title || !it.title.trim()) && sch.name) { it.title = sch.name; appidChanged = true }
      if ((!it.desc || !it.desc.trim()) && sch.desc) { it.desc = sch.desc; appidChanged = true }
      if (sch.icon_hash) {
        const localIcon = copyIconToCache(appid, sch.icon_hash)
        if (localIcon) {
          if (it.icon !== localIcon) { it.icon = localIcon; appidChanged = true; iconsCopied++ }
          const localGray = copyIconToCache(appid, sch.icongray_hash)
          if (localGray && it.icongray !== localGray) { it.icongray = localGray; appidChanged = true }
        }
      }
    }
    if (appidChanged) updated++
  }
  saveAchievements(store)
  return { updated, iconsCopied, total: Object.keys(store).length }
}

module.exports = { loadSchemaForAppid, loadAllSchemas, pickLocalized, copyIconToCache, STATS_DIR, ICON_DIR, ICON_CACHE_DIR }
