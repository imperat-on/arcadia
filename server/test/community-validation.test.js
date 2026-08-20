"use strict"

// These tests intentionally import only community-validation.js: malformed
// payloads and limit behavior remain covered on machines without PostgreSQL.
const test = require("node:test")
const assert = require("node:assert/strict")
const {
  MAX_COLLECTION_ITEMS,
  MAX_REVIEW_TEXT,
  normalizeAppid,
  normalizeReviewInput,
  normalizeCollectionInput,
  normalizeCollectionItems,
  normalizeReportInput,
  parsePagination,
} = require("../src/community-validation")

test("appid is canonicalized and rejects path/query injection", () => {
  assert.deepEqual(normalizeAppid(" 000730 "), { value: "730" })
  assert.deepEqual(normalizeAppid(730), { value: "730" })
  assert.match(normalizeAppid("730/../x").error.code, /appid_invalido/)
  assert.match(normalizeAppid("12345678901").error.code, /appid_invalido/)
})

test("review payload normalizes whitespace and legacy positive values", () => {
  const review = normalizeReviewInput({
    appid: "730",
    text: "  Ótimo\n\n jogo  ",
    positive: false,
    hours: "12.345",
  })
  assert.equal(review.value.appid, "730")
  assert.equal(review.value.text, "Ótimo\n\n jogo")
  assert.equal(review.value.rating, 1)
  assert.equal(review.value.positive, 0)
  assert.equal(review.value.hours, 12.35)
})

test("review and collection limits produce field-specific errors", () => {
  const longText = normalizeReviewInput({ appid: "10", text: "x".repeat(MAX_REVIEW_TEXT + 1) })
  assert.equal(longText.error.field, "text")
  assert.equal(longText.error.code, "text_muito_longo")
  const tooMany = normalizeCollectionItems(Array.from({ length: MAX_COLLECTION_ITEMS + 1 }, (_, i) => ({ appid: String(i + 1) })))
  assert.equal(tooMany.error.code, "items_limite_excedido")
  const duplicated = normalizeCollectionItems([{ appid: "1" }, { appid: "0001" }])
  assert.equal(duplicated.error.code, "item_duplicado")
})

test("collection normalizes privacy and preserves optional fields", () => {
  const collection = normalizeCollectionInput({
    title: "  Meus jogos favoritos ",
    description: "  para zerar  ",
    visibility: "UNLISTED",
    items: [{ appid: "00010", title: " Half-Life ", note: " jogar" }],
  })
  assert.equal(collection.value.title, "Meus jogos favoritos")
  assert.equal(collection.value.description, "para zerar")
  assert.equal(collection.value.visibility, "unlisted")
  assert.deepEqual(collection.value.items[0], { appid: "10", title: "Half-Life", note: "jogar", position: 0 })
  assert.equal(normalizeCollectionInput({ title: "x", visibility: "friends" }).error.code, "visibilidade_invalida")
})

test("pagination is bounded while malformed values are rejected", () => {
  assert.deepEqual(parsePagination({ limit: "999", offset: "2" }).value, { limit: 100, offset: 2 })
  assert.equal(parsePagination({ limit: "-1" }).error.code, "limit_invalido")
  assert.equal(parsePagination({ offset: "NaN" }).error.code, "offset_invalido")
})

test("report payload is normalized and bounded", () => {
  const report = normalizeReportInput({ reason: " spam ", details: " texto " })
  assert.deepEqual(report.value, { reason: "spam", details: "texto" })
  assert.equal(normalizeReportInput({ reason: "" }).error.code, "reason_vazio")
})
