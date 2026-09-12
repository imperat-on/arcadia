// Reconciliação de conquistas entre ESPAÇOS DE CHAVE diferentes.
//
// Regressão do bug real: a máquina Windows subiu os desbloqueios do GTA SA com
// os apinames reais ("ACH08"); a máquina Linux tinha o MESMO jogo indexado com
// apinames sintéticos do scrape ("ach_02"). O pull criava um item paralelo (sem
// block/bit) e o loadAllSchemas do boot seguinte o descartava — a conquista
// sumia para sempre: o painel mostrava 0/35 enquanto o servidor tinha as 7.
//
// Roda em Node puro; ARCADIA_DATA_DIR aponta pra pasta temporária, definido
// ANTES do require (padrão dos outros testes da suíte).
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-achkey-"))
process.env.ARCADIA_DATA_DIR = DIR

// Appid fictício: não existe bin da Steam para ele, então o schema local vem
// dos itens já salvos (é o cenário dos jogos sem bin — repack/crackeado/loja).
const APPID = "999999001"

const conta = require("../electron/supabase/conta")
conta.definirConta("tester")

const { applyPulled } = require("../electron/supabase/sync")
const { loadAllSchemas } = require("../electron/achievements/loader")
const { buildIndex, findItem, normalizeTitle, aliasesOf } = require("../electron/achievements/match")
const { apinamesByPercent, parseAchievementsHtml } = require("../electron/achievements/steam_bin")

const ACH_FILE = path.join(DIR, "contas", "tester", "achievements.json")
const CONTA_DIR = path.join(DIR, "contas", "tester")

// game_settings.json vazio: evita o ENOENT que o loader loga por appid.
fs.mkdirSync(CONTA_DIR, { recursive: true })
fs.writeFileSync(path.join(CONTA_DIR, "game_settings.json"), "{}")

function item(apiname, title, block, extra = {}) {
  return {
    apiname,
    title,
    desc: "",
    block,
    bit: 0,
    icon: "",
    icongray: "",
    achieved: false,
    unlock: 0,
    percent: 0,
    ...extra,
  }
}

function seed(items) {
  fs.writeFileSync(ACH_FILE, JSON.stringify({ [APPID]: { at: Date.now(), items } }))
}

function lerItens() {
  return JSON.parse(fs.readFileSync(ACH_FILE, "utf8"))[APPID].items
}

test("conquista do servidor com apiname diferente casa por título e sobrevive ao reload", async () => {
  seed([
    item("ach_01", "Primeiros passos", 0),
    item("ach_02", "Pague e pinte", 1),
    item("ach_03", "Pronto para San Fierro", 2),
  ])

  const mudou = applyPulled([
    {
      appid: APPID,
      apiname: "ACH08",
      title: "Pague e pinte",
      unlocked_at: 1700000000,
      achieved: true,
      percent: 100,
    },
  ])
  assert.equal(mudou, true, "o pull deve alterar o arquivo")

  await loadAllSchemas()

  const itens = lerItens()
  assert.equal(itens.length, 3, "não pode duplicar nem perder item no reload")

  const alvo = itens.find((i) => i.title === "Pague e pinte")
  assert.ok(alvo, "a conquista continua no arquivo")
  assert.equal(alvo.achieved, true, "o desbloqueio vindo do servidor não pode ser apagado")
  assert.equal(alvo.unlock, 1700000000, "o timestamp do desbloqueio é preservado")
  assert.deepEqual(aliasesOf(alvo), ["ach_02", "ACH08"], "a chave do servidor vira apelido durável")
  assert.equal(alvo.remoteApiname, "ACH08", "o push seguinte usa a chave que o servidor conhece")
})

test("segundo pull do mesmo item é no-op (idempotente)", async () => {
  seed([item("ach_01", "Primeiros passos", 0), item("ach_02", "Pague e pinte", 1)])
  const linha = {
    appid: APPID,
    apiname: "ACH08",
    title: "Pague e pinte",
    unlocked_at: 1700000000,
    achieved: true,
  }

  assert.equal(applyPulled([linha]), true, "primeiro pull muda o arquivo")
  await loadAllSchemas()
  assert.equal(applyPulled([linha]), false, "re-pull do mesmo desbloqueio não muda nada")
  assert.equal(lerItens().filter((i) => i.title === "Pague e pinte").length, 1, "não duplica")
})

