"use strict"

// Desbloqueio de conquista por CRACKER, um caso por formato que o vigia declara
// suportar (cracked_watcher.js, topo do arquivo).
//
// Por que existe: o usuário só tinha validado em jogo real o voices38/UPC (Black
// Flag) e o CODEX — o resto dos formatos nunca tinha sido exercitado ponta a
// ponta. Esta varredura escreve o arquivo de progresso no caminho EXATO que o
// vigia procura, roda o vigia de verdade (não o parser isolado) e exige o
// contrato completo do callback: um aviso, apiname certo, carimbo de tempo do
// arquivo e estado persistido no achievements.json — com a conquista vizinha
// intacta (marcar a errada é pior do que não marcar).
//
// Dois defeitos reais foram encontrados por esta varredura e ficam guardados
// aqui como regressão:
//   1. RLD! (State/Time em hex): o parser lia `Buffer.buffer` sem byteOffset e o
//      slab do Node devolvia lixo — State=1 virava zero e NENHUM desbloqueio RLD
//      era ingerido (arquivo com 1 desbloqueio devolvia []).
//   2. FLT, SteamData e 3DMGAME montavam o payload inline, sem `apiname`/
//      `provider` (o toast casava pelo key, mas o log e qualquer consumidor novo
//      liam campos vazios). Agora os três usam o mesmo construtor canônico.
//
// Isolamento: cada caso roda em processo próprio (node:test compartilha o
// processo entre testes do mesmo arquivo e o DATA_DIR do módulo de conta é
// capturado no require). O filho é este mesmo arquivo com ARCADIA_CRACKER_CASE
// definido.

const test = require("node:test")
const assert = require("node:assert/strict")
const { execFileSync } = require("node:child_process")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const RAIZ_APP = path.join(__dirname, "..")
const CASOS = [
  "goldberg_gse",
  "goldberg_steamemu",
  "emulator_schema",
  "codex",
  "rune",
  "skidrow_docs",
  "skidrow_player",
  "skidrow_local",
  "empress_roaming",
  "empress_public",
  "onlinefix_stats",
  "onlinefix_direct",
  "creamapi",
  "smartsteamemu",
  "razor1911",
  "rld_rld",
  "rld_player",
  "rld_steam",
  "flt",
  "steamdata",
  "game3dm",
  "upc",
]

