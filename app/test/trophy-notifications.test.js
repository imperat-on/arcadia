const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const vm = require("node:vm")
const path = require("node:path")
const { trophySets, platinumForGame } = require("../electron/achievements/platinum")

const main = fs.readFileSync(path.join(__dirname, "../electron/main.js"), "utf8")
const code = main.slice(main.indexOf("const JANELA_TOAST_S ="), main.indexOf("// Callback de revogo"))

test("preferência de som pode ser gravada pelo IPC de configuração", () => {
  const allowlist = main.split("const ALLOWED_CONFIG = new Set([")[1].split("])")[0]
  assert.match(allowlist, /"achievement_sound"/)
})

function setup({ running = false, focused = true, config = {}, reducedMotion = false, appid = "570", items, platinumCelebrated } = {}) {
  const notices = [], events = []
  const now = 1_800_000_000
  let disk = JSON.stringify({ [appid]: { platinumCelebrated, items: items || [{ apiname: "A", block: 0, bit: 0, title: "Campeão", percent: 2.5 }] } })
  const unlock = vm.runInNewContext(`${code}\nonUnlockAchievement`, {
    path, DATA_DIR: "/test", caminhoConta: file => file,
    fs: { readFileSync: () => disk, writeFileSync: (_file, value) => { disk = value } },
    require: id => id === "./debug" ? { log() {} } : { enqueue() {}, scheduleNow() {} },
    aliasesOf: item => [item.apiname],
    platinumForGame,
    win: { isDestroyed: () => false, isVisible: () => true, isMinimized: () => false, webContents: { send: (...args) => events.push(args) } },
    jogoRodando: running, jogoAtivo: null, lancamentoAtual: null, focado: focused,
    readLibrary: () => [{ id: "steam:570", title: "Jogo teste" }], readConfig: () => config,
    systemPreferences: { getAnimationSettings: () => ({ prefersReducedMotion: reducedMotion }) },
    showAchievementToast: (...args) => notices.push(args),
    Date: { now: () => now * 1000 },
  })
  return { now, notices, events, unlock: (age = 0, options) => unlock({ appid, key: "0|0", title: "Campeão", unlock: now - age }, options), state: () => JSON.parse(disk)[appid].items[0] }
}

test("último troféu base promove o mesmo aviso nativo a platina sem contar DLC", () => {
  const appid = "268910"
  const items = trophySets[appid].base.map((apiname, block) => ({ apiname, block, bit: 0, achieved: block !== 0, unlock: block ? 1700000000 : 0 }))
  items.push({ apiname: trophySets[appid].dlc[0], achieved: false })
  const s = setup({ appid, items, running: true })
  s.unlock(); s.unlock()
  assert.equal(s.events.length, 1)
  assert.equal(s.events[0][1].platinum, true)
  assert.equal(s.notices.length, 1)
  assert.equal(s.notices[0][1].platinum, true)
})

test("uma platina já celebrada não repete o aviso especial", () => {
  const appid = "221910"
  const items = trophySets[appid].base.map((apiname, block) => ({ apiname, block, bit: 0, achieved: block > 0 }))
  const s = setup({ appid, items, platinumCelebrated: true })
  s.unlock()
  assert.equal(s.events[0][1].notice, "launcher")
  assert.equal(s.events[0][1].platinum, false)
})

test("launcher focado mostra um aviso interno por conquista, mesmo com detecção repetida", () => {
  const s = setup()
  s.unlock(); s.unlock()
  assert.equal(s.notices.length, 0)
  assert.equal(s.events.length, 1)
  assert.equal(s.events[0][0], "achievement:unlocked")
  const [, payload] = s.events[0]
  assert.equal(payload.notice, "launcher")
  assert.equal(payload.gameTitle, "Jogo teste")
  assert.equal(payload.apiname, "A")
  assert.equal(payload.rare, true)
})

test("jogo rodando ou launcher desfocado escolhe a janela nativa sem pedir toast interno", () => {
  for (const options of [{ running: true }, { focused: false }]) {
    const s = setup({ ...options, config: { achievement_sound: false, no_anim: true } })
    s.unlock()
    assert.equal(s.notices.length, 1)
    assert.equal(s.events.length, 1)
    assert.equal(s.events[0][1].notice, "native")
    assert.equal(s.notices[0][0].sound, false)
    assert.equal(s.notices[0][0].reducedMotion, true)
    assert.notEqual(s.notices[0][1]?.platinum, true, "uma lista sem base confirmada não é platina")
  }
})

test("notificação respeita movimento reduzido do sistema", () => {
  const s = setup({ reducedMotion: true })
  s.unlock()
  assert.equal(s.events[0][1].reducedMotion, true)
})

test("histórico, timestamps no futuro e importações manuais gravam sem celebrar", () => {
  for (const [age, options] of [[301], [-1], [0, { silent: true }]]) {
    const s = setup()
    s.unlock(age, options)
    assert.equal(s.state().achieved, true)
    assert.equal(s.notices.length, 0)
    assert.equal(s.events.length, 1, "o painel ainda recebe histórico sem toast")
    assert.equal(s.events[0][1].notice, null)
  }
})

test("HTML nativo não tenta tocar áudio com som desligado e mantém o visual raro", () => {
  const html = fs.readFileSync(path.join(__dirname, "../public/notify.html"), "utf8")
  const script = html.match(/<script>([\s\S]*?)<\/script>/)[1]
  const classes = new Set()
  const nodes = new Map()
  const audio = []
  const document = { getElementById(id) {
    if (!nodes.has(id)) nodes.set(id, { style: {}, classList: { add: value => classes.add(value) } })
    return nodes.get(id)
  } }
  const run = sound => vm.runInNewContext(script, {
    URLSearchParams, document, location: { search: `?variant=rare&sound=${sound}&reducedMotion=1&desc=Meu+jogo` },
    setTimeout() {}, Audio: class { constructor(src) { audio.push(src) } play() { return Promise.resolve() } },
  })
  run("off")
  assert.deepEqual(audio, [])
  assert.equal(classes.has("rare"), true)
  assert.equal(classes.has("still"), true)
  assert.equal(nodes.get("desc").textContent, "Meu jogo")
  assert.equal(nodes.get("toast").style.visibility, "visible")
  run("unlock")
  assert.deepEqual(audio, ["sounds/unlock.wav"])
})

test("captura FLT usa data do arquivo, não a data da varredura", t => {
  const { parseFLT } = require("../electron/achievements/cracked_watcher")
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-flt-"))
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }))
  const file = path.join(dir, "ACH_TEST")
  fs.writeFileSync(file, "")
  fs.utimesSync(file, 1700000000, 1700000000)
  fs.mkdirSync(path.join(dir, "not-an-achievement"))
  assert.deepEqual(parseFLT(dir), [{ name: "ACH_TEST", unlockTime: 1700000000000 }])
})
