const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-platinum-"))
process.env.ARCADIA_DATA_DIR = dir
const { saveAchievements, loadAchievements } = require("../electron/achievements/schema")
const { definirConta } = require("../electron/supabase/conta")
const { applyPulled } = require("../electron/supabase/sync")
const { getTrophySummary } = require("../electron/achievements/summary")
const { evaluatePlatinum, trophySets } = require("../electron/achievements/platinum")
const { parseSteamAchievementData } = require("../electron/achievements/steam_bin")

const definition = { base: ["FIRST", "LAST"], dlc: ["EXTRA"] }
const item = (apiname, achieved = true, unlock = 1700000000, more = {}) => ({ apiname, title: apiname, achieved, unlock, ...more })

test("a lista base completa dá platina mesmo faltando DLC; DLC novo não remove o selo", () => {
  const items = [item("FIRST"), item("LAST", true, 1700001234), item("EXTRA", false), item("NEW", false)]
  assert.deepEqual(evaluatePlatinum(definition, items), {
    base: { done: 2, total: 2, known: 2 },
    dlc: { done: 0, total: 1, known: 1 },
    awarded: true,
    earnedAt: 1700001234,
  })
})

test("ausência, alias ambíguo e revogação nunca completam o jogo base", () => {
  for (const items of [
    [item("FIRST"), item("EXTRA")],
    [item("FIRST"), item("ALIAS_A", true, 1, { aliases: ["LAST"] }), item("ALIAS_B", true, 1, { aliases: ["LAST"] })],
    [item("FIRST", true, 1, { aliases: ["LAST"] })],
    [item("FIRST"), item("LAST", true, 1, { revoked: true })],
  ]) assert.equal(evaluatePlatinum(definition, items).awarded, false)
  assert.equal(evaluatePlatinum(undefined, [item("FIRST"), item("LAST")]), null)
})

test("histórico sem data ganha selo sem fabricar horário", () => {
  const result = evaluatePlatinum(definition, [item("FIRST"), item("synthetic", true, 0, { aliases: ["LAST"] })])
  assert.equal(result.awarded, true)
  assert.equal(result.earnedAt, null)
})

test("Cuphead tem jogo base e DLC separados pelo ID Steam, nunca pela posição", () => {
  const set = trophySets[268910]
  assert.equal(set.base.length, 28)
  assert.equal(set.dlc.length, 14)
  assert.equal(new Set([...set.base, ...set.dlc]).size, 42)
  assert.ok(set.dlc.includes("DefeatKOGGauntlet"))
  assert.ok(set.base.includes("ARankWorld4"))
})

test("catálogo público separa Cuphead em grupos sem creditar desbloqueios", async () => {
  const set = trophySets[268910]
  const r = await getTrophySummary("268910", {
    readEntry: () => undefined,
    fetchCatalog: async () => [...set.base, ...set.dlc].map(id => item(id, false)),
  })
  assert.equal(r.platinum.awarded, false)
  assert.equal(r.items.filter(it => it.trophyGroup === "base").length, 28)
  assert.equal(r.items.filter(it => it.trophyGroup === "dlc").length, 14)
})

test("Portal with RTX não herda desbloqueios de Portal", async () => {
  const portal = { items: trophySets[400].base.map(id => item(id)) }
  const rtx = { items: [item(trophySets[2012840].base[0])] }
  const readEntry = appid => ({ 400: portal, 2012840: rtx })[appid]
  assert.equal((await getTrophySummary("400", { readEntry })).platinum.awarded, true)
  const result = await getTrophySummary("2012840", { readEntry })
  assert.equal(result.platinum.base.total, 15)
  assert.equal(result.platinum.awarded, false)
})

test("catálogo público moderno entrega apinames reais e segredos sem adivinhar pelo percentual", () => {
  const group = { ispublic: true, archived: false, developeronly: false, achievements: [
    { api_name: "LAST", name: "Final", description: "Fim", hidden: true, icon_achieved: "abcdef.jpg" },
    { api_name: "REMOVED", name: "Old", archived: true },
  ] }
  const page = `<script type="application/json" id="valve-ssr-data">${JSON.stringify({ renderContext: { queryData: JSON.stringify({ queries: [{ state: { data: { appid: 400, groups: [group] } } }] }) } })}</script>`
  const catalog = parseSteamAchievementData(page, "400")
  assert.deepEqual(catalog.map(({ apiname, title, hidden }) => ({ apiname, title, hidden })), [{ apiname: "LAST", title: "Final", hidden: true }])
  assert.equal(parseSteamAchievementData(page, "999"), null)
})

test("pull reconstrói o selo silenciosamente em outra conta/dispositivo e revisão invalida o selo", async () => {
  const appid = "221910"
  definirConta("device-a")
  const base = trophySets[appid].base
  saveAchievements({ [appid]: { items: base.map(id => item(id)) } })
  assert.equal((await getTrophySummary(appid)).platinum.awarded, true)

  definirConta("device-b")
  saveAchievements({ [appid]: { items: [] } })
  const rows = base.map(apiname => ({ appid, apiname, unlocked_at: 1700000000, achieved: true }))
  assert.equal(applyPulled(rows), true)
  assert.equal((await getTrophySummary(appid)).platinum.awarded, true)
  assert.equal(applyPulled(rows), false)

  const store = loadAchievements()
  const last = store[appid].items.at(-1)
  Object.assign(last, { achieved: false, unlock: 0, revoked: true, revokedAt: 1700000010 })
  saveAchievements(store)
  assert.equal(applyPulled(rows), false, "um pull atrasado não ressuscita a linha revogada")
  assert.equal((await getTrophySummary(appid)).platinum.awarded, false)
  assert.equal(applyPulled([{ ...rows.at(-1), unlocked_at: 1700000020 }]), true)
  assert.equal((await getTrophySummary(appid)).platinum.awarded, true)

  definirConta("device-a")
  assert.equal((await getTrophySummary(appid)).platinum.awarded, true)
})

test.after(() => { definirConta(null); fs.rmSync(dir, { recursive: true, force: true }) })
