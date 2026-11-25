// JavaScript · summary.js · Electron main · read-only trophy summary
const fs = require("node:fs")
const { ACHIEVEMENTS_FILE } = require("./schema")
const { consolidarItens } = require("./consolidar")
const { platinumForGame } = require("./platinum")

function readEntry(appid) {
  try {
    return JSON.parse(fs.readFileSync(ACHIEVEMENTS_FILE(), "utf8"))?.[appid]
  } catch (error) {
    if (error.code !== "ENOENT") throw error
  }
}

async function getTrophySummary(appid, {
  readEntry: read = readEntry,
  fetchCatalog = require("./steam_bin").fetchAchievementsForApp,
  accountKey = ACHIEVEMENTS_FILE,
} = {}) {
  if (typeof appid !== "string" || !/^\d+$/.test(appid)) return null
  const account = accountKey()
  let entry
  let stale = false
  try { entry = read(appid) } catch { stale = true }

  const local = Array.isArray(entry?.items) ? entry.items.filter(Boolean) : []
  let items = consolidarItens(local)
  let catalog = local.length ? "partial" : "unknown"
  if (entry?.trophyCatalog === "confirmed" && (entry.trophyCatalogSize ?? local.length) === items.length) {
    catalog = local.length ? "confirmed" : "empty"
  }
  if (catalog === "unknown") {
    try {
      const listed = await fetchCatalog(appid)
      if (listed.length) {
        items = listed
        catalog = "known"
      }
    } catch { stale = true }
  }
  if (accountKey() !== account) return null

  const done = items.filter(item => item.achieved === true).length
  const total = items.length
  const progress = done || entry?.trophyProgress ? "detected" : "unknown"
  const percent = catalog === "confirmed" && progress === "detected" ? Math.floor(100 * done / total) : null
  const groups = new Map()
  const platinum = platinumForGame(appid, local, catalog === "known" ? undefined : groups)
  if (platinum) {
    if (catalog === "known") platinumForGame(appid, items, groups)
    // A consolidação pode clonar a entrada; não transfere grupo para um alias ambíguo.
    items = items.map(item => groups.has(item) ? { ...item, trophyGroup: groups.get(item) } : item)
  }
  return { items, catalog, progress, done, total, percent, stale: stale || entry?.trophyProgressStale === true, ...(platinum && { platinum }) }
}

module.exports = { getTrophySummary }
