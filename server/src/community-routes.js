"use strict"

// Community API: reviews/avaliacoes and public collections/listas.  The
// handlers intentionally keep authorization beside each SQL predicate: this
// backend does not have PostgreSQL RLS, so a new query must not accidentally
// turn a private collection into a public one.

const crypto = require("node:crypto")
const { db, withTransaction } = require("./db")
const { verifyToken, extractToken } = require("./jwt")
const asyncHandler = require("./async-handler")
const { sendError } = require("./api-observability")
const {
  REVIEW_STATUSES,
  COLLECTION_STATUSES,
  VISIBILITIES,
  normalizeReviewInput,
  normalizeCollectionInput,
  normalizeCollectionItems,
  normalizeReportInput,
  normalizeAppid,
  parsePagination,
  validationResponse,
} = require("./community-validation")

const REVIEW_LIST_PATHS = ["/community/v1/reviews", "/community/v1/avaliacoes"]
const REVIEW_ITEM_PATHS = ["/community/v1/reviews/:appid", "/community/v1/avaliacoes/:appid"]
const REVIEW_DETAIL_PATHS = ["/community/v1/review/:id", "/community/v1/avaliacao/:id"]
// Mutation aliases also accept the plural form used by REST clients. GET on
// /reviews/:appid remains the game review listing and therefore stays separate.
const REVIEW_MUTATION_PATHS = [
  ...REVIEW_DETAIL_PATHS,
  "/community/v1/reviews/:id",
  "/community/v1/avaliacoes/:id",
]
const COLLECTION_LIST_PATHS = [
  "/community/v1/collections",
  "/community/v1/lists",
  "/community/v1/listas",
]
const COLLECTION_DETAIL_PATHS = [
  "/community/v1/collections/:id",
  "/community/v1/lists/:id",
  "/community/v1/listas/:id",
]

function currentUser(req) {
  const token = extractToken(req)
  const verified = verifyToken(token || "")
  return verified.ok && typeof verified.sub === "string" ? verified.sub : null
}

function fail(req, res, status, code, details, message) {
  return sendError(req, res, status, code, {
    details,
    message,
  })
}

function requireUser(req, res) {
  const uid = currentUser(req)
  if (!uid) {
    fail(req, res, 401, "nao_autenticado")
    return null
  }
  return uid
}

function validationFail(req, res, error) {
  const result = validationResponse(error)
  return fail(req, res, result.status, result.code, result.details)
}

function idValue(value) {
  const id = String(value || "").trim()
  return id && id.length <= 128 ? id : null
}

function publicReview(row, includeModeration = false) {
  if (!row) return null
  const result = {
    id: Number(row.id),
    appid: String(row.appid),
    title: row.title || null,
    text: row.text,
    rating: Number(row.rating),
    positive: Number(row.positive) === 1,
    hours: Number(row.hours || 0),
    created_at: row.created_at,
    updated_at: row.updated_at,
    user_id: row.user_id,
    username: row.username,
    display_name: row.display_name || null,
    avatar_url: row.avatar_url || null,
    author: {
      id: row.user_id,
      username: row.username,
      display_name: row.display_name || null,
      avatar_url: row.avatar_url || null,
    },
  }
  if (includeModeration) {
    result.status = row.status
    result.moderation_reason = row.moderation_reason || null
    result.reported_at = row.reported_at || null
  }
  return result
}

function publicCollection(row, includeModeration = false) {
  if (!row) return null
  const result = {
    id: row.id,
    title: row.title,
    description: row.description || null,
    visibility: row.visibility,
    created_at: row.created_at,
    updated_at: row.updated_at,
    user_id: row.user_id,
    username: row.username,
    display_name: row.display_name || null,
    avatar_url: row.avatar_url || null,
    owner: {
      id: row.user_id,
      username: row.username,
      display_name: row.display_name || null,
      avatar_url: row.avatar_url || null,
    },
    item_count: Number(row.item_count || 0),
  }
  if (includeModeration) {
    result.status = row.status
    result.moderation_reason = row.moderation_reason || null
  }
  if (row.items) result.items = row.items
  return result
}

