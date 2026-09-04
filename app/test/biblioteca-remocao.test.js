// Testes da REMOCAO no pull: jogos que sumiram do servidor saem da posse
// local. Antes o pull so adicionava — remover na conta em uma maquina nunca
// propagava para as outras (owned_games.json local ficava com o jogo).
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-remocao-"))
process.env.ARCADIA_DATA_DIR = DIR

const conta = require("../electron/supabase/conta.js")
const { getClient } = require("../electron/supabase/client.js")
const biblioteca = require("../electron/supabase/biblioteca.js")

test.after(() => fs.rmSync(DIR, { recursive: true, force: true }))

function preparar({ owned, rows }) {
  conta.definirConta("u1")
  fs.writeFileSync(conta.caminhoArquivoConta("owned_games.json"), JSON.stringify(owned))
  fs.writeFileSync(conta.caminhoArquivoConta("pending_games.json"), JSON.stringify([]))
  fs.writeFileSync(conta.caminhoArquivoConta("custom_games.json"), JSON.stringify([]))
  fs.writeFileSync(conta.caminhoArquivoConta("sync_state.json"), JSON.stringify({ libPush: {}, playtimePush: {} }))

  const client = getClient()
  client.auth.getUser = async () => ({ data: { user: { id: "u1" } }, error: null })
  client.rpc = async (fn) => {
    if (fn === "pull_library") return { data: rows, error: null }
    return { data: null, error: { message: `rpc nao mockada: ${fn}` } }
  }
}

function ownedAtual() {
  return JSON.parse(fs.readFileSync(conta.caminhoArquivoConta("owned_games.json"), "utf-8"))
}

test("pull remove do owned os jogos que sumiram do servidor", async () => {
  // Antes: owned tem [a, b]; servidor agora so devolve [a] (b foi removido
  // em outra maquina)
  preparar({ owned: ["steam:a", "steam:b"], rows: [{ appid: "steam:a", title: "Jogo A", platform: "windows", minutes: 0 }] })

  const mudou = await biblioteca.pull()

  assert.equal(mudou, true, "pull deve sinalizar mudanca")
  assert.deepEqual(ownedAtual(), ["steam:a"], "jogo removido no servidor sai do owned local")
})

test("pull mantem jogos que continuam no servidor", async () => {
  preparar({ owned: ["steam:a", "steam:b"], rows: [
    { appid: "steam:a", title: "Jogo A", platform: "windows", minutes: 0 },
    { appid: "steam:b", title: "Jogo B", platform: "windows", minutes: 0 },
  ] })

  await biblioteca.pull()

  assert.deepEqual(ownedAtual().sort(), ["steam:a", "steam:b"], "nenhum jogo removido quando todos continuam")
})

test("pull com servidor vazio remove tudo do owned", async () => {
  preparar({ owned: ["steam:a", "steam:b"], rows: [] })
  // 1o pull vazio: quorum protege contra wipe acidental
  await biblioteca.pull()
  assert.deepEqual(ownedAtual().sort(), ["steam:a", "steam:b"], "1o pull vazio: quorum protege, owned preservado")
  // 2o pull vazio: confirma a intenção, executa remoção
  await biblioteca.pull()
  assert.deepEqual(ownedAtual(), [], "2o pull vazio: owned limpo")
})

test("pull nao remove jogo custom local que nao esta no servidor", async () => {
  // Jogo custom local (epic:x) esta no custom_games.json — o pull nao deve
  // remove-lo do owned mesmo quando o servidor nao o devolve (o custom e
  // local, nao veio do servidor)
  fs.writeFileSync(
    conta.caminhoArquivoConta("custom_games.json"),
    JSON.stringify([{ id: "epic:x", title: "Meu Custom", launcher: "epic" }]),
  )
  fs.writeFileSync(conta.caminhoArquivoConta("owned_games.json"), JSON.stringify(["epic:x"]))
  fs.writeFileSync(conta.caminhoArquivoConta("pending_games.json"), JSON.stringify([]))
  fs.writeFileSync(conta.caminhoArquivoConta("sync_state.json"), JSON.stringify({ libPush: {}, playtimePush: {} }))

  const client = getClient()
  client.auth.getUser = async () => ({ data: { user: { id: "u1" } }, error: null })
  client.rpc = async (fn) => {
    if (fn === "pull_library") return { data: [], error: null }
    return { data: null, error: { message: `rpc nao mockada: ${fn}` } }
  }

  await biblioteca.pull()

  assert.deepEqual(ownedAtual(), ["epic:x"], "custom local preservado")
})