// ---------------------------------------------------------------------------
// Modo runner (processo filho): um caso, o vigia de verdade, exit code.
// ---------------------------------------------------------------------------
function rodarCaso() {
  const CASO = process.env.ARCADIA_CRACKER_CASE
  const APPID = process.env.ARCADIA_CRACKER_APPID || "880001"

  const raiz = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-cracker-")))
  const dados = path.join(raiz, "dados")
  const prefixo = path.join(raiz, "prefix")
  const steam = path.join(raiz, "steam")
  const jogo = path.join(raiz, "jogo")
  const u = path.join(prefixo, "drive_c", "users", "steamuser")
  const appdata = path.join(u, "AppData", "Roaming")
  const local = path.join(u, "AppData", "Local")
  const userDocs = path.join(u, "Documents")
  const publicDocs = path.join(prefixo, "drive_c", "users", "Public", "Documents")
  const programData = path.join(prefixo, "drive_c", "ProgramData")

  for (const d of [path.join(dados, "contas", "zes"), appdata, local, userDocs, publicDocs, programData, steam, jogo]) {
    fs.mkdirSync(d, { recursive: true })
  }
  // Sem loginusers na Steam falsa => "sem Steam" => captura liberada.
  fs.writeFileSync(path.join(dados, "config.json"), JSON.stringify({ achievements_auto_capture: true }))
  fs.writeFileSync(path.join(dados, "contas", ".migrado"), "1")
  fs.writeFileSync(
    path.join(dados, "contas", "zes", "game_settings.json"),
    JSON.stringify({
      ["steam:" + APPID]: { exePath: path.join(jogo, "Game.exe"), prefixPath: prefixo, uplayId: "777" },
    }),
  )
  fs.writeFileSync(
    path.join(dados, "contas", "zes", "achievements.json"),
    JSON.stringify({
      [APPID]: {
        items: [
          { apiname: "ACH01", title: "Primeiros passos", desc: "d1", block: 1, bit: 0, achieved: false, unlock: 0, percent: 0 },
          { apiname: "ACH02", title: "Com molho extra", desc: "d2", block: 1, bit: 1, achieved: false, unlock: 0, percent: 0 },
        ],
      },
    }),
  )
  fs.writeFileSync(
    path.join(dados, "library.json"),
    JSON.stringify({ version: 1, games: [{ id: "steam:" + APPID, installed: true, exe: path.join(jogo, "Game.exe") }] }),
  )

  process.env.ARCADIA_DATA_DIR = dados
  process.env.HOME = raiz
  process.env.STEAM_DIR = steam
  process.env.APPDATA = path.join(raiz, "home", "AppData", "Roaming")
  fs.mkdirSync(process.env.APPDATA, { recursive: true })

  const T = 1700000000
  const hexLE = (n) => {
    const b = Buffer.alloc(4)
    b.writeUInt32LE(n, 0)
    return b.toString("hex").toUpperCase()
  }

  const CASES = {
    goldberg_gse: {
      provider: "goldberg",
      arquivo: path.join(appdata, "GSE Saves", APPID, "achievements.json"),
      conteudo: JSON.stringify({ ACH01: { earned: true, earned_time: T }, ACH02: { earned: false, earned_time: 0 } }),
    },
    goldberg_steamemu: {
      provider: "goldberg",
      arquivo: path.join(appdata, "Goldberg SteamEmu Saves", APPID, "achievements.json"),
      conteudo: JSON.stringify([{ name: "ACH01", earned: true, earned_time: T }, { name: "ACH02", earned: 0, earned_time: 0 }]),
    },
    // Emulador com schema próprio: o arquivo de progresso fala o nome REAL e o
    // item local tem a chave sintética — a ponte é o título do steam_settings.
    emulator_schema: {
      provider: "goldberg",
      arquivo: path.join(appdata, "GSE Saves", APPID, "achievements.json"),
      conteudo: JSON.stringify({ ACH19: { earned: true, earned_time: T } }),
      schema: [
        { name: "ACH19", displayName: { brazilian: "Tira meu gatinho da árvore?", english: "Cat in a Tree" } },
      ],
      itens: [{ apiname: "ach_15", title: "Tira meu gatinho da árvore?", block: 14, bit: 0, achieved: false, unlock: 0, percent: 0 }],
      esperaApiname: "ach_15",
    },
    codex: {
      provider: "codex",
      arquivo: path.join(publicDocs, "Steam", "CODEX", APPID, "achievements.ini"),
      conteudo: "[ACH01]\nAchieved=1\nUnlockTime=" + T + "\n[ACH02]\nAchieved=0\nUnlockTime=0\n",
    },
    rune: {
      provider: "rune",
      arquivo: path.join(publicDocs, "Steam", "RUNE", APPID, "achievements.ini"),
      conteudo: "[ACH01]\nAchieved=1\nTimeUnlocked=" + T + "\n[ACH02]\nAchieved=0\n",
    },
    skidrow_docs: {
      provider: "skidrow",
      arquivo: path.join(userDocs, "SKIDROW", APPID, "SteamEmu", "UserStats", "achiev.ini"),
      conteudo: "[Achievements]\nACH01=1@ACH01@" + T + "\nACH02=0@ACH02@0\n",
    },
    skidrow_player: {
      provider: "skidrow",
      arquivo: path.join(userDocs, "Player", APPID, "SteamEmu", "UserStats", "achiev.ini"),
      conteudo: "[Achievements]\nACH01=1@ACH01@" + T + "\n",
    },
    skidrow_local: {
      provider: "skidrow",
      arquivo: path.join(local, "SKIDROW", APPID, "SteamEmu", "UserStats", "achiev.ini"),
      conteudo: "[Achievements]\nACH01=1@ACH01@" + T + "\n",
    },
    empress_roaming: {
      provider: "empress",
      arquivo: path.join(appdata, "EMPRESS", "remote", APPID, "achievements.json"),
      conteudo: JSON.stringify({ ACH01: { earned: true, earned_time: T } }),
    },
    empress_public: {
      provider: "empress",
      arquivo: path.join(publicDocs, "EMPRESS", APPID, "remote", APPID, "achievements.json"),
      conteudo: JSON.stringify({ ACH01: { earned: true, earned_time: T } }),
    },
    onlinefix_stats: {
      provider: "onlinefix",
      arquivo: path.join(publicDocs, "OnlineFix", APPID, "Stats", "Achievements.ini"),
      conteudo: "[ACH01]\nAchieved=true\nTimeUnlocked=" + T + "\n[ACH02]\nAchieved=false\n",
    },
    onlinefix_direct: {
      provider: "onlinefix",
      arquivo: path.join(publicDocs, "OnlineFix", APPID, "Achievements.ini"),
      conteudo: "[ACH01]\nachieved=true\ntimestamp=" + T + "\n",
    },
    creamapi: {
      provider: "creamapi",
      arquivo: path.join(appdata, "CreamAPI", APPID, "stats", "CreamAPI.Achievements.cfg"),
      conteudo: "[ACH01]\nachieved=true\nunlocktime=" + T + "\n[ACH02]\nachieved=false\n",
    },
    smartsteamemu: {
      provider: "sse",
      arquivo: path.join(appdata, "SmartSteamEmu", APPID, "User", "Achievements.ini"),
      conteudo: "[ACH01]\nachieved=true\ntimestamp=" + T + "\n",
    },
    razor1911: {
      provider: "razor",
      arquivo: path.join(appdata, ".1911", APPID, "achievement"),
      conteudo: "ACH01 1 " + T + "\nACH02 0 0\n",
    },
    // Regressão do slab: sem o readUInt32LE isto devolvia [] (State=1 virava 0).
    rld_rld: {
      provider: "rld",
      arquivo: path.join(programData, "RLD!", APPID, "achievements.ini"),
      conteudo: "[ACH01]\nState=" + hexLE(1) + "\nTime=" + hexLE(T) + "\n[ACH02]\nState=" + hexLE(0) + "\nTime=" + hexLE(0) + "\n",
    },
    rld_player: {
      provider: "rld",
      arquivo: path.join(programData, "Steam", "Player", APPID, "stats", "achievements.ini"),
      conteudo: "[ACH01]\nState=" + hexLE(1) + "\nTime=" + hexLE(T) + "\n[Steam]\nState=" + hexLE(1) + "\n",
    },
    rld_steam: {
      provider: "rld",
      arquivo: path.join(programData, "Steam", "RLD!", APPID, "stats", "achievements.ini"),
      conteudo: "[ACH01]\nState=" + hexLE(1) + "\nTime=" + hexLE(T) + "\n",
    },
    // FLT não tem carimbo no arquivo: o unlock é o instante da detecção.
    flt: { provider: "flt", pasta: path.join(appdata, "FLT", APPID), unlockAgora: true },
    steamdata: {
      provider: "steamdata",
      arquivo: path.join(jogo, "SteamData", "user_stats.ini"),
      conteudo: '[ACHIEVEMENTS]\nACH01={"unlocked = true, time = ' + T + '"}\nACH02={"unlocked = false, time = 0"}\n',
    },
    game3dm: {
      provider: "3dmgame",
      arquivo: path.join(jogo, "3DMGAME", "Player", "stats", "achievements.ini"),
      conteudo: "[ACH01]\nAchieved=1\nUnlockTime=" + T + "\n",
    },
    // Controle: o formato que o usuário já validou em jogo (voices38/UPC).
    upc: {
      provider: "upc",
      arquivo: path.join(appdata, "Goldberg UplayEmu Saves", "777", "achievements.json"),
      conteudo: JSON.stringify({ 1: { displayName: "Primeiros passos", earned: 1, earned_time: T } }),
    },
  }

  const caso = CASES[CASO]
  const falhar = (motivo) => {
    console.log("FAIL\t" + CASO + "\t" + motivo)
    process.exit(1)
  }
  if (!caso) falhar("caso desconhecido")

  if (caso.itens) {
    fs.writeFileSync(
      path.join(dados, "contas", "zes", "achievements.json"),
      JSON.stringify({ [APPID]: { items: caso.itens } }),
    )
  }
  if (caso.schema) {
    fs.mkdirSync(path.join(jogo, "steam_settings"), { recursive: true })
    fs.writeFileSync(path.join(jogo, "steam_settings", "achievements.json"), JSON.stringify(caso.schema))
  }
  if (caso.pasta) {
    fs.mkdirSync(caso.pasta, { recursive: true })
    fs.writeFileSync(path.join(caso.pasta, "ACH01"), "")
  } else {
    fs.mkdirSync(path.dirname(caso.arquivo), { recursive: true })
    fs.writeFileSync(caso.arquivo, caso.conteudo)
  }

  const conta = require(path.join(RAIZ_APP, "electron", "supabase", "conta.js"))
  conta.definirConta("zes")
  const cw = require(path.join(RAIZ_APP, "electron", "achievements", "cracked_watcher.js"))

  const avisos = []
  const parar = cw.iniciarVigia((p) => avisos.push(p))
  const esperar = (cond, ms) =>
    new Promise((resolve) => {
      const t0 = Date.now()
      const tick = setInterval(() => {
        if (cond() || Date.now() - t0 > ms) {
          clearInterval(tick)
          resolve()
        }
      }, 25)
    })

  const apinameEsperado = caso.esperaApiname || "ACH01"

  ;(async () => {
    await esperar(() => avisos.length > 0, 2000)
    let erro = null
    try {
      assert.equal(avisos.length, 1, "esperava 1 aviso, veio " + avisos.length)
      assert.equal(avisos[0].apiname, apinameEsperado, "apiname errado: " + avisos[0].apiname)
      assert.equal(avisos[0].provider, caso.provider, "provider errado: " + avisos[0].provider)
      assert.ok(avisos[0].unlock > 0, "sem carimbo de tempo")
      if (caso.unlockAgora) {
        assert.ok(Math.abs(Math.floor(Date.now() / 1000) - avisos[0].unlock) <= 5, "FLT usa o instante da detecção")
      } else {
        assert.equal(avisos[0].unlock, T, "carimbo do arquivo deveria chegar inteiro")
      }
      const salvo = JSON.parse(
        fs.readFileSync(path.join(dados, "contas", "zes", "achievements.json"), "utf8"),
      )[APPID].items
      const alvo = salvo.find((i) => i.apiname === apinameEsperado)
      assert.equal(alvo.achieved, true, "não persistiu o desbloqueio")
      assert.equal(alvo.unlock, avisos[0].unlock, "o timestamp salvo diverge do aviso")
      for (const outro of salvo.filter((i) => i.apiname !== apinameEsperado)) {
        assert.equal(outro.achieved, false, "marcou a conquista errada: " + outro.apiname)
      }
    } catch (e) {
      erro = e.message
    } finally {
      parar()
      await new Promise((r) => setTimeout(r, 120))
      try {
        fs.rmSync(raiz, { recursive: true, force: true })
      } catch {}
    }
    if (erro) falhar(erro)
    console.log("PASS\t" + CASO + "\t" + avisos[0].apiname + " @ " + avisos[0].unlock + " provider=" + avisos[0].provider)
  })()
}

// ---------------------------------------------------------------------------
// Modo suite (processo pai): um teste por caso, em processo próprio.
// ---------------------------------------------------------------------------
if (process.env.ARCADIA_CRACKER_CASE) {
  rodarCaso()
} else {
  for (const caso of CASOS) {
    test(`desbloqueio de conquista via ${caso}`, () => {
      const saida = execFileSync(process.execPath, [__filename], {
        env: { ...process.env, ARCADIA_CRACKER_CASE: caso },
        encoding: "utf8",
        timeout: 60000,
      })
      // O vigia loga a criação dos observadores no stdout antes da linha do
      // resultado, então a checagem é por linha (m), não pela string inteira.
      assert.doesNotMatch(saida, /^FAIL\t/m, `o caso ${caso} falhou: ${saida}`)
      assert.match(saida, /^PASS\t/m, `o caso ${caso} não passou: ${saida}`)
    })
  }
}
