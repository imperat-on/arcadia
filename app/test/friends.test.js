// Testes do módulo de amigos — partes puras (sem rede).
// canonicalPair: a chave do modelo anti-duplicata (user_a < user_b).
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const path = require("node:path")

const { canonicalPair } = require("../electron/supabase/friends.js")

test("canonicalPair ordena ids (user_a < user_b)", () => {
  assert.deepEqual(canonicalPair("b", "a"), { user_a: "a", user_b: "b" })
  assert.deepEqual(canonicalPair("a", "b"), { user_a: "a", user_b: "b" })
})

test("canonicalPair é idempotente (mesmo par → mesma linha)", () => {
  const p1 = canonicalPair("abc-123", "xyz-999")
  const p2 = canonicalPair("xyz-999", "abc-123")
  assert.deepEqual(p1, p2)
})

test("canonicalPair com ids iguais mantém ordem", () => {
  assert.deepEqual(canonicalPair("x", "x"), { user_a: "x", user_b: "x" })
})

test("canonicalPair com uuids grandes", () => {
  const a = "11111111-1111-1111-1111-111111111111"
  const b = "22222222-2222-2222-2222-222222222222"
  assert.deepEqual(canonicalPair(b, a), { user_a: a, user_b: b })
})
