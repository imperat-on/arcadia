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
const { getClient } = require("../electron/supabase/client.js")
const biblioteca = require("../electron/supabase/biblioteca.js")

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

// ---------- push()/pull() de posse (biblioteca.js) ----------
// getClient().rpc e getUser sao mockados na instancia real (mesmo padrao
// dos outros testes de sync do app: sem rede, sem servidor de teste).
const client = getClient()

test("push: ids possuidos sem watermark sobem como stub no push_library", async () => {
  conta.definirConta("push-owned-1")
  fs.writeFileSync(conta.caminhoArquivoConta("custom_games.json"), "[]")
  ownedAdd("game-a")
  ownedAdd("game-b")

  let recebido = null
  client.auth.getUser = async () => ({ data: { user: { id: "u-push" } } })
  client.rpc = async (fn, args) => {
    if (fn === "push_library") recebido = args
    return { data: null, error: null }
  }

  await biblioteca.push()

  assert.ok(recebido, "push_library deveria ter sido chamado")
  const enviados = recebido.p_lib.filter((g) => g.appid === "game-a" || g.appid === "game-b")
  assert.deepEqual(
    enviados.map((g) => g.appid).sort(),
    ["game-a", "game-b"]
  )
  for (const g of enviados) {
    assert.equal(g.title, g.appid, "stub sobe com title = id")
    assert.equal(g.platform, "windows")
  }
})

test("pull: row nao possuida ganha owned + custom_games ganha stub + removidos somem (regressao)", async () => {
  conta.definirConta("pull-owned-1")
  fs.writeFileSync(conta.caminhoArquivoConta("custom_games.json"), "[]")
  // "ja-tinha" esta no owned local mas o servidor NAO o devolve mais — foi
  // removido em outra maquina. O pull deve remove-lo da posse local.
  fs.writeFileSync(conta.caminhoArquivoConta("owned_games.json"), JSON.stringify(["ja-tinha"]))

  client.auth.getUser = async () => ({ data: { user: { id: "u-pull-1" } } })
  client.rpc = async (fn) => {
    if (fn === "pull_library") {
      return { data: [{ appid: "game-c", title: "Game C", platform: "windows", minutes: 0 }], error: null }
    }
    return { data: null, error: null }
  }

  const mudou = await biblioteca.pull()
  assert.equal(mudou, true)

  // game-c entra, ja-tinha sai (removido no servidor)
  assert.deepEqual(ownedSet(), new Set(["game-c"]))

  const lib = JSON.parse(fs.readFileSync(conta.caminhoArquivoConta("custom_games.json"), "utf-8"))
  assert.deepEqual(lib.map((g) => g.id), ["game-c"])
  assert.equal(lib[0].launcher, "custom")
  assert.equal(lib[0].exe, "")
})

test("pull: owned_games.json ausente (null = possui tudo, constraint 7) nao e criado pelo pull", async () => {
  conta.definirConta("pull-owned-2")
  fs.writeFileSync(conta.caminhoArquivoConta("custom_games.json"), "[]")
  const arquivoOwned = conta.caminhoArquivoConta("owned_games.json")
  assert.ok(!fs.existsSync(arquivoOwned))

  client.auth.getUser = async () => ({ data: { user: { id: "u-pull-2" } } })
  client.rpc = async (fn) => {
    if (fn === "pull_library") {
      return { data: [{ appid: "game-d", title: "Game D", platform: "windows", minutes: 0 }], error: null }
    }
    return { data: null, error: null }
  }

  await biblioteca.pull()
  assert.ok(!fs.existsSync(arquivoOwned), "pull nao deveria materializar owned_games.json")
})

function OWNED_GAMES_GUEST() {
  return path.join(DIR, "owned_games.json")
}
