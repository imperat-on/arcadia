"use strict"

// O arquivo de progresso de um crack fala o NOME REAL da conquista ("ACH19"),
// enquanto o item local pode ter ficado com a chave SINTÉTICA do scrape
// ("ach_15", posicional) — sem apelido nenhum quando a conquista ainda não veio
// do servidor. O nome não casa, e o desbloqueio era descartado EM SILÊNCIO: sem
// erro, sem toast, só o número que não sobe (o caso do GTA San Andreas DE no
// Linux, appid 1547000, com o schema sintético e o emulador GSE gravando ACHnn).
//
// A ponte é o schema que o PRÓPRIO emulador carrega: `steam_settings/achievements.json`
// traz `name` + `displayName` por idioma. O título vira a chave de casamento —
// pela mesma regra do resto do app (match.js): título ÚNICO dentro do appid, ou
// nada. Medido com os dados reais do usuário: antes da correção 0/3 avisos,
// depois 3/3 (25 → 28 desbloqueadas), casando os itens certos por título (não
// por posição).

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const APPID = "999000"
const CONTA = "zes"
const TITULO_LONGO = "Tira meu gatinho da árvore?"

let cw = null

function armar(caminhos) {
  process.env.ARCADIA_DATA_DIR = caminhos.dados
  process.env.APPDATA = path.join(caminhos.raiz, "home", "AppData", "Roaming")
  process.env.STEAM_DIR = caminhos.steam
  process.env.HOME = caminhos.raiz
  fs.mkdirSync(process.env.APPDATA, { recursive: true })
}

function preparar({ itens, schema, itensAmbiguos = false }) {
  const raizBruta = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-schema-emu-"))
  let raiz = raizBruta
  try {
    raiz = fs.realpathSync.native(raizBruta)
  } catch {}
  const dados = path.join(raiz, "dados")
  const prefixo = path.join(raiz, "prefix")
  const jogo = path.join(raiz, "jogo")
  const win64 = path.join(jogo, "Gameface", "Binaries", "Win64")
  const steam = path.join(raiz, "steam")
  for (const d of [path.join(dados, "contas", CONTA), prefixo, win64, steam]) fs.mkdirSync(d, { recursive: true })
  fs.mkdirSync(path.join(prefixo, "drive_c", "users", "steamuser", "AppData", "Roaming"), { recursive: true })
  // Sem loginusers na Steam falsa => "sem Steam" => captura liberada (não é troca de conta).
  fs.writeFileSync(path.join(dados, "config.json"), JSON.stringify({}))
  fs.writeFileSync(
    path.join(dados, "contas", CONTA, "game_settings.json"),
    JSON.stringify({ [`steam:${APPID}`]: { exePath: path.join(win64, "Game.exe"), prefixPath: prefixo } }),
  )
  fs.writeFileSync(path.join(dados, "contas", CONTA, "achievements.json"), JSON.stringify({ [APPID]: { items: itens } }))
  if (schema) {
    fs.mkdirSync(path.join(jogo, "steam_settings"), { recursive: true })
    fs.writeFileSync(path.join(jogo, "steam_settings", "achievements.json"), JSON.stringify(schema))
  }

  armar({ raiz, dados, prefixo, steam })
  const arquivoConta = path.join(dados, "contas", CONTA, "achievements.json")
  return { raiz, dados, prefixo, arquivoConta, pastaJogo: jogo }
}

function caminhoProgresso(prefixo) {
  return path.join(prefixo, "drive_c", "users", "steamuser", "AppData", "Roaming", "GSE Saves", APPID, "achievements.json")
}

function escreverProgresso(prefixo, entradas) {
  const arquivo = caminhoProgresso(prefixo)
  fs.mkdirSync(path.dirname(arquivo), { recursive: true })
  fs.writeFileSync(arquivo, JSON.stringify(entradas))
  return arquivo
}

function esperar(cond, timeoutMs) {
  return new Promise((resolve) => {
    const t0 = Date.now()
    const tick = setInterval(() => {
      if (cond() || Date.now() - t0 > timeoutMs) {
        clearInterval(tick)
        resolve(Date.now() - t0)
      }
    }, 25)
  })
}

function itensDaConta(arquivoConta) {
  return JSON.parse(fs.readFileSync(arquivoConta, "utf-8"))[APPID].items
}

const SCHEMA_GTA = [
  { name: "ACH06", displayName: { brazilian: "Vou querer dois números 9", english: "Nine and a Half" } },
  { name: "ACH12", displayName: { brazilian: "Faz-tudo", english: "Jack of All Trades" } },
  { name: "ACH19", displayName: { brazilian: TITULO_LONGO, english: "Cat in a Tree" } },
]