function paginationBody(rows, limit, offset, key) {
  const hasMore = rows.length > limit
  const items = hasMore ? rows.slice(0, limit) : rows
  return {
    ok: true,
    [key]: items,
    items,
    pagination: { limit, offset, has_more: hasMore },
  }
}

function moderatorIdsFromEnv() {
  return new Set(
    String(process.env.COMMUNITY_MODERATOR_IDS || "")
      .split(",")
      .map((id) => id.trim())
      .filter(Boolean),
  )
}

function moderatorNamesFromEnv() {
  return new Set(
    String(process.env.COMMUNITY_MODERATOR_USERNAMES || "")
      .split(",")
      .map((name) => name.trim().toLowerCase())
      .filter(Boolean),
  )
}

async function isModerator(uid) {
  if (!uid) return false
  if (moderatorIdsFromEnv().has(uid)) return true
  try {
    const row = (await db.query("SELECT 1 FROM community_moderators WHERE user_id = $1", [uid])).rows[0]
    if (row) return true
  } catch (error) {
    // During a rolling deploy the route can be loaded before migration 0003.
    // Treat a missing moderator table as no privileges, never as admin.
    if (error.code !== "42P01") throw error
  }
  const profile = (await db.query("SELECT username FROM profiles WHERE id = $1", [uid])).rows[0]
  return Boolean(profile && moderatorNamesFromEnv().has(String(profile.username).toLowerCase()))
}

function blockPredicate(viewer, column, params) {
  if (!viewer) return "TRUE"
  params.push(viewer)
  const viewerParam = `$${params.length}`
  params.push(viewer)
  const viewerParam2 = `$${params.length}`
  return `( ${column} = ${viewerParam} OR NOT EXISTS (
    SELECT 1 FROM blocks community_block
     WHERE (community_block.blocker_id = ${viewerParam2} AND community_block.blocked_id = ${column})
        OR (community_block.blocker_id = ${column} AND community_block.blocked_id = ${viewerParam2})
  ))`
}

function reviewSelect() {
  return `SELECT r.id, r.user_id, r.appid, r.title, r.text, r.rating, r.positive,
                 r.hours, r.status, r.moderation_reason, r.reported_at,
                 r.created_at, r.updated_at,
                 p.username, p.display_name, p.avatar_url
            FROM user_reviews r
            JOIN profiles p ON p.id = r.user_id`
}

async function findReview(id, viewer, { includeOwner = true, moderator = false } = {}) {
  const params = [id]
  const access = []
  if (moderator) access.push("TRUE")
  else if (includeOwner && viewer) {
    params.push(viewer)
    access.push(`(r.status = 'visible' OR r.user_id = $${params.length})`)
  } else access.push("r.status = 'visible'")
  const blocks = blockPredicate(viewer, "r.user_id", params)
  const row = (await db.query(
    `${reviewSelect()} WHERE r.id = $1 AND ${access.join(" AND ")} AND ${blocks}`,
    params,
  )).rows[0]
  return row || null
}

async function listReviews(req, res, routeAppid) {
  const viewer = currentUser(req)
  const paging = parsePagination(req.query)
  if (paging.error) return validationFail(req, res, paging.error)
  const { limit, offset } = paging.value
  const params = []
  const clauses = ["r.status = 'visible'"]
  const appidValue = routeAppid ?? req.query.appid ?? req.query.app_id
  if (appidValue !== undefined) {
    const appid = normalizeAppid(appidValue)
    if (appid.error) return validationFail(req, res, appid.error)
    params.push(appid.value)
    clauses.push(`r.appid = $${params.length}`)
  }
  clauses.push(blockPredicate(viewer, "r.user_id", params))
  params.push(limit + 1)
  const limitParam = `$${params.length}`
  params.push(offset)
  const offsetParam = `$${params.length}`
  const rows = (await db.query(
    `${reviewSelect()} WHERE ${clauses.join(" AND ")}
      ORDER BY r.created_at DESC, r.id DESC LIMIT ${limitParam} OFFSET ${offsetParam}`,
    params,
  )).rows
  const body = paginationBody(rows.map((row) => publicReview(row)), limit, offset, "reviews")
  body.ok = true
  return res.json(body)
}

