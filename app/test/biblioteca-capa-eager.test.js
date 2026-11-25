// Testes do enriquecimento EAGER de capa/hero/icone ao criar um stub pending
// no pull. Antes o stub nascia so com URLs de capa "chutadas" (podiam nao
// existir) e sem icone nenhum — a correcao real so rodava depois, ao abrir
// library:get (curarCapasSteam), e dava um flash de carregamento visivel na
// sidebar. Agora o pull busca a arte real ANTES de criar o stub.
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-capa-eager-"))
process.env.ARCADIA_DATA_DIR = DIR

const conta = require("../electron/supabase/conta.js")
const { getClient } = require("../electron/supabase/client.js")
const biblioteca = require("../electron/supabase/biblioteca.js")
const steamstore = require("../electron/steamstore.js")

test.after(() => fs.rmSync(DIR, { recursive: true, force: true }))

function preparar({ rows }) {
  conta.definirConta("u_capa")
  fs.writeFileSync(conta.caminhoArquivoConta("owned_games.json"), JSON.stringify([]))
  fs.writeFileSync(conta.caminhoArquivoConta("pending_games.json"), JSON.stringify([]))
  fs.writeFileSync(conta.caminhoArquivoConta("custom_games.json"), JSON.stringify([]))
  fs.writeFileSync(conta.caminhoArquivoConta("sync_state.json"), JSON.stringify({ libPush: {}, playtimePush: {} }))

  const client = getClient()
  client.auth.getUser = async () => ({ data: { user: { id: "u_capa" } }, error: null })
  client.rpc = async (fn) => {
    if (fn === "pull_library") return { data: rows, error: null }
    return { data: null, error: { message: `rpc nao mockada: ${fn}` } }
  }
}

function pendentesAtual() {
  return JSON.parse(fs.readFileSync(conta.caminhoArquivoConta("pending_games.json"), "utf-8"))
}

test("pull usa capa/hero/icone reais da loja ao criar o stub pending", async () => {
  const original = steamstore.itensDaLoja
  steamstore.itensDaLoja = async (appids) => {
    assert.deepEqual(appids, ["555"], "busca so o appid do jogo novo")
    const mapa = new Map()
    mapa.set("555", { tipo: 0, capa: "https://real/capa.jpg", heroi: "https://real/hero.jpg", icon: "https://real/icon.jpg" })
    return { mapa, respondidos: new Set(["555"]) }
  }
  try {
    preparar({ rows: [{ appid: "steam:555", title: "Jogo Real", platform: "windows", minutes: 0 }] })
    await biblioteca.pull()
  } finally {
    steamstore.itensDaLoja = original
  }

  const [stub] = pendentesAtual()
  assert.equal(stub.cover, "https://real/capa.jpg", "capa vem da loja, nao da URL chutada")
  assert.equal(stub.hero, "https://real/hero.jpg", "hero vem da loja")
  assert.equal(stub.icon, "https://real/icon.jpg", "icone ja vem preenchido (sidebar nao pisca)")
})

test("pull cai na URL chutada quando a loja nao responde (rede fora)", async () => {
  const original = steamstore.itensDaLoja
  steamstore.itensDaLoja = async () => {
    throw new Error("rede fora")
  }
  try {
    preparar({ rows: [{ appid: "steam:777", title: "Jogo Sem Rede", platform: "windows", minutes: 0 }] })
    await biblioteca.pull()
  } finally {
    steamstore.itensDaLoja = original
  }

  const [stub] = pendentesAtual()
  assert.equal(stub.cover, "https://cdn.cloudflare.steamstatic.com/steam/apps/777/library_600x900.jpg")
  assert.equal(stub.icon, "", "sem resposta da loja, icone fica vazio ate a cura lazy rodar")
})