test("pull remove retro sincronizado do snapshot local quando some do servidor", async () => {
  conta.definirConta("u-retro")
  fs.writeFileSync(
    conta.caminhoArquivoConta("custom_games.json"),
    JSON.stringify([
      {
        id: "retro:ps2:crimson-desert",
        title: "Crimson Desert",
        launcher: "retro",
        retro: true,
        platform: "ps2",
        cover: "https://example.test/crimson.jpg",
      },
    ]),
  )
  fs.writeFileSync(conta.caminhoArquivoConta("owned_games.json"), JSON.stringify(["retro:ps2:crimson-desert"]))
  fs.writeFileSync(conta.caminhoArquivoConta("pending_games.json"), JSON.stringify([]))
  fs.writeFileSync(
    conta.caminhoArquivoConta("sync_state.json"),
    JSON.stringify({
      libPush: {
        "retro:ps2:crimson-desert": { title: "Crimson Desert", platform: "emulator" },
      },
      playtimePush: {},
    }),
  )

  const client = getClient()
  client.auth.getUser = async () => ({ data: { user: { id: "u-retro" } }, error: null })
  client.rpc = async (fn) => {
    if (fn === "pull_library") return { data: [], error: null }
    return { data: null, error: { message: `rpc nao mockada: ${fn}` } }
  }

  const mudou = await biblioteca.pull()
    // 1o pull vazio: quorum protege, sem remoção
    const mudou2 = await biblioteca.pull()  // 2o: confirma remoção

    assert.equal(mudou, false, "1o pull vazio: quorum protege, sem mudanca")
    assert.equal(mudou2, true, "2o pull vazio: executa remoção do retrô")
    assert.deepEqual(JSON.parse(fs.readFileSync(conta.caminhoArquivoConta("custom_games.json"), "utf8")), [], "retrô removido sai do snapshot local")
    assert.deepEqual(ownedAtual(), [], "retrô removido sai da posse local")
    const estadoFinal = JSON.parse(fs.readFileSync(conta.caminhoArquivoConta("sync_state.json"), "utf8"))
    assert.equal(estadoFinal.libPush["retro:ps2:crimson-desert"], undefined, "watermark antigo é limpo para permitir uma futura adição")
  })

test("pull preserva retro local ainda nao sincronizado", async () => {
  conta.definirConta("u-retro-local")
  fs.writeFileSync(
    conta.caminhoArquivoConta("custom_games.json"),
    JSON.stringify([{ id: "retro:snes:local", title: "Jogo local", launcher: "retro", retro: true }]),
  )
  fs.writeFileSync(conta.caminhoArquivoConta("owned_games.json"), JSON.stringify(["retro:snes:local"]))
  fs.writeFileSync(conta.caminhoArquivoConta("pending_games.json"), JSON.stringify([]))
  fs.writeFileSync(conta.caminhoArquivoConta("sync_state.json"), JSON.stringify({ libPush: {}, playtimePush: {} }))

  const client = getClient()
  client.auth.getUser = async () => ({ data: { user: { id: "u-retro-local" } }, error: null })
  client.rpc = async (fn) => {
    if (fn === "pull_library") return { data: [], error: null }
    return { data: null, error: { message: `rpc nao mockada: ${fn}` } }
  }

  await biblioteca.pull()

  assert.deepEqual(JSON.parse(fs.readFileSync(conta.caminhoArquivoConta("custom_games.json"), "utf8")).map((game) => game.id), ["retro:snes:local"], "retrô aguardando push permanece local")
  assert.deepEqual(ownedAtual(), ["retro:snes:local"])
})

