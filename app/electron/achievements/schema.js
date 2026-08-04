// Schema das conquistas locais: achievements.json (o índice completo que o
// index.py grava) e o store de scrape da loja. Tudo mora em
// ~/.local/share/arcadia — o mesmo diretório de dados do app.

const fs = require("fs")
const path = require("path")
const os = require("os")

const ACHIEVEMENTS_FILE = path.join(os.homedir(), ".local/share/arcadia/achievements.json")
const ACHIEVEMENTS_STORE_FILE = path.join(
  os.homedir(),
  ".local/share/arcadia/achievements_store.json",
)
const STORE_TTL_MS = 30 * 24 * 3600 * 1000

// Lê achievements.json por inteiro: { appid: { items: [...] } }. Devolve {}
// se o arquivo não existe ou está corrompido — nunca lança.
function loadAchievements() {
  try {
    return JSON.parse(fs.readFileSync(ACHIEVEMENTS_FILE, "utf-8"))
  } catch {
    return {}
  }
}

// Grava achievements.json de forma atômica (tmp + rename): uma queda no meio
// do write não deixa o arquivo truncado, e o rename é atômico dentro do mesmo
// sistema de arquivos — ou fica o antigo, ou fica o novo.
function saveAchievements(store) {
  const tmp = ACHIEVEMENTS_FILE + ".tmp"
  fs.writeFileSync(tmp, JSON.stringify(store))
  fs.renameSync(tmp, ACHIEVEMENTS_FILE)
}

// appid -> (block|bit -> item do achievements.json). Índice leve que o vigia
// de bins da Steam usa para resolver (block,bit) em conquistas.
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

module.exports = {
  ACHIEVEMENTS_FILE,
  ACHIEVEMENTS_STORE_FILE,
  STORE_TTL_MS,
  loadAchievements,
  saveAchievements,
  loadItemIndex,
}
