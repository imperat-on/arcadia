"use strict"

const asyncHandler = require("./async-handler")
const { db } = require("./db")
const { SYSTEMS, activeVersion, syncRetroCatalog } = require("./retro-service")

const MAX_PAGE_SIZE = 100

// A small, editorial first-party shelf for the initial Retro Store experience.
// The complete source catalog remains available with mode=all. Keep these as
// canonical English LaunchBox titles so regional/duplicate variants collapse
// into the same game while the full catalog work continues.
const ESSENTIAL_TITLES = [
  "Castlevania: Symphony of the Night", "Metal Gear Solid", "Final Fantasy VII", "Final Fantasy IX", "Resident Evil 2", "Tekken 3",
  "Grand Theft Auto: San Andreas", "Grand Theft Auto: Vice City", "Shadow of the Colossus", "God of War II", "Metal Gear Solid 3: Subsistence", "Silent Hill 2", "Resident Evil 4", "Kingdom Hearts II", "Final Fantasy X",
  "Metal Gear Solid 4: Guns of the Patriots", "Uncharted 2: Among Thieves", "The Last of Us", "Red Dead Redemption", "Demon's Souls", "God of War III",
  "Super Mario Bros. 3", "The Legend of Zelda", "Mega Man 2", "Contra", "Metroid", "Castlevania III: Dracula's Curse",
  "Super Mario World", "The Legend of Zelda: A Link to the Past", "Super Metroid", "Chrono Trigger", "Final Fantasy VI", "Donkey Kong Country 2: Diddy's Kong Quest",
  "The Legend of Zelda: Ocarina of Time", "Super Mario 64", "Mario Kart 64", "GoldenEye 007", "Perfect Dark", "Banjo-Kazooie",
  "Pokémon Red/Blue", "The Legend of Zelda: Link's Awakening", "Super Mario Land 2: 6 Golden Coins", "Pokémon Gold/Silver", "Tetris",
  "Pokémon Emerald", "The Legend of Zelda: The Minish Cap", "Metroid Fusion", "Advance Wars", "Fire Emblem", "Mario Kart: Super Circuit",
  "The Legend of Zelda: The Wind Waker", "Metroid Prime", "Super Smash Bros. Melee", "Resident Evil", "Mario Kart: Double Dash!!", "Eternal Darkness: Sanity's Requiem",
  "Super Mario Galaxy", "The Legend of Zelda: Twilight Princess", "Wii Sports", "Mario Kart Wii", "Metroid Prime 3: Corruption", "New Super Mario Bros. Wii",
  "New Super Mario Bros.", "Mario Kart DS", "Pokémon Platinum", "The Legend of Zelda: Phantom Hourglass", "Animal Crossing: Wild World", "Castlevania: Dawn of Sorrow",
  "Crisis Core: Final Fantasy VII", "God of War: Chains of Olympus", "Monster Hunter Freedom Unite", "Persona 3 Portable", "Grand Theft Auto: Vice City Stories", "Metal Gear Solid: Peace Walker",
].map((title) => title.toLowerCase())

function integer(value, fallback, max = Number.MAX_SAFE_INTEGER) {
  const parsed = Number(value)
  return Number.isSafeInteger(parsed) && parsed >= 0 ? Math.min(parsed, max) : fallback
}

function gameFromRow(row) {
  const artwork = row.artwork && typeof row.artwork === "object" ? row.artwork : {}
  return {
    id: row.game_id,
    systemId: row.system_id,
    platform: row.system_id,
    title: row.title,
    titleLocale: row.title_locale,
    sortTitle: row.sort_title,
    aliases: row.aliases || [],
    artwork,
    cover: artwork.cover || undefined,
    hero: artwork.backgrounds?.[0] || undefined,
    screenshots: Array.isArray(artwork.screenshots) ? artwork.screenshots : [],
    titleScreens: Array.isArray(artwork.titleScreens) ? artwork.titleScreens : [],
    logo: artwork.logos?.[0] || undefined,
    description: artwork.description || undefined,
    releaseYear: artwork.releaseYear || undefined,
    developer: artwork.developer || [],
    publisher: artwork.publisher || [],
    genres: artwork.genres || [],
    series: artwork.series || [],
    playMode: artwork.playMode || [],
    maxPlayers: artwork.maxPlayers || undefined,
    offerCount: Number(row.offer_count) || 0,
    matchQuality: row.match_quality,
  }
}