test("pull remove jogo com stub pending quando some do servidor (nunca instalado)", async () => {
  // Jogo "steam:2622380" foi adicionado em OUTRA maquina e chegou aqui via
  // pull anterior: ganhou owned + stub em pending_games.json (nunca foi
  // instalado, entao o stub nunca foi limpo pelo indexer). Removido no
  // servidor agora — antes o pendentesIds.has(id) bloqueava a remocao pra
  // sempre e o jogo ficava fantasma nesta maquina.
  conta.definirConta("u3")
  fs.writeFileSync(conta.caminhoArquivoConta("owned_games.json"), JSON.stringify(["steam:2622380"]))
  fs.writeFileSync(
    conta.caminhoArquivoConta("pending_games.json"),
    JSON.stringify([{ id: "steam:2622380", title: "ELDEN RING NIGHTREIGN", launcher: "steam", installed: false, pendente: true }]),
  )
  fs.writeFileSync(conta.caminhoArquivoConta("custom_games.json"), JSON.stringify([]))
  fs.writeFileSync(conta.caminhoArquivoConta("sync_state.json"), JSON.stringify({ libPush: {}, playtimePush: {} }))

  const client = getClient()
  client.auth.getUser = async () => ({ data: { user: { id: "u3" } }, error: null })
  client.rpc = async (fn) => {
    if (fn === "pull_library") return { data: [], error: null } // removido em outra maquina
    return { data: null, error: { message: `rpc nao mockada: ${fn}` } }
  }

  const mudou = await biblioteca.pull()
    // 1o pull vazio: quorum protege, sem remoção
    const mudou2 = await biblioteca.pull()  // 2o: confirma remoção

    assert.equal(mudou, false, "1o pull vazio: quorum protege")
    assert.equal(mudou2, true, "2o pull vazio: executa remoção")
    assert.deepEqual(ownedAtual(), [], "jogo removido no servidor sai do owned mesmo com stub pending")
    const pendentesFinal = JSON.parse(fs.readFileSync(conta.caminhoArquivoConta("pending_games.json"), "utf-8"))
    assert.deepEqual(pendentesFinal, [], "stub pending orfao (jogo sumiu do servidor) e limpo")
  })

test("pull cria watermark para jogos vindos do servidor (remocao local propaga)", async () => {
  conta.definirConta("u2")
  fs.writeFileSync(conta.caminhoArquivoConta("owned_games.json"), JSON.stringify([]))
  fs.writeFileSync(conta.caminhoArquivoConta("pending_games.json"), JSON.stringify([]))
  fs.writeFileSync(conta.caminhoArquivoConta("custom_games.json"), JSON.stringify([]))
  fs.writeFileSync(conta.caminhoArquivoConta("sync_state.json"), JSON.stringify({ libPush: {}, playtimePush: {} }))

  const client = getClient()
  client.auth.getUser = async () => ({ data: { user: { id: "u2" } }, error: null })
  let recebido = null
  client.rpc = async (fn, args) => {
    if (fn === "pull_library") return { data: [{ appid: "steam:1", title: "Jogo Um", platform: "windows", minutes: 0 }], error: null }
    if (fn === "push_library") { recebido = args; return { data: [], error: null } }
    return { data: null, error: { message: `rpc nao mockada: ${fn}` } }
  }

  // 1o pull: jogo vem do servidor, ganha owned + watermark
  await biblioteca.pull()
  const st1 = JSON.parse(fs.readFileSync(conta.caminhoArquivoConta("sync_state.json"), "utf-8"))
  assert.ok(st1.libPush["steam:1"], "pull deve criar watermark para jogo do servidor")

  // Agora o usuario remove localmente (ownedRemove) e o push roda
  const { ownedRemove } = require("../electron/owned.js")
  ownedRemove("steam:1")
  await biblioteca.push()
  assert.ok(recebido, "push deve rodar apos remocao")
  const removido = recebido.p_lib.find((x) => x.appid === "steam:1")
  assert.ok(removido && removido.removed === true, "remocao local propaga removed:true ao servidor")
})