async function createReview(req, res, routeAppid) {
  const uid = requireUser(req, res)
  if (!uid) return
  const normalized = normalizeReviewInput(req.body, routeAppid)
  if (normalized.error) return validationFail(req, res, normalized.error)
  const value = normalized.value
  try {
    const inserted = (await db.query(
      `INSERT INTO user_reviews (user_id, appid, title, text, rating, positive, hours, status, updated_at)
       VALUES ($1, $2, $3, $4, $5, $6, $7, 'visible', now())
       RETURNING id`,
      [uid, value.appid, value.title, value.text, value.rating, value.positive, value.hours],
    )).rows[0]
    const review = await findReview(inserted.id, uid)
    return res.status(201).json({ ok: true, review: publicReview(review), data: publicReview(review) })
  } catch (error) {
    if (error.code === "23505") return fail(req, res, 409, "review_existente")
    if (error.code === "23503") return fail(req, res, 404, "usuario_nao_existe")
    throw error
  }
}

async function patchReview(req, res) {
  const uid = requireUser(req, res)
  if (!uid) return
  const id = idValue(req.params.id)
  if (!id || !/^\d+$/.test(id)) return fail(req, res, 400, "review_id_invalido")
  const moderator = await isModerator(uid)
  const existing = await findReview(id, uid, { includeOwner: true, moderator })
  if (!existing) return fail(req, res, 404, "review_nao_encontrada")
  if (existing.user_id !== uid && !moderator) return fail(req, res, 403, "permissao_negada")

  const body = req.body || {}
  const reviewPayload = {
    ...body,
    appid: existing.appid,
    text: body.text ?? existing.text,
    title: Object.prototype.hasOwnProperty.call(body, "title") ? body.title : existing.title,
    rating: body.rating ?? existing.rating,
    positive: Object.prototype.hasOwnProperty.call(body, "positive") ? body.positive : Number(existing.positive) === 1,
    hours: body.hours ?? existing.hours,
  }
  const normalized = normalizeReviewInput(reviewPayload, existing.appid)
  if (normalized.error) return validationFail(req, res, normalized.error)
  const value = normalized.value
  let status = existing.status
  let reason = existing.moderation_reason
  if (Object.prototype.hasOwnProperty.call(body, "status")) {
    if (!moderator || typeof body.status !== "string" || !REVIEW_STATUSES.has(body.status)) {
      return fail(req, res, 403, "moderacao_negada")
    }
    status = body.status
    reason = typeof body.moderation_reason === "string" ? body.moderation_reason.trim().slice(0, 1000) || null : null
  }
  const row = (await db.query(
    `UPDATE user_reviews
        SET title = $1, text = $2, rating = $3, positive = $4, hours = $5,
            status = $6, moderation_reason = $7,
            moderated_by = CASE WHEN $8::boolean THEN $9 ELSE moderated_by END,
            updated_at = now()
      WHERE id = $10
      RETURNING id`,
    [value.title, value.text, value.rating, value.positive, value.hours, status, reason, moderator, moderator ? uid : null, id],
  )).rows[0]
  if (!row) return fail(req, res, 404, "review_nao_encontrada")
  const review = await findReview(id, uid, { includeOwner: true, moderator })
  return res.json({ ok: true, review: publicReview(review, moderator), data: publicReview(review, moderator) })
}

