// Testes do tempo de jogo retrô: minutos locais devem usar o mesmo contrato
// de sync dos demais jogos e voltar para a conta em outro dispositivo.
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-playtime-retro-"))
process.env.ARCADIA_DATA_DIR = DIR

const conta = require("../electron/supabase/conta.js")
const { getClient } = require("../electron/supabase/client.js")
const biblioteca = require("../electron/supabase/biblioteca.js")

const ID = "retro:ps2:crimson-desert"
const COVER = "https://example.test/crimson-cover.jpg"
const HERO = "https://example.test/crimson-hero.jpg"

test.after(() => fs.rmSync(DIR, { recursive: true, force: true }))

function arquivo(nome) {
  return conta.caminhoArquivoConta(nome)
}

function escreverConta(username, { minutos = 0, watermark = true } = {}) {
  conta.definirConta(username)
  fs.writeFileSync(arquivo("owned_games.json"), JSON.stringify([ID]))
  fs.writeFileSync(
    arquivo("custom_games.json"),
    JSON.stringify([{ id: ID, title: "Crimson Desert", launcher: "retro", retro: true, platform: "ps2", cover: COVER, hero: HERO }]),
  )
  fs.writeFileSync(arquivo("pending_games.json"), JSON.stringify([]))
  fs.writeFileSync(arquivo("overrides.json"), JSON.stringify(minutos ? { [ID]: { playtime_added_minutes: minutos } } : {}))
  fs.writeFileSync(
    arquivo("sync_state.json"),
    JSON.stringify({
      libPush: watermark ? { [ID]: { title: "Crimson Desert", platform: "emulator", cover: COVER, hero: HERO, icon: COVER } } : {},
      playtimePush: {},
    }),
  )
}

function autenticar(username) {
  const client = getClient()
  client.auth.getUser = async () => ({ data: { user: { id: username } }, error: null })
  return client
}

test("push envia minutos de jogo retrô ao RPC e atualiza o watermark da conta", async () => {
  escreverConta("retro-play-push", { minutos: 37 })
  const client = autenticar("retro-play-push")
  let recebido = null
  client.rpc = async (fn, args) => {
    if (fn === "push_library") {
      recebido = args
      return { data: [], error: null }
    }
    return { data: [], error: null }
  }

  await biblioteca.push()

  assert.ok(recebido, "push_library deve ser chamado para minutos retrô")
  assert.deepEqual(recebido.p_playtime, [{ appid: ID, minutes: 37 }])
  const state = JSON.parse(fs.readFileSync(arquivo("sync_state.json"), "utf8"))
  assert.equal(state.playtimePush[ID], 37, "watermark de minutos deve ficar salvo na conta")
})

test("pull aplica minutos retrô vindos do servidor no override da conta", async () => {
  escreverConta("retro-play-pull", { watermark: false })
  fs.writeFileSync(arquivo("overrides.json"), JSON.stringify({ [ID]: { cover: "/home/pc-a/.local/share/arcadia/art/bully.png" } }))
  const client = autenticar("retro-play-pull")
  client.rpc = async (fn) => {
    if (fn === "pull_library") {
      return {
        data: [{ appid: ID, title: "Crimson Desert", platform: "emulator", retro: true, cover: COVER, icon: "https://example.test/legacy-icon.jpg", hero: HERO, minutes: 52 }],
        error: null,
      }
    }
    return { data: [], error: null }
  }

  const mudou = await biblioteca.pull()

  assert.equal(mudou, true)
  const overrides = JSON.parse(fs.readFileSync(arquivo("overrides.json"), "utf8"))
  assert.equal(overrides[ID].playtime_added_minutes, 52, "horas retrô devem ser aplicadas localmente")
  assert.equal(overrides[ID].cover, COVER, "override local deve ser substituído pela URL portátil")
  const custom = JSON.parse(fs.readFileSync(arquivo("custom_games.json"), "utf8"))
  assert.equal(custom[0].icon, COVER, "a capa canônica deve ser usada como ícone retrô em todas as máquinas")
  const state = JSON.parse(fs.readFileSync(arquivo("sync_state.json"), "utf8"))
  assert.equal(state.playtimePush[ID], 52, "watermark puxado deve ficar salvo na conta")
})

test("pull reenvia minutos locais quando o servidor perdeu o total retrô", async () => {
  escreverConta("retro-play-repair", { minutos: 112 })
  const client = autenticar("retro-play-repair")
  let recebido = null
  client.rpc = async (fn, args) => {
    if (fn === "pull_library") {
      return {
        data: [{ appid: ID, title: "Crimson Desert", platform: "emulator", retro: true, cover: COVER, hero: HERO, minutes: 0 }],
        error: null,
      }
    }
    if (fn === "push_library") {
      recebido = args
      return { data: [], error: null }
    }
    return { data: [], error: null }
  }

  // Simula uma conta cujo watermark local já dizia 112, mas cujo total remoto
  // voltou a zero. O pull deve detectar a divergência e reparar o servidor.
  const state = JSON.parse(fs.readFileSync(arquivo("sync_state.json"), "utf8"))
  state.playtimePush[ID] = 112
  fs.writeFileSync(arquivo("sync_state.json"), JSON.stringify(state))

  await biblioteca.pull()

  assert.deepEqual(recebido?.p_playtime, [{ appid: ID, minutes: 112 }], "o total local deve ser reenviado ao servidor")
  const finalState = JSON.parse(fs.readFileSync(arquivo("sync_state.json"), "utf8"))
  assert.equal(finalState.playtimePush[ID], 112, "o watermark deve continuar representando o total local")
})
