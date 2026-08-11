// Testes do escopo por conta (node --test, zero dependências).
// ARCADIA_DATA_DIR aponta pra pasta temporária, precisa ser definido ANTES
// do require (DATA_DIR é lido no load do módulo).
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-conta-"))
process.env.ARCADIA_DATA_DIR = DIR

const conta = require("../electron/supabase/conta.js")

test.after(() => fs.rmSync(DIR, { recursive: true, force: true }))

test("migração: owned_games.json da raiz é herdado pela primeira conta", () => {
  fs.writeFileSync(path.join(DIR, "owned_games.json"), '{"440":true}')

  conta.definirConta("u1")

  const herdado = path.join(DIR, "contas", "u1", "owned_games.json")
  assert.ok(fs.existsSync(herdado), "owned_games.json deveria ter sido copiado pra contas/u1/")
  assert.equal(fs.readFileSync(herdado, "utf8"), '{"440":true}')
})

test("caminhoArquivoConta: owned_games.json cai em contas/u1/", () => {
  const caminho = conta.caminhoArquivoConta("owned_games.json")
  assert.equal(caminho, path.join(DIR, "contas", "u1", "owned_games.json"))
})