function offerSummary(row) {
  const metadata = row.metadata && typeof row.metadata === "object" ? row.metadata : {}
  return {
    id: row.offer_id,
    sourceId: row.source_id,
    sourceTitle: row.source_title,
    originalTitle: row.original_title,
    normalizedTitle: row.normalized_title,
    systemId: row.system_id,
    ...metadata,
    hasUris: Array.isArray(row.uris) && row.uris.length > 0,
    uriCount: Array.isArray(row.uris) ? row.uris.length : 0,
  }
}

function etag(req, res, version, suffix = "") {
  const value = `"retro-${version || "empty"}${suffix}"`
  if (req.headers["if-none-match"] === value) {
    res.status(304).end()
    return true
  }
  res.set("ETag", value)
  res.set("Cache-Control", "private, max-age=60, stale-while-revalidate=86400")
  return false
}

function registerRetroRoutes(app) {
  app.get("/catalog/v1/retro/manifest", asyncHandler(async (req, res) => {
    const row = (await db.query(
      `SELECT v.version, v.generated_at, v.source_meta, v.stats
         FROM retro_catalog_state s LEFT JOIN retro_catalog_versions v ON v.version = s.active_version
        WHERE s.singleton`,
    )).rows[0]
    if (!row?.version) return res.status(503).json({ error: "retro_catalog_empty" })
    if (etag(req, res, row.version)) return
    res.json({ ok: true, version: row.version, generatedAt: row.generated_at, sourceMeta: row.source_meta, stats: row.stats })
  }))

  app.get("/catalog/v1/retro/systems", asyncHandler(async (req, res) => {
    const version = await activeVersion()
    if (etag(req, res, version, "-systems")) return
    const systems = [...new Map(Object.values(SYSTEMS).map(([id, collection]) => [id, { id, collection }])).values()]
    res.json({ ok: true, systems })
  }))

  // Coverage report used by the enrichment pipeline. It is intentionally
  // read-only and aggregates only public catalog metadata (never offer URIs).
  app.get("/catalog/v1/retro/audit", asyncHandler(async (req, res) => {
    const version = await activeVersion()
    if (!version) return res.status(503).json({ error: "retro_catalog_empty" })
    const requestedSystem = String(req.query.system || "").trim().slice(0, 80)
    const sampleLimit = Math.max(0, Math.min(100, integer(req.query.samples, 20, 100)))
    const auditSuffix = `-audit-${Buffer.from(`${requestedSystem}|${sampleLimit}`).toString("base64url")}`
    if (etag(req, res, version, auditSuffix)) return
    const filter = requestedSystem ? " AND system_id = $2" : ""
    const filterValues = requestedSystem ? [version, requestedSystem] : [version]
    const rows = (await db.query(
      `SELECT system_id,
              count(*)::int AS games,
              count(*) FILTER (WHERE nullif(artwork->>'cover', '') IS NOT NULL)::int AS covers,
              count(*) FILTER (WHERE nullif(artwork->>'description', '') IS NOT NULL)::int AS descriptions,
              count(*) FILTER (WHERE jsonb_typeof(artwork->'screenshots') = 'array' AND jsonb_array_length(artwork->'screenshots') > 0)::int AS screenshots,
              count(*) FILTER (WHERE jsonb_typeof(artwork->'backgrounds') = 'array' AND jsonb_array_length(artwork->'backgrounds') > 0)::int AS heroes,
              count(*) FILTER (WHERE nullif(artwork->>'logo', '') IS NOT NULL OR (jsonb_typeof(artwork->'logos') = 'array' AND jsonb_array_length(artwork->'logos') > 0))::int AS logos
         FROM retro_games
        WHERE version = $1 AND title_locale = 'en' AND offer_count > 0${filter}
        GROUP BY system_id ORDER BY system_id`,
      filterValues,
    )).rows
    const fields = ["covers", "descriptions", "screenshots", "heroes", "logos"]
    const totals = rows.reduce((acc, row) => {
      acc.games += row.games
      for (const field of fields) acc[field] += row[field]
      return acc
    }, { games: 0, covers: 0, descriptions: 0, screenshots: 0, heroes: 0, logos: 0 })
    let samples = []
    if (sampleLimit > 0) {
      const sampleValues = requestedSystem ? [version, requestedSystem, sampleLimit] : [version, sampleLimit]
      const systemClause = requestedSystem ? " AND system_id = $2" : ""
      const limitParam = requestedSystem ? "$3" : "$2"
      samples = (await db.query(
        `SELECT game_id AS id, title, system_id AS "systemId", offer_count,
                CASE WHEN nullif(artwork->>'cover', '') IS NULL THEN 'cover'
                     WHEN COALESCE(jsonb_array_length(COALESCE(artwork->'screenshots', '[]'::jsonb)), 0) = 0
                       AND nullif(artwork->>'description', '') IS NULL THEN 'details'
                     WHEN COALESCE(jsonb_array_length(COALESCE(artwork->'backgrounds', '[]'::jsonb)), 0) = 0 THEN 'hero'
                     ELSE 'complete' END AS "missingField"
           FROM retro_games
          WHERE version = $1 AND title_locale = 'en' AND offer_count > 0${systemClause}
            AND (nullif(artwork->>'cover', '') IS NULL
              OR COALESCE(jsonb_array_length(COALESCE(artwork->'screenshots', '[]'::jsonb)), 0) = 0
                 AND nullif(artwork->>'description', '') IS NULL
              OR COALESCE(jsonb_array_length(COALESCE(artwork->'backgrounds', '[]'::jsonb)), 0) = 0)
          ORDER BY CASE WHEN nullif(artwork->>'cover', '') IS NULL THEN 0 ELSE 1 END, sort_title, game_id
          LIMIT ${limitParam}`,
        sampleValues,
      )).rows
    }
    const versionStats = (await db.query("SELECT stats FROM retro_catalog_versions WHERE version = $1", [version])).rows[0]?.stats || {}
    res.json({ ok: true, version, totals, systems: rows, missing: Object.fromEntries(fields.map((field) => [field, totals.games - totals[field]])), unmatched: { total: Number(versionStats.unmatched) || 0, byReason: versionStats.unmatchedByReason || {}, bySystem: versionStats.unmatchedBySystem || {}, samples: versionStats.unmatchedSamples || [] }, samples })
  }))

  app.get("/catalog/v1/retro/games", asyncHandler(async (req, res) => {
    const version = await activeVersion()
    if (!version) return res.status(503).json({ error: "retro_catalog_empty" })
    const offset = integer(req.query.offset, 0)
    const limit = Math.max(1, integer(req.query.limit, 24, MAX_PAGE_SIZE))
    const query = String(req.query.query || "").trim().slice(0, 120)
    const system = String(req.query.system || "").trim().slice(0, 80)
    const variants = String(req.query.variants || "") === "all"
    const mode = String(req.query.mode || "") === "all" ? "all" : "essentials"
    const values = [version]
    const where = ["version = $1", "title_locale = 'en'", "offer_count > 0"]
    if (system) { values.push(system); where.push(`system_id = $${values.length}`) }
    if (query) { values.push(query); where.push(`to_tsvector('simple', search_text) @@ plainto_tsquery('simple', $${values.length})`) }
    if (!variants) where.push(`EXISTS (SELECT 1 FROM retro_offers o WHERE o.version = retro_games.version AND o.game_id = retro_games.game_id AND o.original_title !~* '(\\bbeta\\b|\\bdemo\\b|\\bproto(?:type)?\\b|\\bhomebrew\\b|\\baftermarket\\b|\\b(unl|hack|translation|patch|trainer|bios|firmware)\\b|\\bgame ?jam\\b)')`)
    if (mode === "essentials") { values.push(ESSENTIAL_TITLES); where.push(`lower(title) = ANY($${values.length}::text[])`) }
    const condition = where.join(" AND ")
    const total = Number((await db.query(`SELECT count(*) AS count FROM retro_games WHERE ${condition}`, values)).rows[0]?.count || 0)
    const filteredOffers = Number((await db.query(`SELECT COALESCE(SUM(offer_count), 0) AS count FROM retro_games WHERE ${condition}`, values)).rows[0]?.count || 0)
    values.push(limit, offset)
    const rows = (await db.query(
      `SELECT * FROM retro_games WHERE ${condition}
        ORDER BY
          CASE system_id
            WHEN 'sony-playstation-2' THEN 0
            WHEN 'sony-playstation-3' THEN 1
            ELSE 2
          END,
          CASE WHEN COALESCE(NULLIF(artwork->>'cover', ''), '') <> '' THEN 0 ELSE 1 END,
          CASE WHEN jsonb_array_length(COALESCE(artwork->'screenshots', '[]'::jsonb)) > 0
                 OR COALESCE(NULLIF(artwork->>'description', ''), '') <> '' THEN 0 ELSE 1 END,
          sort_title, system_id, game_id LIMIT $${values.length - 1} OFFSET $${values.length}`,
      values,
    )).rows
    const stats = (await db.query("SELECT stats FROM retro_catalog_versions WHERE version = $1", [version])).rows[0]?.stats || {}
    const suffix = `-${Buffer.from(`${query}|${system}|${mode}|${variants ? "all" : "retail"}|${offset}|${limit}`).toString("base64url")}`
    if (etag(req, res, version, suffix)) return
    res.json({ ok: true, games: rows.map(gameFromRow), totalGames: total, totalOffers: filteredOffers, unmatchedOffers: Number(stats.unmatched) || 0, total, offset, limit, hasMore: offset + rows.length < total, mode, variants: variants ? "all" : "retail", updatedAt: version })
  }))

  app.get("/catalog/v1/retro/games/:gameId", asyncHandler(async (req, res) => {
    const version = await activeVersion()
    const gameId = String(req.params.gameId || "").slice(0, 240)
    const row = (await db.query("SELECT * FROM retro_games WHERE version = $1 AND game_id = $2 AND title_locale = 'en'", [version, gameId])).rows[0]
    if (!row) return res.status(404).json({ error: "retro_game_not_found" })
    const offers = (await db.query("SELECT * FROM retro_offers WHERE version = $1 AND game_id = $2 ORDER BY source_title, original_title", [version, gameId])).rows
    if (etag(req, res, version, `-${gameId}`)) return
    res.json({ ok: true, game: gameFromRow(row), offers: offers.map(offerSummary) })
  }))

  app.get("/catalog/v1/retro/games/:gameId/offers", asyncHandler(async (req, res) => {
    const version = await activeVersion()
    const rows = (await db.query("SELECT * FROM retro_offers WHERE version = $1 AND game_id = $2 ORDER BY source_title, original_title", [version, String(req.params.gameId || "").slice(0, 240)])).rows
    res.json({ ok: true, offers: rows.map(offerSummary) })
  }))

  app.get("/catalog/v1/retro/offers/:offerId", asyncHandler(async (req, res) => {
    const version = await activeVersion()
    const row = (await db.query("SELECT * FROM retro_offers WHERE version = $1 AND offer_id = $2", [version, String(req.params.offerId || "").slice(0, 80)])).rows[0]
    if (!row) return res.status(404).json({ error: "retro_offer_not_found" })
    res.set("Cache-Control", "no-store")
    res.json({ ok: true, offer: { ...offerSummary(row), uris: Array.isArray(row.uris) ? row.uris : [] } })
  }))
}

let timer
function startRetroSync() {
  if (process.env.NODE_ENV === "test" || process.env.RETRO_SYNC_ENABLED === "0" || timer) return
  const run = () => syncRetroCatalog().catch((error) => console.error("[retro-sync]", error.message))
  setTimeout(run, 5_000).unref()
  timer = setInterval(run, 24 * 60 * 60 * 1000)
  timer.unref()
}

module.exports = { registerRetroRoutes, startRetroSync, gameFromRow, offerSummary }
