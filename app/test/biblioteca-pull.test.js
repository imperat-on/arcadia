// Testes do pull da biblioteca — cria stub pending para jogos steam:* vindos
// do servidor (feature de sync entre máquinas). ARCADIA_DATA_DIR aponta pra
// pasta temporária ANTES do require (padrão da suite).
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-bibpull-"))
process.env.ARCADIA_DATA_DIR = DIR

const conta = require("../electron/supabase/conta.js")
const { getClient } = require("../electron/supabase/client.js")
const biblioteca = require("../electron/supabase/biblioteca.js")

test.after(() => fs.rmSync(DIR, { recursive: true, force: true }))

// Setup: conta logada + mock do servidor (pull_library devolve rows).
function preparar(rows) {
  conta.definirConta("u1")
  // owned_games.json existente para o pull poder mexer na posse
  fs.writeFileSync(conta.caminhoArquivoConta("owned_games.json"), JSON.stringify([]))
  fs.writeFileSync(conta.caminhoArquivoConta("custom_games.json"), JSON.stringify([]))
  fs.writeFileSync(conta.caminhoArquivoConta("pending_games.json"), JSON.stringify([]))

  const client = getClient()
  client.auth.getUser = async () => ({ data: { user: { id: "u1" } }, error: null })
  client.rpc = async (fn) => {
    if (fn === "pull_library") return { data: rows, error: null }
    return { data: null, error: { message: `rpc nao mockada: ${fn}` } }
  }
}

test("pull cria stub pending para jogo steam:* que falta localmente", async () => {
  preparar([{ appid: "steam:1091500", title: "Cyberpunk 2077", platform: "windows", minutes: 0 }])

  const mudou = await biblioteca.pull()

  const pending = JSON.parse(fs.readFileSync(conta.caminhoArquivoConta("pending_games.json"), "utf-8"))
  assert.equal(mudou, true, "pull deve sinalizar mudanca")
  assert.equal(pending.length, 1, "deve criar um stub pending")
  assert.equal(pending[0].id, "steam:1091500")
  assert.equal(pending[0].title, "Cyberpunk 2077")
  assert.equal(pending[0].launcher, "steam")
  assert.equal(pending[0].pendente, true)
  // Não deve entrar no custom_games (steam vai pro pending, não custom)
  const custom = JSON.parse(fs.readFileSync(conta.caminhoArquivoConta("custom_games.json"), "utf-8"))
  assert.equal(custom.length, 0, "jogo steam nao vai pro custom_games")
})

test("pull usa fallback de titulo quando servidor manda appid como titulo", async () => {
  preparar([{ appid: "steam:812140", title: "steam:812140", platform: "windows", minutes: 0 }])

  await biblioteca.pull()

  const pending = JSON.parse(fs.readFileSync(conta.caminhoArquivoConta("pending_games.json"), "utf-8"))
  assert.equal(pending.length, 1)
  assert.equal(pending[0].title, "Steam 812140", "titulo feio vira 'Steam <appid>'")
})

test("pull nao duplica stub se o jogo ja esta no pending", async () => {
  fs.writeFileSync(
    conta.caminhoArquivoConta("pending_games.json"),
    JSON.stringify([{ id: "steam:1091500", title: "Cyberpunk 2077", launcher: "steam" }]),
  )
  preparar([{ appid: "steam:1091500", title: "Cyberpunk 2077", platform: "windows", minutes: 0 }])

  await biblioteca.pull()

  const pending = JSON.parse(fs.readFileSync(conta.caminhoArquivoConta("pending_games.json"), "utf-8"))
  assert.equal(pending.length, 1, "nao duplica stub ja existente")
})

test("pull nao duplica jogo custom (nao-steam) existente", async () => {
  fs.writeFileSync(
    conta.caminhoArquivoConta("custom_games.json"),
    JSON.stringify([{ id: "epic:abc", title: "Meu Jogo", launcher: "epic" }]),
  )
  preparar([{ appid: "epic:abc", title: "Meu Jogo", platform: "windows", minutes: 0 }])

  await biblioteca.pull()

  const custom = JSON.parse(fs.readFileSync(conta.caminhoArquivoConta("custom_games.json"), "utf-8"))
  assert.equal(custom.length, 1, "nao duplica custom existente")
})

test("pull preserva metadata e identifica retrô vindo da conta", async () => {
  preparar([
    {
      appid: "retro:nes:sha1:abc123",
      title: "Super Mario Bros.",
      platform: "emulator",
      retro: true,
      systemId: "nes",
      cover: "https://cdn.example.test/mario-cover.jpg",
      hero: "https://cdn.example.test/mario-hero.jpg",
    },
  ])

  await biblioteca.pull()

  const custom = JSON.parse(fs.readFileSync(conta.caminhoArquivoConta("custom_games.json"), "utf-8"))
  assert.equal(custom.length, 1)
  assert.equal(custom[0].launcher, "retro")
  assert.equal(custom[0].retro, true)
  assert.equal(custom[0].systemId, "nes")
  assert.equal(custom[0].platform, "nes")
  assert.equal(custom[0].cover, "https://cdn.example.test/mario-cover.jpg")
  assert.equal(custom[0].hero, "https://cdn.example.test/mario-hero.jpg")
})
