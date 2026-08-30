"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-upc-"))
process.env.ARCADIA_DATA_DIR = DATA_DIR

const uplay = require("../electron/achievements/uplay")
const watcher = require("../electron/achievements/cracked_watcher")

function blackFlagItems() {
  return [
    {
      apiname: "ACObsidian_Ach_1",
      title: "Último a Sair",
      desc: "Sente-se em um navio em chamas",
      block: 1,
      bit: 0,
    },
    {
      apiname: "ACObsidian_Ach_40",
      title: "Artesanato de Primeira",
      desc: "Crie um item",
      block: 2,
      bit: 7,
    },
  ]
}

function fullBlackFlagItems() {
  return Array.from({ length: 49 }, (_, index) => {
    const id = index + 1
    return {
      apiname: `ACObsidian_Ach_${id}`,
      title: id === 40 ? "Artesanato de Primeira" : `Conquista ${id}`,
      desc: id === 40 ? "Crie um item" : `Descrição ${id}`,
      block: id <= 32 ? 1 : 2,
      bit: id <= 32 ? id - 1 : id - 33,
    }
  })
}

test("parseUPC lê o runtime como mapa numérico e ignora earned=0", () => {
  const found = uplay.parseUPC(
    JSON.stringify({
      1: { displayName: "One", description: "D1", earned: 0 },
      2: { displayName: "Two", description: "D2", earned: true, earned_time: 1788096412 },
      3: { displayName: "Three", description: "D3", earned: 1, earned_time: 1788096413000 },
      4: { displayName: "Four", description: "D4" },
    }),
  )
  assert.deepEqual(found, [
    { id: "2", name: "Two", unlockTime: 1788096412000 },
    { id: "3", name: "Three", unlockTime: 1788096413000 },
  ])
})

test("parseUPC rejeita schema/array e JSON inválido como runtime desbloqueado", () => {
  assert.deepEqual(uplay.parseUPC("[]"), [])
  assert.deepEqual(uplay.parseUPC("not-json"), [])
  assert.deepEqual(uplay.parseUPC(JSON.stringify({ 1: { earned: 0 } })), [])
})

test("buildUplaySchema gera IDs decimais do Black Flag sem prefixo", () => {
  const schema = uplay.buildUplaySchema(blackFlagItems())
  assert.deepEqual(schema, {
    1: { displayName: "Último a Sair", description: "Sente-se em um navio em chamas", earned: 0 },
    40: { displayName: "Artesanato de Primeira", description: "Crie um item", earned: 0 },
  })
  assert.equal(uplay.validateUplaySchema(schema).ok, true)
  assert.equal(uplay.validateUplaySchema(schema, { requireContiguous: true }).ok, false)
})

test("resolveUplayId usa configuração e mapeamento conhecido do Black Flag", () => {
  assert.equal(uplay.resolveUplayId("3751950"), "66088")
  assert.equal(uplay.resolveUplayId("999", { upcId: "1234" }), "1234")
  assert.equal(uplay.resolveUplayId("999", { uplayId: "../unsafe" }), null)
})

test("caminhosPrefixados inclui runtime UPC no prefixo informado", () => {
  const prefix = path.join(DATA_DIR, "prefix")
  const records = watcher.caminhosPrefixados(prefix, "3751950", { id: "steam:3751950" })
  const upc = records.find((record) => record.name === "upc" && record.uplayId === "66088")
  assert.ok(upc)
  assert.equal(
    upc.file,
    path.join(
      prefix,
      "drive_c",
      "users",
      "steamuser",
      "AppData",
      "Roaming",
      "Goldberg UplayEmu Saves",
      "66088",
      "achievements.json",
    ),
  )
})

test("itemParaDesbloqueio converte UPC 40 em ACObsidian_Ach_40", () => {
  const item = watcher.itemParaDesbloqueio(
    blackFlagItems(),
    { id: "40", name: "Artesanato de Primeira" },
    { name: "upc" },
  )
  assert.equal(item.apiname, "ACObsidian_Ach_40")
  const payload = watcher.payloadParaDesbloqueio(
    "3751950",
    item,
    { id: "40", unlockTime: 1788096412000 },
    { name: "upc" },
  )
  assert.equal(payload.key, "2|7")
  assert.equal(payload.apiname, "ACObsidian_Ach_40")
  assert.equal(payload.unlock, 1788096412)
})

