"use strict"

// Shared, dependency-free validation for the community API.  Keeping these
// helpers separate makes limits and canonical representations testable without
// opening a PostgreSQL connection (and avoids trusting Express' coercions).

const MAX_REVIEW_TEXT = 4_000
const MAX_REVIEW_TITLE = 120
const MAX_COLLECTION_TITLE = 120
const MAX_COLLECTION_DESCRIPTION = 2_000
const MAX_COLLECTION_ITEMS = 500
const MAX_ITEM_NOTE = 500
const MAX_ITEM_TITLE = 200
const MAX_PAGE_SIZE = 100
const DEFAULT_PAGE_SIZE = 25

const VISIBILITIES = new Set(["public", "unlisted", "private"])
const REVIEW_STATUSES = new Set(["visible", "pending", "hidden", "rejected"])
const COLLECTION_STATUSES = new Set(["visible", "pending", "hidden", "rejected"])

function invalid(field, code, message) {
  return { field, code, message }
}

function text(value, field, max, { required = false, collapse = false } = {}) {
  if (value === undefined || value === null) {
    return required ? { error: invalid(field, `${field}_obrigatorio`, `${field} e obrigatorio`) } : { value: null }
  }
  if (typeof value !== "string") {
    return { error: invalid(field, `${field}_invalido`, `${field} deve ser texto`) }
  }
  let normalized = value.normalize("NFC").trim()
  if (collapse) normalized = normalized.replace(/\s+/gu, " ")
  if (!normalized) {
    return required ? { error: invalid(field, `${field}_vazio`, `${field} nao pode ficar vazio`) } : { value: null }
  }
  if ([...normalized].length > max) {
    return { error: invalid(field, `${field}_muito_longo`, `${field} excede o limite de ${max} caracteres`) }
  }
  return { value: normalized }
}

function normalizeAppid(value) {
  const candidate = typeof value === "number" && Number.isSafeInteger(value)
    ? String(value)
    : typeof value === "string" ? value.trim() : ""
  if (!/^\d{1,10}$/.test(candidate)) {
    return { error: invalid("appid", "appid_invalido", "appid deve conter de 1 a 10 digitos") }
  }
  // Steam appids are represented canonically throughout the API.  Keep zero
  // as "0" instead of returning an empty string for defensive callers.
  return { value: candidate.replace(/^0+(?=\d)/, "") }
}

function normalizeRating(value) {
  if (value === undefined || value === null || value === "") return { value: null }
  const rating = typeof value === "string" && /^\d+$/.test(value.trim()) ? Number(value.trim()) : value
  if (!Number.isSafeInteger(rating) || rating < 1 || rating > 5) {
    return { error: invalid("rating", "rating_invalido", "rating deve ser um inteiro entre 1 e 5") }
  }
  return { value: rating }
}

function normalizePositive(value, rating) {
  if (value === undefined || value === null || value === "") {
    return { value: rating === null ? 1 : rating >= 3 ? 1 : 0 }
  }
  if (typeof value !== "boolean" && value !== 0 && value !== 1 && value !== "0" && value !== "1") {
    return { error: invalid("positive", "positive_invalido", "positive deve ser booleano") }
  }
  return { value: value === true || value === 1 || value === "1" ? 1 : 0 }
}

function normalizeHours(value) {
  if (value === undefined || value === null || value === "") return { value: 0 }
  const hours = typeof value === "string" && value.trim() !== "" ? Number(value) : value
  if (typeof hours !== "number" || !Number.isFinite(hours) || hours < 0 || hours > 999_999) {
    return { error: invalid("hours", "hours_invalido", "hours deve estar entre 0 e 999999") }
  }
  return { value: Math.round(hours * 100) / 100 }
}

function normalizeReviewInput(body = {}, routeAppid = undefined) {
  if (!body || typeof body !== "object" || Array.isArray(body)) {
    return { error: invalid("body", "payload_invalido", "payload deve ser um objeto") }
  }
  const appid = normalizeAppid(body.appid ?? body.app_id ?? body.game_id ?? body.gameId ?? routeAppid)
  if (appid.error) return appid
  const title = text(body.title, "title", MAX_REVIEW_TITLE, { collapse: true })
  if (title.error) return title
  const reviewText = text(body.text ?? body.review ?? body.content ?? body.comment, "text", MAX_REVIEW_TEXT, { required: true })
  if (reviewText.error) return reviewText
  const rating = normalizeRating(body.rating ?? body.score)
  if (rating.error) return rating
  const positive = normalizePositive(body.positive, rating.value)
  if (positive.error) return positive
  const hours = normalizeHours(body.hours)
  if (hours.error) return hours
  return {
    value: {
      appid: appid.value,
      title: title.value,
      text: reviewText.value,
      rating: rating.value ?? (positive.value ? 5 : 1),
      positive: positive.value,
      hours: hours.value,
    },
  }
}

function normalizeVisibility(value, fallback = "public") {
  const visibility = value === undefined || value === null || value === "" ? fallback : value
  if (typeof visibility !== "string" || !VISIBILITIES.has(visibility.trim().toLowerCase())) {
    return { error: invalid("visibility", "visibilidade_invalida", "visibility deve ser public, unlisted ou private") }
  }
  return { value: visibility.trim().toLowerCase() }
}