async function removeReview(req, res) {
  const uid = requireUser(req, res)
  if (!uid) return
  const id = idValue(req.params.id)
  if (!id || !/^\d+$/.test(id)) return fail(req, res, 400, "review_id_invalido")
  const moderator = await isModerator(uid)
  const row = (await db.query("SELECT user_id FROM user_reviews WHERE id = $1", [id])).rows[0]
  if (!row) return fail(req, res, 404, "review_nao_encontrada")
  if (row.user_id !== uid && !moderator) return fail(req, res, 403, "permissao_negada")
  await db.query("DELETE FROM user_reviews WHERE id = $1", [id])
  return res.status(204).end()
}

async function reportTarget(req, res, targetType) {
  const uid = requireUser(req, res)
  if (!uid) return
  const targetId = idValue(req.params.id)
  if (!targetId) return fail(req, res, 400, "target_id_invalido")
  const normalized = normalizeReportInput(req.body)
  if (normalized.error) return validationFail(req, res, normalized.error)
  const existsQuery = targetType === "review"
    ? "SELECT 1 FROM user_reviews WHERE id = $1"
    : "SELECT 1 FROM collections WHERE id = $1"
  if (!(await db.query(existsQuery, [targetId])).rows[0]) {
    return fail(req, res, 404, targetType === "review" ? "review_nao_encontrada" : "colecao_nao_encontrada")
  }
  try {
    const row = (await db.query(
      `INSERT INTO community_reports (reporter_id, target_type, target_id, reason, details)
       VALUES ($1, $2, $3, $4, $5) RETURNING id, created_at`,
      [uid, targetType, targetId, normalized.value.reason, normalized.value.details],
    )).rows[0]
    return res.status(201).json({ ok: true, report: { id: Number(row.id), status: "open", created_at: row.created_at } })
  } catch (error) {
    if (error.code === "23505") return fail(req, res, 409, "denuncia_existente")
    throw error
  }
}

function collectionSelect() {
  return `SELECT c.id, c.user_id, c.title, c.description, c.visibility, c.status,
                 c.moderation_reason, c.created_at, c.updated_at,
                 p.username, p.display_name, p.avatar_url,
                 (SELECT count(*)::integer FROM collection_items ci WHERE ci.collection_id = c.id) AS item_count
            FROM collections c
            JOIN profiles p ON p.id = c.user_id`
}

async function listCollections(req, res) {
  const viewer = currentUser(req)
  const paging = parsePagination(req.query)
  if (paging.error) return validationFail(req, res, paging.error)
  const { limit, offset } = paging.value
  const mine = String(req.query.mine || "").toLowerCase() === "true" || String(req.query.mine || "") === "1"
  if (mine && !viewer) return fail(req, res, 401, "nao_autenticado")
  const moderator = viewer ? await isModerator(viewer) : false
  const params = []
  const access = []
  if (moderator) access.push("TRUE")
  else if (viewer) {
    params.push(viewer)
    const viewerParam = `$${params.length}`
    access.push(mine
      ? `c.user_id = ${viewerParam}`
      : `(c.user_id = ${viewerParam} OR (c.visibility = 'public' AND c.status = 'visible'))`)
  } else access.push("c.visibility = 'public' AND c.status = 'visible'")
  if (req.query.owner !== undefined || req.query.user_id !== undefined) {
    const owner = String(req.query.owner ?? req.query.user_id).trim()
    if (!owner || owner.length > 128) return fail(req, res, 400, "owner_invalido")
    params.push(owner)
    const ownerParam = `$${params.length}`
    access.push(`(c.user_id = ${ownerParam} OR p.username = ${ownerParam})`)
  }
  if (req.query.visibility !== undefined) {
    const visibility = String(req.query.visibility).trim().toLowerCase()
    if (!VISIBILITIES.has(visibility)) return fail(req, res, 400, "visibilidade_invalida")
    params.push(visibility)
    access.push(`c.visibility = $${params.length}`)
  }
  access.push(blockPredicate(viewer, "c.user_id", params))
  params.push(limit + 1)
  const limitParam = `$${params.length}`
  params.push(offset)
  const offsetParam = `$${params.length}`
  const rows = (await db.query(
    `${collectionSelect()} WHERE ${access.join(" AND ")}
      ORDER BY c.created_at DESC, c.id DESC LIMIT ${limitParam} OFFSET ${offsetParam}`,
    params,
  )).rows
  const includeModeration = Boolean(moderator || mine)
  const body = paginationBody(rows.map((row) => publicCollection(row, includeModeration)), limit, offset, "collections")
  return res.json(body)
}

