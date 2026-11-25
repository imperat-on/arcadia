const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const fixture = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-trophy-"))
process.env.ARCADIA_DATA_DIR = path.join(fixture, "arcadia")
process.env.STEAM_DIR = path.join(fixture, "steam")
fs.mkdirSync(process.env.STEAM_DIR)
const { getTrophySummary } = require("../electron/achievements/summary")
const { loadAllSchemas, STATS_DIR } = require("../electron/achievements/loader")
const { saveAchievements, loadAchievements } = require("../electron/achievements/schema")

const locked = { apiname: "A", title: "First trophy", achieved: false }
const earned = { apiname: "B", title: "Second trophy", achieved: true }

function get(entry, catalog = async () => []) {
  return getTrophySummary("570", { readEntry: () => entry, fetchCatalog: catalog })
}

test("catálogo público não é evidência de progresso zero", async () => {
  const r = await get(undefined, async () => [locked])
  assert.deepEqual(r, { items: [locked], catalog: "known", progress: "unknown", done: 0, total: 1, percent: null, stale: false })
})

test("zero exige catálogo confirmado e leitura válida", async () => {
  const r = await get({ items: [locked], trophyCatalog: "confirmed", trophyProgress: "steam" })
  assert.equal(r.progress, "detected")
  assert.equal(r.percent, 0)
})

test("lista local legada não promete conclusão, mesmo com tudo desbloqueado", async () => {
  const r = await get({ items: [earned] })
  assert.equal(r.catalog, "partial")
  assert.equal(r.progress, "detected")
  assert.equal(r.percent, null)
})

test("lista local confirmada calcula progresso sem baixar catálogo", async () => {
  const r = await get({ items: [locked, earned], trophyCatalog: "confirmed" }, async () => assert.fail("não consulta rede"))
  assert.equal(r.percent, 50)
})

test("catálogo confirmado vazio é diferente de resposta pública vazia", async () => {
  assert.equal((await get({ items: [], trophyCatalog: "confirmed" })).catalog, "empty")
  assert.equal((await get()).catalog, "unknown")
})

test("falha de leitura não é catálogo vazio", async () => {
  const r = await getTrophySummary("570", { readEntry() { throw new Error("unreadable") }, fetchCatalog: async () => [locked] })
  assert.equal(r.catalog, "known")
  assert.equal(r.progress, "unknown")
  assert.equal(r.stale, true)
})

test("progresso conhecido sobrevive a falha de leitura da fonte", async () => {
  const r = await get({ items: [locked, earned], trophyCatalog: "confirmed", trophyProgress: "steam", trophyProgressStale: true })
  assert.equal(r.percent, 50)
  assert.equal(r.stale, true)
})

test("troca de conta invalida leitura pendente", async () => {
  let account = "old"
  const r = await getTrophySummary("570", { accountKey: () => account, readEntry: () => undefined, fetchCatalog: async () => { account = "new"; return [locked] } })
  assert.equal(r, null)
})

test("appid inválido não consulta arquivo ou rede", async () => {
  const fail = () => assert.fail("não deve consultar")
  assert.equal(await getTrophySummary("../570", { readEntry: fail, fetchCatalog: fail }), null)
})

function kv(name, value) {
  const key = Buffer.from(`${name}\0`)
  if (typeof value === "string") return Buffer.concat([Buffer.from([1]), key, Buffer.from(`${value}\0`)])
  if (typeof value === "number") {
    const data = Buffer.alloc(4)
    data.writeInt32LE(value)
    return Buffer.concat([Buffer.from([2]), key, data])
  }
  return Buffer.concat([Buffer.from([0]), key, ...Object.entries(value).map(([k, v]) => kv(k, v)), Buffer.from([8])])
}

test("loader certifica esquema nativo e aceita zero apenas com bin válido da conta vinculada", async () => {
  fs.mkdirSync(STATS_DIR, { recursive: true })
  fs.mkdirSync(path.join(process.env.STEAM_DIR, "config"), { recursive: true })
  fs.mkdirSync(process.env.ARCADIA_DATA_DIR, { recursive: true })
  fs.writeFileSync(path.join(process.env.STEAM_DIR, "config", "loginusers.vdf"), '"users" { "76561197960265729" { "MostRecent" "1" "PersonaName" "Trophy Test" } }')
  fs.writeFileSync(path.join(process.env.ARCADIA_DATA_DIR, "game_settings.json"), "{}")
  saveAchievements({})

  const appid = "999999004"
  const bin = path.join(STATS_DIR, `UserGameStats_76561197960265729_${appid}.bin`)
  fs.writeFileSync(path.join(STATS_DIR, `UserGameStatsSchema_${appid}.bin`), kv(appid, {
    stats: { 0: { bits: { 0: { name: "A", display: { name: { english: "First trophy" }, hidden: 1 } } } } },
  }))
  const emptyApp = "999999005"
  fs.writeFileSync(path.join(STATS_DIR, `UserGameStatsSchema_${emptyApp}.bin`), kv(emptyApp, { stats: { 0: { type: "1" } } }))
  fs.writeFileSync(bin.replace("76561197960265729", "76561197960265730"), kv(appid, { 0: { data: 0 } }))
  await loadAllSchemas()
  assert.equal(loadAchievements()[appid].trophyCatalog, "confirmed")
  assert.equal(loadAchievements()[appid].items[0].hidden, true)
  assert.equal((await getTrophySummary(appid)).percent, null)
  assert.equal((await getTrophySummary(emptyApp)).catalog, "empty")

  fs.writeFileSync(bin, kv(appid, { 0: { data: 0 } }))
  await loadAllSchemas()
  assert.equal((await getTrophySummary(appid)).percent, 0)

  fs.writeFileSync(bin, "ilegível")
  await loadAllSchemas()
  const r = await getTrophySummary(appid)
  assert.equal(r.percent, 0)
  assert.equal(r.stale, true)
})

test.after(() => fs.rmSync(fixture, { recursive: true, force: true }))
