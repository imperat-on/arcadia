"use strict"

// O toast de conquista de jogo crackeado tem de sair NO ATO do desbloqueio, não no
// próximo ciclo do polling.
//
// Antes: o vigia só tinha polling de 15s — o usuário desbloqueava, o item marcava no
// painel e o aviso podia levar até 15s (média ~7s). Agora, no Windows, o fs.watch
// avisa e a varredura roda com debounce de 300ms (mesmo padrão do vigia de bin da
// Steam). O polling CONTINUA como rede de segurança, e no Linux é o único caminho
// (dentro do prefixo Wine o inotify não é confiável).
//
// A prova aqui é o TEMPO: o teste grava o arquivo do crack e exige o callback bem
// antes do polling — se alguém remover o watcher, isto falha.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const SEM_WATCH = process.platform !== "win32"
  ? "no Linux o vigia é polling-only (inotify não é confiável dentro do prefixo Wine)"
  : false

let cw = null
let APPID = ""

function preparar() {
  const raizBruta = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-instant-"))
  // Nome LONGO: o tmpdir() do Windows devolve nome curto 8.3 (C:\...\ADMINI~1\...) e o
  // libuv estoura uma assert NATIVA ao observar diretório com nome curto. O código do
  // vigia normaliza com realpath, mas o teste não pode depender só disso.
  let raiz = raizBruta
  try {
    raiz = fs.realpathSync.native(raizBruta)
  } catch {}
  const appdata = path.join(raiz, "roaming")
  const dados = path.join(raiz, "dados")
  const steam = path.join(raiz, "steam")
  const jogo = path.join(raiz, "jogo")
  for (const d of [appdata, dados, steam, jogo]) fs.mkdirSync(d, { recursive: true })

  // Sem loginusers na Steam falsa => "sem Steam" => captura liberada (não é troca de conta).
  fs.mkdirSync(path.join(dados, "contas", "zes"), { recursive: true })
  fs.writeFileSync(path.join(dados, "config.json"), JSON.stringify({}))
  fs.writeFileSync(path.join(dados, "contas", ".migrado"), "1")
  fs.writeFileSync(
    path.join(dados, "contas", "zes", "game_settings.json"),
    JSON.stringify({ [`steam:${APPID}`]: { exePath: path.join(jogo, "Game.exe") } }),
  )
  fs.writeFileSync(
    path.join(dados, "contas", "zes", "achievements.json"),
    JSON.stringify({
      [APPID]: {
        items: [
          { apiname: "ACH01", title: "Primeiros passos", desc: "d1", block: 1, bit: 0, achieved: false, unlock: 0 },
          { apiname: "ACH02", title: "Com molho extra", desc: "d2", block: 1, bit: 1, achieved: false, unlock: 0 },
        ],
      },
    }),
  )

  process.env.ARCADIA_DATA_DIR = dados
  process.env.APPDATA = appdata
  process.env.STEAM_DIR = steam
  process.env.HOME = raiz

  const gse = path.join(appdata, "GSE Saves", APPID)
  fs.mkdirSync(gse, { recursive: true })
  const arquivo = path.join(gse, "achievements.json")
  fs.writeFileSync(arquivo, JSON.stringify({ ACH01: { earned: false, earned_time: 0 }, ACH02: { earned: false, earned_time: 0 } }))

  return { raiz, arquivo }
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

test("o desbloqueio de jogo crackeado avisa NO ATO (fs.watch, não o polling)", { skip: SEM_WATCH }, async () => {
  APPID = "999000"
  const { raiz, arquivo } = preparar()
  cw = require(path.join(__dirname, "..", "electron", "achievements", "cracked_watcher.js"))
  require(path.join(__dirname, "..", "electron", "supabase", "conta.js")).definirConta("zes")

  const recebidos = []
  const parar = cw.iniciarVigia((p) => recebidos.push({ ...p, _em: Date.now() }))
  const t0 = Date.now()

  // O jogo escreve a conquista (é o que o Goldberg faz ao desbloquear).
  fs.writeFileSync(
    arquivo,
    JSON.stringify({
      ACH01: { earned: true, earned_time: Math.floor(Date.now() / 1000) },
      ACH02: { earned: false, earned_time: 0 },
    }),
  )

  await esperar(() => recebidos.length > 0, cw.INTERVALO_POLL)
  const demorou = (recebidos[0]?._em || Date.now()) - t0
  parar()

  assert.equal(recebidos.length, 1, "uma conquista = um aviso")
  assert.equal(recebidos[0].apiname, "ACH01")
  assert.equal(recebidos[0].title, "Primeiros passos")
  assert.ok(recebidos[0].unlock > 0, "veio com o carimbo do arquivo")
  assert.ok(
    demorou < cw.INTERVALO_POLL - 5000,
    `avisou em ${demorou}ms — tem de ser bem antes do polling (${cw.INTERVALO_POLL}ms)`,
  )

  // Os observadores fecham no parar(); apagar o diretório no mesmo instante deixa a
  // janela aberta para a assert nativa do libuv sobre um alvo que sumiu.
  await new Promise((r) => setTimeout(r, 250))
  try {
    fs.rmSync(raiz, { recursive: true, force: true })
  } catch {}
})

test("o polling continua no código como rede de segurança", () => {
  // Sem isto, um evento perdido (pasta de rede, arquivo criado antes do watcher
  // subir) deixaria a conquista invisível para sempre.
  const fonte = fs.readFileSync(path.join(__dirname, "..", "electron", "achievements", "cracked_watcher.js"), "utf8")
  assert.match(fonte, /setInterval\(scan, INTERVALO_POLL\)/, "o polling continua")
  const iBloco = fonte.indexOf("--- Observadores de diretório")
  const iWatch = fonte.indexOf("fs.watch(")
  assert.ok(iBloco > 0 && iWatch > iBloco, "o observador está no bloco de observadores")
  assert.match(
    fonte.slice(iBloco, iWatch),
    /process\.platform === "win32"/,
    "o observador é só do Windows (no Linux o vigia segue polling-only)",
  )
  assert.match(fonte, /clearInterval\(interval\)[\s\S]{0,200}o\.close\(\)/, "parar a vigia fecha os observadores")
})