test("item do pull sem equivalente local é preservado, não descartado", async () => {
  seed([item("ach_01", "Primeiros passos", 0)])

  applyPulled([
    {
      appid: APPID,
      apiname: "ACH99",
      title: "Conquista de outra máquina",
      unlocked_at: 1700000001,
      achieved: true,
    },
  ])
  await loadAllSchemas()

  let itens = lerItens()
  assert.equal(itens.length, 2, "o item sem par local não pode ser apagado")
  const orfa = itens.find((i) => i.apiname === "ACH99")
  assert.ok(orfa, "o item do pull sobrevive ao reload")
  assert.equal(orfa.achieved, true)
  assert.equal(orfa.block, undefined, "continua sem block/bit, como veio do servidor")

  await loadAllSchemas()
  itens = lerItens()
  assert.equal(itens.length, 2, "sobrevive a reloads sucessivos sem acumular")
})

test("título repetido no mesmo appid não casa por título", () => {
  const itens = [item("a1", "Mesma coisa", 0), item("a2", "Mesma coisa", 1)]
  const indice = buildIndex(itens)
  assert.equal(
    findItem(indice, { apiname: "desconhecido", title: "Mesma coisa" }),
    null,
    "título ambíguo não pode marcar a conquista errada",
  )
  assert.equal(findItem(indice, { apiname: "a2" })?.apiname, "a2", "apiname exato continua funcionando")
})

test("normalizeTitle ignora acento, caixa e espaço extra", () => {
  assert.equal(normalizeTitle("  Pé de  Valsa "), normalizeTitle("Pe de valsa"))
  assert.equal(normalizeTitle(""), "")
})

test("apinamesByPercent recupera o apiname real pelo percentual", () => {
  const pagina = [
    { apiname: "", title: "Primeiros passos", percent: 80.8 },
    { apiname: "", title: "Pague e pinte", percent: 56.7 },
  ]
  const api = [
    { name: "ACH01", percent: 80.8 },
    { name: "ACH08", percent: 56.7 },
  ]
  assert.deepEqual(
    apinamesByPercent(pagina, api).map((i) => i.apiname),
    ["ACH01", "ACH08"],
  )
})

test("apinamesByPercent mantém o sintético quando o percentual é ambíguo", () => {
  const pagina = [
    { apiname: "", title: "A", percent: 9.5 },
    { apiname: "", title: "B", percent: 9.5 },
  ]
  const api = [
    { name: "ACH20", percent: 9.5 },
    { name: "ACH21", percent: 9.5 },
  ]
  assert.deepEqual(
    apinamesByPercent(pagina, api).map((i) => i.apiname),
    ["ach_01", "ach_02"],
  )
})

test("apinamesByPercent não mexe em quem já tem apiname", () => {
  const pagina = [{ apiname: "JA_TEM", title: "A", percent: 80.8 }]
  assert.equal(apinamesByPercent(pagina, [{ name: "ACH01", percent: 80.8 }])[0].apiname, "JA_TEM")
})

test("parseAchievementsHtml extrai título, descrição e percentual", () => {
  const html =
    '<div class="achieveRow "><div class="achieveImgHolder">' +
    '<img src="http://cdn/i.jpg" width="64" /></div>' +
    '<div class="achieveTxtHolder"><div class="achievePercent">80.8%</div>' +
    '<div class="achieveTxt"><h3>Primeiros passos</h3>' +
    '<h5>Complete &quot;Big Smoke&quot;.</h5></div></div>' +
    '<div style="clear: both;"></div></div>'

  const itens = parseAchievementsHtml(html)
  assert.equal(itens.length, 1)
  assert.equal(itens[0].title, "Primeiros passos")
  assert.equal(itens[0].desc, 'Complete "Big Smoke".')
  assert.equal(itens[0].percent, 80.8)
  assert.equal(itens[0].apiname, "", "a página não expõe mais o apiname")
})