test("item sintético sem apelido casa pelo título do schema do emulador", async () => {
  const { raiz, prefixo, arquivoConta } = preparar({
    itens: [
      { apiname: "ach_15", title: TITULO_LONGO, block: 14, bit: 0, achieved: false, unlock: 0 },
      { apiname: "ach_17", title: "Outra conquista", block: 16, bit: 0, achieved: false, unlock: 0 },
    ],
    schema: SCHEMA_GTA,
  })
  cw = require(path.join(__dirname, "..", "electron", "achievements", "cracked_watcher.js"))
  require(path.join(__dirname, "..", "electron", "supabase", "conta.js")).definirConta(CONTA)

  // O vigia começa com o progresso zerado e só depois o emulador grava o
  // desbloqueio — é o fluxo real (mtime muda, o vigia rele).
  escreverProgresso(prefixo, [{ name: "ACH19", earned: false, earned_time: 0 }])

  const avisos = []
  const parar = cw.iniciarVigia((p) => avisos.push(p))
  escreverProgresso(prefixo, [{ name: "ACH19", earned: true, earned_time: Math.floor(Date.now() / 1000) }])

  await esperar(() => avisos.length > 0, cw.INTERVALO_POLL)
  parar()

  assert.equal(avisos.length, 1, "o desbloqueio do emulador tem de ser ingerido")
  assert.equal(avisos[0].apiname, "ach_15", "casou com o item de MESMO título, não pelo nome")
  assert.ok(avisos[0].unlock > 0, "veio com o carimbo do arquivo do emulador")

  const itens = itensDaConta(arquivoConta)
  const alvo = itens.find((i) => i.apiname === "ach_15")
  const vizinho = itens.find((i) => i.apiname === "ach_17")
  assert.equal(alvo.achieved, true)
  assert.equal(vizinho.achieved, false, "não marcou a conquista errada (nada de casamento por posição)")
  assert.ok(alvo.aliases.includes("ACH19"), "o nome real do emulador fica gravado como apelido")
  assert.equal(alvo.remoteApiname, "ACH19", "e é a chave que sobe para o servidor (mesma linha da outra máquina)")

  await new Promise((r) => setTimeout(r, 250))
  try {
    fs.rmSync(raiz, { recursive: true, force: true })
  } catch {}
})

test("sem o schema do emulador não há ponte: o desbloqueio continua silencioso", async () => {
  const { raiz, prefixo, arquivoConta } = preparar({
    itens: [{ apiname: "ach_15", title: TITULO_LONGO, block: 14, bit: 0, achieved: false, unlock: 0 }],
    schema: null,
  })
  cw = require(path.join(__dirname, "..", "electron", "achievements", "cracked_watcher.js"))
  require(path.join(__dirname, "..", "electron", "supabase", "conta.js")).definirConta(CONTA)

  escreverProgresso(prefixo, [{ name: "ACH19", earned: false, earned_time: 0 }])
  const avisos = []
  const parar = cw.iniciarVigia((p) => avisos.push(p))
  escreverProgresso(prefixo, [{ name: "ACH19", earned: true, earned_time: Math.floor(Date.now() / 1000) }])
  await esperar(() => avisos.length > 0, 2000)
  parar()

  assert.equal(avisos.length, 0, "sem schema não inventamos um dono para o nome")
  assert.equal(itensDaConta(arquivoConta)[0].achieved, false)

  await new Promise((r) => setTimeout(r, 250))
  try {
    fs.rmSync(raiz, { recursive: true, force: true })
  } catch {}
})

test("título repetido no appid não casa (marcar a errada é pior)", async () => {
  const { raiz, prefixo, arquivoConta } = preparar({
    itens: [
      { apiname: "ach_15", title: TITULO_LONGO, block: 14, bit: 0, achieved: false, unlock: 0 },
      { apiname: "ach_16", title: TITULO_LONGO, block: 15, bit: 0, achieved: false, unlock: 0 },
    ],
    schema: SCHEMA_GTA,
  })
  cw = require(path.join(__dirname, "..", "electron", "achievements", "cracked_watcher.js"))
  require(path.join(__dirname, "..", "electron", "supabase", "conta.js")).definirConta(CONTA)

  escreverProgresso(prefixo, [{ name: "ACH19", earned: false, earned_time: 0 }])
  const avisos = []
  const parar = cw.iniciarVigia((p) => avisos.push(p))
  escreverProgresso(prefixo, [{ name: "ACH19", earned: true, earned_time: Math.floor(Date.now() / 1000) }])
  await esperar(() => avisos.length > 0, 2000)
  parar()

  assert.equal(avisos.length, 0)
  assert.deepEqual(
    itensDaConta(arquivoConta).map((i) => i.achieved),
    [false, false],
  )

  await new Promise((r) => setTimeout(r, 250))
  try {
    fs.rmSync(raiz, { recursive: true, force: true })
  } catch {}
})

test("o schema é achado subindo diretórios a partir do exe (steam_settings na raiz do jogo)", () => {
  const { raiz, pastaJogo } = preparar({ itens: [], schema: SCHEMA_GTA })
  cw = require(path.join(__dirname, "..", "electron", "achievements", "cracked_watcher.js"))

  const titulos = cw.titulosDoSchemaEmulador(path.join(pastaJogo, "Gameface", "Binaries", "Win64"))
  assert.ok(titulos, "achou o steam_settings/achievements.json subindo do exe até a raiz")
  assert.deepEqual(titulos.get("ach19"), [TITULO_LONGO, "Cat in a Tree"])
  const vazio = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-sem-schema-"))
  fs.mkdirSync(path.join(vazio, "jogo", "Win64"), { recursive: true })
  assert.equal(
    cw.titulosDoSchemaEmulador(path.join(vazio, "jogo", "Win64")),
    null,
    "sem steam_settings em lugar nenhum devolve null",
  )
  try {
    fs.rmSync(vazio, { recursive: true, force: true })
  } catch {}

  try {
    fs.rmSync(raiz, { recursive: true, force: true })
  } catch {}
})