async function loadCollection(id, viewer, moderator = false) {
  const params = [id]
  const access = []
  if (moderator) access.push("TRUE")
  else if (viewer) {
    params.push(viewer)
    const viewerParam = `$${params.length}`
    access.push(`(c.user_id = ${viewerParam} OR (c.status = 'visible' AND c.visibility IN ('public', 'unlisted')))`)
  } else access.push("c.status = 'visible' AND c.visibility IN ('public', 'unlisted')")
  access.push(blockPredicate(viewer, "c.user_id", params))
  const row = (await db.query(`${collectionSelect()} WHERE c.id = $1 AND ${access.join(" AND ")}`, params)).rows[0]
  if (!row) return null
  row.items = (await db.query(
    `SELECT appid, position, title, note, created_at
       FROM collection_items WHERE collection_id = $1 ORDER BY position ASC`,
    [id],
  )).rows.map((item) => ({
    appid: String(item.appid),
    position: Number(item.position),
    title: item.title || null,
    note: item.note || null,
    created_at: item.created_at,
  }))
  return row
}

async function getCollection(req, res) {
  const id = idValue(req.params.id)
  if (!id) return fail(req, res, 400, "colecao_id_invalido")
  const viewer = currentUser(req)
  const moderator = viewer ? await isModerator(viewer) : false
  const row = await loadCollection(id, viewer, moderator)
  if (!row) return fail(req, res, 404, "colecao_nao_encontrada")
  return res.json({ ok: true, collection: publicCollection(row, moderator || row.user_id === viewer), data: publicCollection(row, moderator || row.user_id === viewer) })
}

async function insertCollectionItems(client, collectionId, items) {
  for (const item of items) {
    await client.query(
      `INSERT INTO collection_items (collection_id, appid, position, title, note)
       VALUES ($1, $2, $3, $4, $5)`,
      [collectionId, item.appid, item.position, item.title, item.note],
    )
  }
}

async function createCollection(req, res) {
  const uid = requireUser(req, res)
  if (!uid) return
  const normalized = normalizeCollectionInput(req.body)
  if (normalized.error) return validationFail(req, res, normalized.error)
  const value = normalized.value
  const id = crypto.randomUUID()
  try {
    await withTransaction(async (client) => {
      await client.query(
        `INSERT INTO collections (id, user_id, title, description, visibility, status)
         VALUES ($1, $2, $3, $4, $5, 'visible')`,
        [id, uid, value.title, value.description, value.visibility],
      )
      await insertCollectionItems(client, id, value.items || [])
    })
  } catch (error) {
    if (error.code === "23503") return fail(req, res, 404, "usuario_nao_existe")
    if (error.code === "23505") return fail(req, res, 409, "item_duplicado")
    if (error.code === "23514") return fail(req, res, 400, "item_invalido")
    throw error
  }
  const row = await loadCollection(id, uid)
  return res.status(201).json({ ok: true, collection: publicCollection(row, true), data: publicCollection(row, true) })
}

