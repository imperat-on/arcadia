// JavaScript · platinum.js · Electron main · verified Steam achievement sets
const { aliasesOf } = require("./match")

// IDs públicos: steamcommunity.com/stats/<appid>/achievements/?l=english.
// Cuphead: separação da lista base / The Delicious Last Course revisada em
// https://cuphead.wiki.gg/wiki/Achievements; o Gauntlet é o 14º troféu do DLC.
// Portal Soundtrack não tem troféus; Portal with RTX tem appid e lista próprios.
const trophySets = Object.freeze({
  400: { base: `PORTAL_GET_PORTALGUNS PORTAL_KILL_COMPANIONCUBE PORTAL_ESCAPE_TESTCHAMBERS PORTAL_BEAT_GAME PORTAL_HIT_TURRET_WITH_TURRET PORTAL_BEAT_2ADVANCEDMAPS PORTAL_LONGJUMP PORTAL_BEAT_4ADVANCEDMAPS PORTAL_BEAT_6ADVANCEDMAPS PORTAL_INFINITEFALL PORTAL_TRANSMISSION_RECEIVED PORTAL_DETACH_ALL_CAMERAS PORTAL_GET_ALLSILVER PORTAL_GET_ALLBRONZE PORTAL_GET_ALLGOLD`.split(" ") },
  221910: { base: `TSP_BEATTHEGAME TSP_WELCOMEBACK TSP_JUMPING TSP_ACHIEVEMENT TSP_430 TSP_888 TSP_GOOUTSIDE TSP_SPEED TSP_UNACHIEVABLE TSP_COMMITMENT`.split(" ") },
  268910: {
    base: `DefeatBoss ParryApprentice ParryMaster ExWin SuperWin ParryChain NoHitsTaken ARankWorld1 ARankWorld2 ARankWorld3 CompleteWorld1 CompleteWorld2 CompleteWorld3 UnlockedAllSupers FoundAllLevelMoney BoughtAllItems CompleteDicePalace ARankWorld4 GoodEnding SRank NewGamePlus FoundSecretPassage SmallPlaneOnlyWin FoundAllMoney PacifistRun NoHitsTakenDicePalace BadEnding CompleteDevil`.split(" "),
    dlc: `CompleteWorldDLC ARankWorldDLC DefeatBossAsChalice DefeatXBossesAsChalice ChaliceSuperWin DefeatBossDLCWeapon DefeatAllKOG DefeatSaltbaker SRankAnyDLC DefeatBossNoMinions HP9 DefeatDevilPhase2 Paladin DefeatKOGGauntlet`.split(" "),
  },
})

function achievementKey(value) { return String(value ?? "").trim().toLowerCase() }

function resolveSets(definition, items) {
  const exact = new Map()
  const aliases = new Map()
  const add = (map, key, item) => {
    if (!key) return
    const previous = map.get(key)
    map.set(key, !map.has(key) || previous === item ? item : null)
  }
  for (const item of items) {
    if (!item) continue
    const id = achievementKey(item.apiname)
    add(exact, id, item)
    for (const alias of [...aliasesOf(item), item.remoteApiname]) {
      const key = achievementKey(alias)
      if (key !== id) add(aliases, key, item)
    }
  }
  const keys = [...definition.base, ...(definition.dlc || [])].map(achievementKey)
  const resolved = keys.map(key => exact.has(key) ? exact.get(key) : aliases.get(key))
  const counts = new Map()
  for (const item of resolved) if (item) counts.set(item, (counts.get(item) || 0) + 1)
  return resolved.map(item => counts.get(item) === 1 ? item : null)
}

function evaluatePlatinum(definition, items = [], groups) {
  if (!definition?.base?.length) return null
  const resolved = resolveSets(definition, items)
  const base = resolved.slice(0, definition.base.length)
  const dlc = resolved.slice(base.length)
  if (groups) {
    for (const item of base) if (item) groups.set(item, "base")
    for (const item of dlc) if (item) groups.set(item, "dlc")
  }
  const isEarned = item => item?.achieved === true && item.revoked !== true
  const progress = list => ({ done: list.filter(isEarned).length, total: list.length, known: list.filter(Boolean).length })
  const awarded = base.every(isEarned)
  const dates = awarded ? base.map(item => item.unlockEstimated ? 0 : Number(item.unlock)) : []
  return {
    base: progress(base), dlc: progress(dlc), awarded,
    earnedAt: awarded && dates.every(ts => Number.isSafeInteger(ts) && ts > 0) ? Math.max(...dates) : null,
  }
}

function platinumForGame(appid, items, groups) {
  return Object.hasOwn(verifiedSets, appid) ? evaluatePlatinum(verifiedSets[appid], items, groups) : null
}

// Portal with RTX publishes the same IDs under a separate AppID.
const verifiedSets = Object.freeze({ ...trophySets, 2012840: trophySets[400] })
module.exports = { trophySets: verifiedSets, evaluatePlatinum, platinumForGame }