test("setIniSetting insere a chave na seção Settings quando ela não existe", () => {
  const updated = uplay.setIniSetting("[Settings]\nLogging = 1\n", "Achievements", "1")
  assert.equal(updated, "[Settings]\nAchievements = 1\nLogging = 1\n")
})

test("prepareUplayInstallation cria schema e ativa Achievements com backup somente no jogo de teste", () => {
  const gameDir = path.join(DATA_DIR, "fake-game")
  fs.mkdirSync(gameDir, { recursive: true })
  fs.writeFileSync(path.join(gameDir, "upc_r2_loader64.dll"), "fake")
  fs.writeFileSync(path.join(gameDir, "upc_r2.ini"), "[Settings]\nAchievements = 0\nLogging = 1\n")
  const result = uplay.prepareUplayInstallation({
    gameDir,
    appid: "3751950",
    items: fullBlackFlagItems(),
  })
  assert.equal(result.ok, true)
  assert.equal(result.schema, "created")
  assert.equal(result.changed, true)
  assert.equal(result.backupPaths.length, 1)
  assert.equal(
    JSON.parse(fs.readFileSync(path.join(gameDir, "achievements_schema.json"), "utf8"))["40"]
      .earned,
    0,
  )
  assert.match(fs.readFileSync(path.join(gameDir, "upc_r2.ini"), "utf8"), /Achievements = 1/)
  assert.ok(fs.existsSync(result.backupPaths[0]))
  assert.equal(fs.existsSync(path.join(gameDir, "achievements.json")), false)
})

test("prepareUplayInstallation não sobrescreve schema existente inválido", () => {
  const gameDir = path.join(DATA_DIR, "invalid-schema-game")
  fs.mkdirSync(gameDir, { recursive: true })
  fs.writeFileSync(path.join(gameDir, "upc_r2_loader64.dll"), "fake")
  fs.writeFileSync(path.join(gameDir, "upc_r2.ini"), "[Settings]\nAchievements = 1\n")
  const schemaPath = path.join(gameDir, "achievements_schema.json")
  fs.writeFileSync(schemaPath, "[]")
  const result = uplay.prepareUplayInstallation({
    gameDir,
    appid: "3751950",
    items: blackFlagItems(),
  })
  assert.equal(result.ok, false)
  assert.equal(result.requiresConfirmation, true)
  assert.equal(fs.readFileSync(schemaPath, "utf8"), "[]")
})

test("watcher detecta UPC no prefixo e entrega a conquista ao callback", () => {
  const prefix = path.join(DATA_DIR, "watch-prefix")
  const runtime = uplay.uplayRuntimePath(prefix, "66088")
  fs.mkdirSync(path.dirname(runtime), { recursive: true })
  fs.writeFileSync(
    runtime,
    JSON.stringify({
      40: { displayName: "Artesanato de Primeira", earned: 1, earned_time: 1788096412 },
    }),
  )

  const achievementFile = path.join(DATA_DIR, "achievements.json")
  fs.writeFileSync(
    achievementFile,
    JSON.stringify({
      3751950: {
        items: blackFlagItems().map((item) => ({
          ...item,
          achieved: false,
          unlock: 0,
          percent: 0,
        })),
      },
    }),
  )
  fs.writeFileSync(
    path.join(DATA_DIR, "game_settings.json"),
    JSON.stringify({
      "steam:3751950": {
        prefixPath: prefix,
        uplayId: "66088",
      },
    }),
  )
  fs.writeFileSync(path.join(DATA_DIR, "library.json"), JSON.stringify({ version: 1, games: [] }))

  const seen = []
  const stop = watcher.iniciarVigia((payload) => seen.push(payload))
  try {
    assert.equal(seen.length, 1)
    assert.equal(seen[0].key, "2|7")
    assert.equal(seen[0].provider, "upc")
    assert.equal(seen[0].apiname, "ACObsidian_Ach_40")
    const saved = JSON.parse(fs.readFileSync(achievementFile, "utf8"))
    assert.equal(saved["3751950"].items[1].achieved, true)
    assert.equal(saved["3751950"].items[1].unlock, 1788096412)
  } finally {
    stop()
  }
})
