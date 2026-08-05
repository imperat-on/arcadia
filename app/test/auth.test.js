// Testes do auth — partes offline (sem rede): validações antes de qualquer
// chamada de rede. O signUp/signIn com dados válidos chama o Supabase (rede)
// e fica para o teste manual (f5-4).
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")

const auth = require("../electron/supabase/auth.js")

// ---------- signUp ----------
test("signUp: email inválido falha sem rede", async () => {
  const r = await auth.signUp({ email: "nao-email", username: "teste", password: "123456" })
  assert.deepEqual(r, { ok: false, error: "email_invalido" })
})

test("signUp: username inválido falha sem rede", async () => {
  const r = await auth.signUp({ email: "a@b.com", username: "AB", password: "123456" })
  assert.deepEqual(r, { ok: false, error: "username_invalido" })
})

test("signUp: username com caractere proibido falha", async () => {
  const r = await auth.signUp({ email: "a@b.com", username: "teste@x", password: "123456" })
  assert.deepEqual(r, { ok: false, error: "username_invalido" })
})

test("signUp: senha curta (< 6) falha sem rede", async () => {
  const r = await auth.signUp({ email: "a@b.com", username: "teste", password: "123" })
  assert.deepEqual(r, { ok: false, error: "senha_curta" })
})

test("signUp: sem senha falha", async () => {
  const r = await auth.signUp({ email: "a@b.com", username: "teste" })
  assert.deepEqual(r, { ok: false, error: "senha_curta" })
})

// ---------- signIn ----------
test("signIn: username inválido falha sem rede", async () => {
  const r = await auth.signIn({ username: "AB", password: "123456" })
  assert.deepEqual(r, { ok: false, error: "username_invalido" })
})

test("signIn: sem senha falha", async () => {
  const r = await auth.signIn({ username: "teste" })
  assert.deepEqual(r, { ok: false, error: "senha_curta" })
})