async function updateCollection(req, res) {
  const uid = requireUser(req, res)
  if (!uid) return
  const id = idValue(req.params.id)
  if (!id) return fail(req, res, 400, "colecao_id_invalido")
  const moderator = await isModerator(uid)
  const existing = await loadCollection(id, uid, moderator)
  if (!existing) return fail(req, res, 404, "colecao_nao_encontrada")
  if (existing.user_id !== uid && !moderator) return fail(req, res, 403, "permissao_negada")
  const normalized = normalizeCollectionInput(req.body, { partial: true })
  if (normalized.error) return validationFail(req, res, normalized.error)
  const value = normalized.value
  let status = existing.status
  let reason = existing.moderation_reason
  if (Object.prototype.hasOwnProperty.call(req.body || {}, "status")) {
    if (!moderator || !COLLECTION_STATUSES.has(req.body.status)) return fail(req, res, 403, "moderacao_negada")
    status = req.body.status
    reason = typeof req.body.moderation_reason === "string" ? req.body.moderation_reason.trim().slice(0, 1000) || null : null
  }
  const merged = {
    title: value.title ?? existing.title,
    description: Object.prototype.hasOwnProperty.call(value, "description") ? value.description : existing.description,
    visibility: value.visibility ?? existing.visibility,
  }
  try {
    await withTransaction(async (client) => {
      await client.query(
        `UPDATE collections
            SET title = $1, description = $2, visibility = $3, status = $4,
                moderation_reason = $5,
                moderated_by = CASE WHEN $6::boolean THEN $7 ELSE moderated_by END,
                updated_at = now()
          WHERE id = $8`,
        [merged.title, merged.description, merged.visibility, status, reason, moderator, moderator ? uid : null, id],
      )
      if (Object.prototype.hasOwnProperty.call(value, "items")) {
        await client.query("DELETE FROM collection_items WHERE collection_id = $1", [id])
        await insertCollectionItems(client, id, value.items)
      }
    })
  } catch (error) {
    if (error.code === "23505") return fail(req, res, 409, "item_duplicado")
    if (error.code === "23514") return fail(req, res, 400, "item_invalido")
    throw error
  }
  const row = await loadCollection(id, uid, moderator)
  return res.json({ ok: true, collection: publicCollection(row, moderator || row.user_id === uid), data: publicCollection(row, moderator || row.user_id === uid) })
}

async function deleteCollection(req, res) {
  const uid = requireUser(req, res)
  if (!uid) return
  const id = idValue(req.params.id)
  if (!id) return fail(req, res, 400, "colecao_id_invalido")
  const moderator = await isModerator(uid)
  const row = (await db.query("SELECT user_id FROM collections WHERE id = $1", [id])).rows[0]
  if (!row) return fail(req, res, 404, "colecao_nao_encontrada")
  if (row.user_id !== uid && !moderator) return fail(req, res, 403, "permissao_negada")
  await db.query("DELETE FROM collections WHERE id = $1", [id])
  return res.status(204).end()
}

async function mutateCollectionItems(req, res, mode) {
  const uid = requireUser(req, res)
  if (!uid) return
  const id = idValue(req.params.id)
  if (!id) return fail(req, res, 400, "colecao_id_invalido")
  const moderator = await isModerator(uid)
  const collection = (await db.query("SELECT user_id FROM collections WHERE id = $1", [id])).rows[0]
  if (!collection) return fail(req, res, 404, "colecao_nao_encontrada")
  if (collection.user_id !== uid && !moderator) return fail(req, res, 403, "permissao_negada")
  const existing = (await db.query(
    "SELECT appid, position, title, note FROM collection_items WHERE collection_id = $1 ORDER BY position",
    [id],
  )).rows.map((item) => ({ appid: String(item.appid), title: item.title, note: item.note, position: Number(item.position) }))

  if (mode === "replace") {
    const input = Array.isArray(req.body) ? req.body : req.body?.items
    const normalized = normalizeCollectionItems(input)
    if (normalized.error) return validationFail(req, res, normalized.error)
    const next = normalized.value
    await replaceItems(id, next)
    return res.json({ ok: true, items: next })
  }

  if (mode === "delete") {
    const appid = normalizeAppid(req.params.appid)
    if (appid.error) return validationFail(req, res, appid.error)
    const next = existing.filter((item) => item.appid !== appid.value).map((item, index) => ({ ...item, position: index }))
    if (next.length === existing.length) return fail(req, res, 404, "item_nao_encontrado")
    await replaceItems(id, next)
    return res.status(204).end()
  }

  const payload = req.body?.item && typeof req.body.item === "object" ? req.body.item : req.body
  const normalized = normalizeCollectionItems([payload])
  if (normalized.error) return validationFail(req, res, normalized.error)
  const item = normalized.value[0]
  if (existing.some((entry) => entry.appid === item.appid)) return fail(req, res, 409, "item_duplicado")
  const requestedPosition = payload?.position !== undefined && payload?.position !== null && /^\d+$/.test(String(payload.position).trim())
    ? Number(payload.position)
    : existing.length
  if (!Number.isSafeInteger(requestedPosition) || requestedPosition < 0 || requestedPosition > existing.length) return fail(req, res, 400, "posicao_invalida")
  const next = existing.slice()
  next.splice(requestedPosition, 0, item)
  const compact = next.map((entry, index) => ({ ...entry, position: index }))
  await replaceItems(id, compact)
  return res.status(201).json({ ok: true, item: compact[requestedPosition], items: compact })
}

