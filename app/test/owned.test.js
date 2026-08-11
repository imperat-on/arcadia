// Testes do filtro de posse por conta (node --test, zero dependências).
// ARCADIA_DATA_DIR aponta pra pasta temporária, precisa ser definido ANTES
// do require de conta.js (DATA_DIR é lido no load do módulo).
//
// main.js não é requerível aqui: importa "electron" no topo, e fora do
// runtime do Electron esse require devolve uma STRING (caminho do binário),
// não o objeto {app, BrowserWindow...}. A primeira linha do módulo
// (app.requestSingleInstanceLock()) já lança. Por isso o filtro de posse
// mora em electron/owned.js (mesmo padrão de overrides.js: módulo puro,
// sem tocar Electron, testável direto), e main.js só importa e liga no
// readLibrary(). O que fica sem teste automatizado é a linha do
// _libMtimeKey() em main.js que soma caminhoConta(OWNED_GAMES) às mtimes,
// verificada por leitura de código (nenhum teste existente no repo requer
// main.js).
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-owned-"))
process.env.ARCADIA_DATA_DIR = DIR

const conta = require("../electron/supabase/conta.js")
const { filtrarPorPosse, ownedSet, ownedAdd, ownedRemove } = require("../electron/owned.js")

test.after(() => fs.rmSync(DIR, { recursive: true, force: true }))

const GLOBAIS = [{ id: "a" }, { id: "b" }, { id: "c" }]

test("guest ve tudo, mesmo sem owned_games.json", () => {
  conta.definirConta(null)
  assert.equal(filtrarPorPosse(GLOBAIS).length, 3)
})

test("conta com owned filtra pelos ids possuidos", () => {
  conta.definirConta("u1")
  fs.writeFileSync(conta.caminhoArquivoConta("owned_games.json"), JSON.stringify(["a"]))
  assert.deepEqual(filtrarPorPosse(GLOBAIS).map((g) => g.id), ["a"])
})

test("conta sem owned_games.json ve tudo e materializa (migracao)", () => {
  conta.definirConta("u2")
  const arquivo = conta.caminhoArquivoConta("owned_games.json")
  assert.ok(!fs.existsSync(arquivo), "não deveria existir antes da primeira leitura")

  assert.equal(filtrarPorPosse(GLOBAIS).length, 3)

  assert.ok(fs.existsSync(arquivo), "deveria materializar owned_games.json")
  assert.deepEqual(JSON.parse(fs.readFileSync(arquivo, "utf-8")), ["a", "b", "c"])
})

test("mudanca no owned_games.json reflete na proxima leitura (base da cache key)", () => {
  conta.definirConta("u3")
  const arquivo = conta.caminhoArquivoConta("owned_games.json")

  fs.writeFileSync(arquivo, JSON.stringify(["a"]))
  assert.deepEqual(filtrarPorPosse(GLOBAIS).map((g) => g.id), ["a"])

  fs.writeFileSync(arquivo, JSON.stringify(["a", "b"]))
  assert.deepEqual(filtrarPorPosse(GLOBAIS).map((g) => g.id), ["a", "b"])
})

test("ownedAdd com conta grava o id no arquivo", () => {
  conta.definirConta("u4")
  const arquivo = conta.caminhoArquivoConta("owned_games.json")
  assert.ok(!fs.existsSync(arquivo))

  ownedAdd("steam:1")
  assert.deepEqual(ownedSet(), new Set(["steam:1"]))
  assert.deepEqual(JSON.parse(fs.readFileSync(arquivo, "utf-8")), ["steam:1"])

  ownedAdd("epic:2")
  assert.deepEqual(ownedSet(), new Set(["steam:1", "epic:2"]))
})

test("ownedAdd nao duplica id ja possuido", () => {
  conta.definirConta("u5")
  ownedAdd("steam:1")
  ownedAdd("steam:1")
  assert.deepEqual(ownedSet(), new Set(["steam:1"]))
})

test("ownedAdd em guest e no-op, nao cria arquivo", () => {
  conta.definirConta(null)
  ownedAdd("steam:1")
  assert.ok(!fs.existsSync(OWNED_GAMES_GUEST()))
})

test("ownedRemove tira o id do arquivo", () => {
  conta.definirConta("u6")
  const arquivo = conta.caminhoArquivoConta("owned_games.json")
  ownedAdd("steam:1")
  ownedAdd("epic:2")

  ownedRemove("steam:1")
  assert.deepEqual(ownedSet(), new Set(["epic:2"]))
  assert.deepEqual(JSON.parse(fs.readFileSync(arquivo, "utf-8")), ["epic:2"])
})

test("ownedRemove em guest ou id ausente e no-op", () => {
  conta.definirConta("u7")
  assert.doesNotThrow(() => ownedRemove("nao-existe"))

  conta.definirConta(null)
  assert.doesNotThrow(() => ownedRemove("steam:1"))
})

function OWNED_GAMES_GUEST() {
  return path.join(DIR, "owned_games.json")
}
