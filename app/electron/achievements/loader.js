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
const { loadKvBin, progressMap, fetchAchievementsForApp } = require("./steam_bin")
const { loadAchievements, saveAchievements } = require("./schema")
const { log } = require("./../debug")
const { caminhoArquivoConta } = require("./../supabase/conta")
const { readLibraryFile } = require("./../library-store")

const { findSteamDir } = require("./../steam-path")
const { dataPath } = require("./../runtime-paths")
const STATS_DIR = path.join(findSteamDir(), "appcache", "stats")
const ICON_DIR = path.join(STATS_DIR, "achievement_images")
const ICON_CACHE_DIR = dataPath("achievement_icons")

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

// Apinames reais por appid na ordem do scrape HTML. A API pública
// GetGlobalAchievementPercentagesForApp expõe o nome interno sem chave e a
// página de conquistas expõe título/desc/ícone na mesma ordem. Fallback pra
// jogos sem UserGameStatsSchema_*.bin (repacks/crackeados, ex. Cyberpunk via
// emulador), pra lista e sync não ficarem com só 1 item mínimo.
async function loadSchemaFallback(appid) {
  let items
  try {
    items = await fetchAchievementsForApp(appid)
  } catch (e) {
    log("achievements/fallback-fetch", e)
    return {}
  }
  if (!items || !items.length) return {}
  // Apinames reais (mesma ordem do HTML). Se falhar, gera sintético.
  let apinames = []
  try {
    const res = await fetch(
      "https://api.steampowered.com/ISteamUserStats/GetGlobalAchievementPercentagesForApp/v2/?gameid=" +
        appid,
    )
    const d = await res.json()
    apinames = (d.achievementpercentages?.achievements || []).map((a) => a.name)
  } catch (e) {
    log("achievements/fallback-apinames", e)
  }
  const out = {}
  for (let i = 0; i < items.length; i++) {
    const it = items[i]
    if (!it || !it.title) continue
    // Apiname real quando a API responde (mesma ordem do HTML), senão o
    // sintético que o scrape já carimbou (ach_##).
    const apiname = apinames[i] || it.apiname || "ach_" + String(i + 1).padStart(2, "0")
    out[i + "|0"] = {
      apiname,
      name: it.title,
      desc: it.desc || "",
      icon_hash: it.icon || "",
      icongray_hash: it.icongray || it.icon || "",
    }
  }
  return out
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
async function loadAllSchemas() {
  const loaded = loadAchievements()
  const store = loaded && typeof loaded === "object" ? loaded : {}

  // Appids = união do que já está no achievements.json + todo schema .bin da Steam
  // + appids dos jogos da biblioteca configurados (mesmo sem bin — repacks e
  // voices38/crackeados nativos NÃO têm UserGameStatsSchema_*.bin, mas o schema
  // precisa ser criado via API pública, senão o prepareUplayInstallation falha
  // com "catálogo UPC vazio"). No Linux o bin é gerado pelo Proton; no Windows
  // nativo não existe, então o appid nunca entrava no loop.
  const appids = new Set(Object.keys(store))
  try {
    for (const f of fs.readdirSync(STATS_DIR)) {
      const m = /^UserGameStatsSchema_(\d+)\.bin$/.exec(f)
      if (m) appids.add(m[1])
    }
  } catch (e) {
    log("achievements/listar-bins", e)
  }
  // Jogos configurados na biblioteca/contas: garantem que o fallback da API
  // rode para appids sem bin (crackeados/voices38 que rodam nativos).
  try {
    const lib = readLibraryFile(dataPath("library.json"))
    for (const g of lib?.games || []) {
      const m = /^steam:(\d+)$/.exec(String(g?.id || ""))
      if (m) appids.add(m[1])
    }
    const settings = JSON.parse(fs.readFileSync(caminhoArquivoConta("game_settings.json"), "utf-8"))
    for (const key of Object.keys(settings || {})) {
      const m = /^steam:(\d+)$/.exec(String(key))
      if (m) appids.add(m[1])
    }
  } catch (e) {
    log("achievements/listar-appids-biblioteca", e)
  }

  let updated = 0
  let iconsCopied = 0
  for (const appid of appids) {
    let idx = loadSchemaForAppid(appid)
    // Sem bin do Steam (crackeado/repack): busca o schema na API pública.
    if (!idx || !Object.keys(idx).length) {
      idx = await loadSchemaFallback(appid)
    }
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
    } catch (e) {
      log("achievements/progresso-bin", e)
    }

    const items = []
    // Desbloqueios novos que o sync ainda não viu: sobe pro servidor. A fila
    // deduplica por (appid, apiname), então re-enviar é seguro e o RPC é
    // idempotente (quem desbloqueou primeiro vence).
    const p_sync = []
    for (const [k, sch] of Object.entries(idx)) {
      const [blk, bit] = k.split("|")
      const prev = old.get("apiname:" + sch.apiname) || old.get("bb:" + k)
      const binTs = progress[`${blk}|${bit}`] || 0
      // Ícone: hash local do bin (copia pro cache ou monta URL da Steam) ou URL
      // completa vinda do fallback de jogos sem bin (repack/crackeado).
      const resolveIcon = (hash) =>
        hash.startsWith("http") ? hash : copyIconToCache(appid, hash) || steamIconUrl(appid, hash)
      const icon = resolveIcon(sch.icon_hash)
      if (icon && (!prev || prev.icon !== icon)) iconsCopied++
      const achieved = binTs > 0 ? true : prev ? Boolean(prev.achieved) : false
      const unlock = binTs > 0 ? binTs : prev && prev.unlock ? prev.unlock : 0
      // Enfileira TODAS as conquistas pro sync (desbloqueadas E bloqueadas).
      // Conquistas desbloqueadas sobem com unlocked_at real; bloqueadas sobem
      // com unlocked_at=0 para que outro dispositivo saiba que existem.
      // O merge no servidor usa "achieved OR" — se qualquer máquina desbloqueou,
      // fica desbloqueado em todas.
      if (sch.apiname && (!prev || prev.achieved !== achieved || prev.unlock !== unlock)) {
        p_sync.push({
          appid,
          apiname: sch.apiname,
          unlocked_at: unlock || 0,
          achieved,
          title: sch.name,
          icon,
          percent: prev && prev.percent ? prev.percent : 0,
        })
      }
      items.push({
        apiname: sch.apiname,
        title: sch.name,
        desc: sch.desc,
        block: Number(blk),
        bit: Number(bit),
        icon: icon || (prev && prev.icon) || "",
        icongray: resolveIcon(sch.icongray_hash) || (prev && prev.icongray) || "",
        achieved,
        unlock,
        percent: prev && prev.percent ? prev.percent : 0,
      })
    }
    if (p_sync.length) {
      try {
        const syncMod = require("./../supabase/sync")
        syncMod.enqueue(p_sync)
        syncMod.scheduleNow()
      } catch (e) {
        log("achievements/enqueue-sync", e)
      }
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