async function replaceItems(id, items) {
  await withTransaction(async (client) => {
    await client.query("DELETE FROM collection_items WHERE collection_id = $1", [id])
    await insertCollectionItems(client, id, items)
    await client.query("UPDATE collections SET updated_at = now() WHERE id = $1", [id])
  })
}

async function listModeration(req, res, kind) {
  const uid = requireUser(req, res)
  if (!uid) return
  if (!(await isModerator(uid))) return fail(req, res, 403, "moderacao_negada")
  const paging = parsePagination(req.query)
  if (paging.error) return validationFail(req, res, paging.error)
  const { limit, offset } = paging.value
  const params = [limit + 1, offset]
  if (kind === "reviews") {
    const rows = (await db.query(
      `${reviewSelect()}
       WHERE r.status <> 'visible' OR EXISTS (
         SELECT 1 FROM community_reports cr
          WHERE cr.target_type = 'review' AND cr.target_id = r.id::text AND cr.status = 'open'
       )
       ORDER BY COALESCE(r.reported_at, r.created_at) ASC, r.id ASC LIMIT $1 OFFSET $2`,
      params,
    )).rows
    return res.json(paginationBody(rows.map((row) => publicReview(row, true)), limit, offset, "reviews"))
  }
  const rows = (await db.query(
    `${collectionSelect()}
     WHERE c.status <> 'visible' OR EXISTS (
       SELECT 1 FROM community_reports cr
        WHERE cr.target_type = 'collection' AND cr.target_id = c.id AND cr.status = 'open'
     )
     ORDER BY c.created_at ASC, c.id ASC LIMIT $1 OFFSET $2`,
    params,
  )).rows
  return res.json(paginationBody(rows.map((row) => publicCollection(row, true)), limit, offset, "collections"))
}

async function moderate(req, res, kind) {
  const uid = requireUser(req, res)
  if (!uid) return
  if (!(await isModerator(uid))) return fail(req, res, 403, "moderacao_negada")
  const id = idValue(req.params.id)
  const status = req.body?.status
  const allowed = kind === "review" ? REVIEW_STATUSES : COLLECTION_STATUSES
  if (!id || !allowed.has(status)) return fail(req, res, 400, "status_moderacao_invalido")
  const reason = typeof req.body?.moderation_reason === "string" ? req.body.moderation_reason.trim().slice(0, 1000) || null : null
  const table = kind === "review" ? "user_reviews" : "collections"
  const row = (await db.query(
    `UPDATE ${table}
        SET status = $1, moderation_reason = $2, moderated_by = $3,
            updated_at = now()
      WHERE id = $4
      RETURNING id`,
    [status, reason, uid, id],
  )).rows[0]
  if (!row) return fail(req, res, 404, kind === "review" ? "review_nao_encontrada" : "colecao_nao_encontrada")
  await db.query(
    `UPDATE community_reports SET status = 'actioned', moderator_id = $1, moderator_note = $2, updated_at = now()
      WHERE target_type = $3 AND target_id = $4 AND status = 'open'`,
    [uid, reason, kind, id],
  )
  return res.json({ ok: true, id, status })
}

