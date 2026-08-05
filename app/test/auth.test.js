// Testes do auth — partes offline (sem rede): validações antes de qualquer
// chamada de rede. O signUp com dados válidos chama o RPC username_available
// (rede) e fica para o teste manual (f5-4).
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const path = require("node:path")

const auth = require("../electron/supabase/auth.js")

test("signUp: email inválido falha sem rede", async () => {
  const r = await auth.signUp({ email: "nao-email", username: "teste" })
  assert.deepEqual(r, { ok: false, error: "email_invalido" })
})

test("signUp: username inválido falha sem rede", async () => {
  const r = await auth.signUp({ email: "a@b.com", username: "AB" })
  assert.deepEqual(r, { ok: false, error: "username_invalido" })
})

test("signUp: username com caractere proibido falha", async () => {
  const r = await auth.signUp({ email: "a@b.com", username: "teste@x" })
  assert.deepEqual(r, { ok: false, error: "username_invalido" })
})

test("signUp: email vazio falha", async () => {
  const r = await auth.signUp({ email: "", username: "teste" })
  assert.deepEqual(r, { ok: false, error: "email_invalido" })
})

test("requestCode: email inválido falha sem rede", async () => {
  const r = await auth.requestCode({ email: "x" })
  assert.deepEqual(r, { ok: false, error: "email_invalido" })
})

test("verifyCode: token malformado falha sem rede", async () => {
  const r = await auth.verifyCode({ email: "a@b.com", token: "123" })
  assert.deepEqual(r, { ok: false, error: "codigo_invalido" })
})