function normalizeCollectionItem(item, index = 0) {
  if (typeof item === "string" || typeof item === "number") item = { appid: item }
  if (!item || typeof item !== "object" || Array.isArray(item)) {
    return { error: invalid(`items[${index}]`, "item_invalido", "cada item deve ser um objeto ou appid") }
  }
  const appid = normalizeAppid(item.appid ?? item.app_id ?? item.game_id ?? item.id)
  if (appid.error) return { error: { ...appid.error, field: `items[${index}].appid` } }
  const title = text(item.title ?? item.name, `items[${index}].title`, MAX_ITEM_TITLE, { collapse: true })
  if (title.error) return title
  const note = text(item.note ?? item.description, `items[${index}].note`, MAX_ITEM_NOTE)
  if (note.error) return note
  let position = index
  if (item.position !== undefined && item.position !== null && item.position !== "") {
    const raw = typeof item.position === "string" && /^\d+$/.test(item.position.trim())
      ? Number(item.position.trim()) : item.position
    if (!Number.isSafeInteger(raw) || raw < 0 || raw >= MAX_COLLECTION_ITEMS) {
      return { error: invalid(`items[${index}].position`, "posicao_invalida", `position deve estar entre 0 e ${MAX_COLLECTION_ITEMS - 1}`) }
    }
    position = raw
  }
  return { value: { appid: appid.value, title: title.value, note: note.value, position } }
}

function normalizeCollectionItems(items) {
  if (items === undefined || items === null) return { value: [] }
  if (!Array.isArray(items)) return { error: invalid("items", "items_invalido", "items deve ser uma lista") }
  if (items.length > MAX_COLLECTION_ITEMS) {
    return { error: invalid("items", "items_limite_excedido", `uma colecao pode conter no maximo ${MAX_COLLECTION_ITEMS} itens`) }
  }
  const normalized = []
  const appids = new Set()
  for (let i = 0; i < items.length; i++) {
    const item = normalizeCollectionItem(items[i], i)
    if (item.error) return item
    if (appids.has(item.value.appid)) {
      return { error: invalid(`items[${i}].appid`, "item_duplicado", "appid nao pode aparecer duas vezes na colecao") }
    }
    appids.add(item.value.appid)
    normalized.push({ ...item.value, position: i })
  }
  return { value: normalized }
}

function normalizeCollectionInput(body = {}, { partial = false } = {}) {
  if (!body || typeof body !== "object" || Array.isArray(body)) {
    return { error: invalid("body", "payload_invalido", "payload deve ser um objeto") }
  }
  const value = {}
  if (!partial || Object.prototype.hasOwnProperty.call(body, "title")) {
    const title = text(body.title, "title", MAX_COLLECTION_TITLE, { required: true, collapse: true })
    if (title.error) return title
    value.title = title.value
  }
  if (!partial || Object.prototype.hasOwnProperty.call(body, "description")) {
    const description = text(body.description, "description", MAX_COLLECTION_DESCRIPTION)
    if (description.error) return description
    value.description = description.value
  }
  if (!partial || Object.prototype.hasOwnProperty.call(body, "visibility")) {
    const visibility = normalizeVisibility(body.visibility ?? body.privacy, "public")
    if (visibility.error) return visibility
    value.visibility = visibility.value
  }
  if (Object.prototype.hasOwnProperty.call(body, "items")) {
    const items = normalizeCollectionItems(body.items)
    if (items.error) return items
    value.items = items.value
  }
  if (partial && !Object.keys(value).length) {
    return { error: invalid("body", "sem_campos", "nenhum campo alteravel foi informado") }
  }
  return { value }
}

function parsePagination(query = {}) {
  const rawLimit = query.limit ?? query.limite
  const rawOffset = query.offset ?? query.deslocamento ?? 0
  let limit = DEFAULT_PAGE_SIZE
  if (rawLimit !== undefined && rawLimit !== "") {
    if (!/^\d+$/.test(String(rawLimit).trim())) {
      return { error: invalid("limit", "limit_invalido", "limit deve ser um inteiro positivo") }
    }
    limit = Math.max(1, Math.min(Number(rawLimit), MAX_PAGE_SIZE))
  }
  if (!/^\d+$/.test(String(rawOffset).trim())) {
    return { error: invalid("offset", "offset_invalido", "offset deve ser um inteiro nao negativo") }
  }
  const offset = Number(rawOffset)
  if (!Number.isSafeInteger(offset)) {
    return { error: invalid("offset", "offset_invalido", "offset excede o limite numerico") }
  }
  return { value: { limit, offset } }
}

function normalizeReportInput(body = {}) {
  if (!body || typeof body !== "object" || Array.isArray(body)) {
    return { error: invalid("body", "payload_invalido", "payload deve ser um objeto") }
  }
  const reason = text(body.reason ?? body.category, "reason", 80, { required: true, collapse: true })
  if (reason.error) return reason
  const details = text(body.details ?? body.comment, "details", 1_000)
  if (details.error) return details
  return { value: { reason: reason.value, details: details.value } }
}

function validationResponse(error) {
  return {
    status: 400,
    code: error?.code || "payload_invalido",
    details: error?.field ? { field: error.field } : undefined,
  }
}

module.exports = {
  MAX_REVIEW_TEXT,
  MAX_REVIEW_TITLE,
  MAX_COLLECTION_TITLE,
  MAX_COLLECTION_DESCRIPTION,
  MAX_COLLECTION_ITEMS,
  MAX_ITEM_NOTE,
  MAX_ITEM_TITLE,
  MAX_PAGE_SIZE,
  DEFAULT_PAGE_SIZE,
  VISIBILITIES,
  REVIEW_STATUSES,
  COLLECTION_STATUSES,
  normalizeAppid,
  normalizeRating,
  normalizePositive,
  normalizeHours,
  normalizeReviewInput,
  normalizeVisibility,
  normalizeCollectionItem,
  normalizeCollectionItems,
  normalizeCollectionInput,
  normalizeReportInput,
  parsePagination,
  validationResponse,
}