function registerCommunityRoutes(app) {
  // Reviews: both English and Portuguese aliases are kept here so the server
  // can evolve independently of a renderer release.
  app.get(REVIEW_LIST_PATHS, asyncHandler((req, res) => listReviews(req, res)))
  app.get(REVIEW_ITEM_PATHS, asyncHandler((req, res) => listReviews(req, res, req.params.appid)))
  app.get(REVIEW_DETAIL_PATHS, asyncHandler(async (req, res) => {
    const id = idValue(req.params.id)
    if (!id || !/^\d+$/.test(id)) return fail(req, res, 400, "review_id_invalido")
    const viewer = currentUser(req)
    const moderator = viewer ? await isModerator(viewer) : false
    const row = await findReview(id, viewer, { includeOwner: true, moderator })
    if (!row) return fail(req, res, 404, "review_nao_encontrada")
    const review = publicReview(row, moderator || row.user_id === viewer)
    return res.json({ ok: true, review, data: review })
  }))
  app.post(REVIEW_LIST_PATHS, asyncHandler((req, res) => createReview(req, res)))
  app.post(REVIEW_ITEM_PATHS, asyncHandler((req, res) => createReview(req, res, req.params.appid)))
  app.patch(REVIEW_MUTATION_PATHS, asyncHandler(patchReview))
  app.delete(REVIEW_MUTATION_PATHS, asyncHandler(removeReview))
  app.post(REVIEW_MUTATION_PATHS.map((path) => `${path}/report`), asyncHandler((req, res) => reportTarget(req, res, "review")))

  // Collections/lists.
  app.get(COLLECTION_LIST_PATHS, asyncHandler(listCollections))
  app.post(COLLECTION_LIST_PATHS, asyncHandler(createCollection))
  app.get(COLLECTION_DETAIL_PATHS, asyncHandler(getCollection))
  app.patch(COLLECTION_DETAIL_PATHS, asyncHandler(updateCollection))
  app.delete(COLLECTION_DETAIL_PATHS, asyncHandler(deleteCollection))
  app.post(COLLECTION_DETAIL_PATHS.map((path) => `${path}/items`), asyncHandler((req, res) => mutateCollectionItems(req, res, "add")))
  app.put(COLLECTION_DETAIL_PATHS.map((path) => `${path}/items`), asyncHandler((req, res) => mutateCollectionItems(req, res, "replace")))
  app.delete(COLLECTION_DETAIL_PATHS.map((path) => `${path}/items/:appid`), asyncHandler((req, res) => mutateCollectionItems(req, res, "delete")))
  app.post(COLLECTION_DETAIL_PATHS.map((path) => `${path}/report`), asyncHandler((req, res) => reportTarget(req, res, "collection")))

  // Moderation queue. It is intentionally not mounted under a generic
  // /admin path: the same authorization helper applies to every operation.
  app.get(["/community/v1/moderation/reviews", "/community/v1/moderacao/avaliacoes"], asyncHandler((req, res) => listModeration(req, res, "reviews")))
  app.get(["/community/v1/moderation/collections", "/community/v1/moderacao/listas"], asyncHandler((req, res) => listModeration(req, res, "collections")))
  app.patch(["/community/v1/moderation/reviews/:id", "/community/v1/moderacao/avaliacoes/:id"], asyncHandler((req, res) => moderate(req, res, "review")))
  app.patch(["/community/v1/moderation/collections/:id", "/community/v1/moderacao/listas/:id"], asyncHandler((req, res) => moderate(req, res, "collection")))
}

module.exports = {
  registerCommunityRoutes,
  currentUser,
  normalizeReviewInput,
  normalizeCollectionInput,
  normalizeCollectionItems,
  normalizeReportInput,
}
