// Vigia de conquistas de jogos "Jack Sparrow" (crackeados/emulados).
// Polling a cada 15s nos arquivos de conquista dos crackers dentro do
// prefixo Wine de cada jogo. Formatos suportados:
//   Goldberg  → JSON  (earned / earned_time)
//   CODEX     → INI   (Achieved / UnlockTime)
//   RUNE      → INI   (mesmo do CODEX)
//   Skidrow   → INI   (Achievements/<name>=1@...@timestamp)
//   EMPRESS   → JSON  (mesmo do Goldberg)
//   FLT       → pasta com 1 arquivo por conquista
//   SteamData → INI   (user_stats.ini na pasta do executavel)
//   SmartSteamEmu → INI
//   OnlineFix → INI
//   CreamAPI  → INI
//   Razor1911 → txt (linhas: name 1 timestamp)
//   RLD!      → INI (hex State/Time)
// Formato: 1 arquivo com header ou [Seção] + chave=valor.
// Política: escuta o mtime pra detectar mudancas (sem fs.watch — confiavel
//           em Wine/Proton onde inotify nao pega arquivos dentro do prefixo).
// Integracao: mesmo callback onUnlockAchievement do steam_bin.js — toasts,
//             som e painel compartilhados.

const fs = require("fs")
const path = require("path")
const os = require("os")
const { loadAchievements, saveAchievements } = require("./schema")

const { findSteamDir } = require("./../steam-path")
const COMPATDATA = path.join(findSteamDir(), "steamapps", "compatdata")
const INTERVALO_POLL = 15000 // 15s

// --- Utilitários de parse ---

function parseINI(texto) {
  // Suporta arquivos COM e SEM seções [Section].
  // Linhas: key=value. Comentários: ; ou #. Ignora linhas vazias.
  const secoes = {}
  let secaoAtual = "__root__"
  for (const linha of texto.split(/\r?\n/)) {
    const l = linha.trim()
    if (!l || l.startsWith(";") || l.startsWith("#")) continue
    if (l.startsWith("[") && l.endsWith("]")) {
      secaoAtual = l.slice(1, -1)
      continue
    }
    const eq = l.indexOf("=")
    if (eq === -1) continue
    const chave = l.slice(0, eq).trim()
    const valor = l.slice(eq + 1).trim()
    const map = secoes[secaoAtual] || (secoes[secaoAtual] = {})
    map[chave] = valor
  }
  return secoes
}

function parseGoldbergJSON(texto) {
  try {
    const obj = JSON.parse(texto)
    // Goldberg: array de { name, earned, earned_time }
    return Array.isArray(obj) ? obj : Object.values(obj)
  } catch {
    return []
  }
}

function parseRazor1911(texto) {
  // Formato: ACH_NAME 1 1234567890 (uma por linha)
  // ACH_NAME 0 ...
  const ach = []
  for (const linha of texto.split(/\r?\n/)) {
    const partes = linha.trim().split(/\s+/)
    if (partes.length < 3) continue
    if (partes[1] === "1") {
      ach.push({ name: partes[0], unlockTime: Number(partes[2]) * 1000 })
    }
  }
  return ach
}

function parseFLT(filePath) {
  // FLT: uma pasta com 1 arquivo por conquista desbloqueada
  // O nome do arquivo = apiname da conquista
  try {
    return fs.readdirSync(filePath).map((nome) => ({
      name: nome,
      unlockTime: Date.now(),
    }))
  } catch {
    return []
  }
}

// --- Parsers por cracker: retornam { name, unlockTime }[] ---

function parseGoldberg(conteudo) {
  const items = parseGoldbergJSON(conteudo)
  return items
    .filter((it) => it && it.earned)
    .map((it) => ({
      name: it.name,
      unlockTime: (it.earned_time || 0) * 1000,
    }))
}

function parseCODEX(conteudo) {
  const secoes = parseINI(conteudo)
  const achievements = []
  for (const [secao, pares] of Object.entries(secoes)) {
    for (const [chave, valor] of Object.entries(pares)) {
      if (chave === "Achieved" && valor === "1") {
        const ts = Number(pares.UnlockTime || pares.TimeUnlocked || 0)
        achievements.push({
          name: secao,
          unlockTime: ts > 1e11 ? ts : ts * 1000, // segundos → ms
        })
      }
    }
  }
  return achievements
}

function parseSkidrow(conteudo) {
  const secoes = parseINI(conteudo)
  const map = secoes.Achievements || secoes.__root__ || {}
  const ach = []
  for (const [nome, val] of Object.entries(map)) {
    // Formato: "1@nome@timestamp" ou "1"
    const partes = String(val).split("@")
    if (partes[0] === "1") {
      ach.push({
        name: nome,
        unlockTime: (Number(partes[partes.length - 1]) || 0) * 1000,
      })
    }
  }
  return ach
}

function parseUserStats(conteudo) {
  const secoes = parseINI(conteudo)
  const data = secoes.ACHIEVEMENTS || secoes.achievements || {}
  const ach = []
  for (const [nome, val] of Object.entries(data)) {
    // Formato: {"unlocked = true, time = 12345"}
    const limpo = String(val).replace(/["{}]/g, "")
    const match = limpo.match(/unlocked\s*=\s*true.*?time\s*=\s*(\d+)/i)
    if (match) {
      ach.push({ name: nome, unlockTime: Number(match[1]) * 1000 })
    }
  }
  return ach
}

function parseOnlineFix(conteudo) {
  const secoes = parseINI(conteudo)
  const ach = []
  for (const [nome, pares] of Object.entries(secoes)) {
    if (pares.achieved === "true" || pares.Achieved === "true") {
      const ts = Number(pares.timestamp || pares.TimeUnlocked || 0)
      ach.push({
        name: nome,
        unlockTime: ts > 1e11 ? ts : ts * 1000,
      })
    }
  }
  return ach
}

function parseRLD(conteudo) {
  const secoes = parseINI(conteudo)
  const ach = []
  for (const [nome, pares] of Object.entries(secoes)) {
    if (nome === "Steam") continue
    if (!pares.State) continue
    try {
      const state = new DataView(
        new Uint8Array(Buffer.from(String(pares.State), "hex").buffer),
      ).getUint32(0, true)
      if (state !== 1) continue
      const time = pares.Time
        ? new DataView(new Uint8Array(Buffer.from(String(pares.Time), "hex").buffer)).getUint32(
            0,
            true,
          )
        : 0
      ach.push({ name: nome, unlockTime: time * 1000 })
    } catch {}
  }
  return ach
}

function parseCreamAPI(conteudo) {
  const secoes = parseINI(conteudo)
  const ach = []
  for (const [nome, pares] of Object.entries(secoes)) {
    if (pares.achieved === "true") {
      const ts = Number(pares.unlocktime || 0)
      ach.push({
        name: nome,
        unlockTime: ts > 1e11 ? ts : ts * 1000,
      })
    }
  }
  return ach
}

// --- Mapeamento de cracker → caminhos e parser ---

const USUARIO_WINE = "steamuser" // Proton sempre usa steamuser

function caminhosPrefixados(prefixo, appid) {
  const u = path.join(prefixo, "drive_c", "users", USUARIO_WINE)
  const a = path.join(u, "AppData", "Roaming")
  const pub = path.join(prefixo, "drive_c", "users", "Public", "Documents")
  const prog = path.join(prefixo, "drive_c", "ProgramData")
  const docs = path.join(u, "Documents")

  return [
    // Goldberg (2 variantes)
    {
      name: "goldberg",
      file: path.join(a, "Goldberg SteamEmu Saves", appid, "achievements.json"),
      parse: parseGoldberg,
    },
    {
      name: "goldberg",
      file: path.join(a, "GSE Saves", appid, "achievements.json"),
      parse: parseGoldberg,
    },
    // CODEX
    {
      name: "codex",
      file: path.join(pub, "Steam", "CODEX", appid, "achievements.ini"),
      parse: parseCODEX,
    },
    {
      name: "codex",
      file: path.join(a, "Steam", "CODEX", appid, "achievements.ini"),
      parse: parseCODEX,
    },
    // RUNE
    {
      name: "rune",
      file: path.join(pub, "Steam", "RUNE", appid, "achievements.ini"),
      parse: parseCODEX,
    },
    // Skidrow (3 variantes)
    {
      name: "skidrow",
      file: path.join(docs, "SKIDROW", appid, "SteamEmu", "UserStats", "achiev.ini"),
      parse: parseSkidrow,
    },
    {
      name: "skidrow",
      file: path.join(docs, "Player", appid, "SteamEmu", "UserStats", "achiev.ini"),
      parse: parseSkidrow,
    },
    {
      name: "skidrow",
      file: path.join(a, "..", "Local", "SKIDROW", appid, "SteamEmu", "UserStats", "achiev.ini"),
      parse: parseSkidrow,
    },
    // EMPRESS
    {
      name: "empress",
      file: path.join(a, "EMPRESS", "remote", appid, "achievements.json"),
      parse: parseGoldberg,
    },
    {
      name: "empress",
      file: path.join(pub, "EMPRESS", appid, "remote", appid, "achievements.json"),
      parse: parseGoldberg,
    },
    // OnlineFix
    {
      name: "onlinefix",
      file: path.join(pub, "OnlineFix", appid, "Stats", "Achievements.ini"),
      parse: parseOnlineFix,
    },
    {
      name: "onlinefix",
      file: path.join(pub, "OnlineFix", appid, "Achievements.ini"),
      parse: parseOnlineFix,
    },
    // CreamAPI
    {
      name: "creamapi",
      file: path.join(a, "CreamAPI", appid, "stats", "CreamAPI.Achievements.cfg"),
      parse: parseCreamAPI,
    },
    // SmartSteamEmu
    {
      name: "sse",
      file: path.join(a, "SmartSteamEmu", appid, "User", "Achievements.ini"),
      parse: parseOnlineFix,
    },
    // Razor1911
    {
      name: "razor",
      file: path.join(a, ".1911", appid, "achievement"),
      parse: parseRazor1911,
    },
    // RLD!
    {
      name: "rld",
      file: path.join(prog, "RLD!", appid, "achievements.ini"),
      parse: parseRLD,
    },
    {
      name: "rld",
      file: path.join(prog, "Steam", "Player", appid, "stats", "achievements.ini"),
      parse: parseRLD,
    },
    {
      name: "rld",
      file: path.join(prog, "Steam", "RLD!", appid, "stats", "achievements.ini"),
      parse: parseRLD,
    },
    // FLT (pasta, não arquivo)
    {
      name: "flt",
      file: null, // resolvido separadamente
      parse: null,
    },
  ]
}

// --- Resolvedor de prefixo ---

function resolvePrefixo(appid, entry) {
  // 1. Prefixo customizado do game_settings.json
  try {
    const settingsFile = path.join(os.homedir(), ".local/share/arcadia", "game_settings.json")
    if (fs.existsSync(settingsFile)) {
      const settings = JSON.parse(fs.readFileSync(settingsFile, "utf-8"))
      const key = entry && entry.id ? entry.id : `steam:${appid}`
      const cfg = settings[key] || settings[String(appid)]
      if (cfg && cfg.winePrefixPath) {
        return cfg.winePrefixPath
      }
    }
  } catch {}
  // 2. Proton: ~/.local/share/Steam/steamapps/compatdata/<appid>/pfx/
  return path.join(COMPATDATA, String(appid), "pfx")
}

// --- Lógica principal ---

function lerLibrary() {
  try {
    const raw = fs.readFileSync(
      path.join(os.homedir(), ".local/share/arcadia", "library.json"),
      "utf-8",
    )
    return JSON.parse(raw)
  } catch {
    return []
  }
}

function extrairAppid(entry) {
  // id = "steam:1091500" → "1091500"
  if (!entry || !entry.id) return null
  const m = /^steam:(\d+)$/.exec(String(entry.id))
  return m ? m[1] : null
}

function resolveExeDir(entry, appid) {
  // Lê game_settings.json pra achar o caminho do executável.
  // Devolve o diretório-pai do .exe (onde SteamData/ e 3DMGAME/ costumam ficar).
  try {
    const f = path.join(os.homedir(), ".local/share/arcadia", "game_settings.json")
    if (!fs.existsSync(f)) return null
    const settings = JSON.parse(fs.readFileSync(f, "utf-8"))
    const key = entry && entry.id ? entry.id : `steam:${appid}`
    const cfg = settings[key] || settings[String(appid)]
    if (!cfg || !cfg.exePath) return null
    return path.dirname(cfg.exePath)
  } catch {
    return null
  }
}

function iniciarVigia(onUnlock) {
  const cacheMtime = new Map() // filePath → mtimeMs

  const scan = () => {
    const library = lerLibrary()
    if (!library || !library.length) return

    for (const entry of library) {
      if (!entry.installed) continue
      const appid = extrairAppid(entry)
      if (!appid) continue

      const prefixo = resolvePrefixo(appid, entry)
      if (!fs.existsSync(prefixo)) continue

      // Carrega o índice de conquistas pra mapear apiname → item
      const store = loadAchievements()
      const items = store[appid] && store[appid].items ? store[appid].items : []
      if (!items.length) continue // sem schema, sem o que detectar

      const registros = caminhosPrefixados(prefixo, appid)

      for (const reg of registros) {
        if (!reg.file) continue
        if (!fs.existsSync(reg.file)) continue

        let mtime
        try {
          mtime = fs.statSync(reg.file).mtimeMs
        } catch {
          continue
        }

        const ultima = cacheMtime.get(reg.file)
        if (ultima === mtime) continue // sem mudanca
        cacheMtime.set(reg.file, mtime)

        // Parseia o arquivo
        let conteudo
        try {
          conteudo = fs.readFileSync(reg.file, "utf-8")
        } catch {
          continue
        }

        const desbloqueadas = reg.parse(conteudo)
        if (!desbloqueadas || !desbloqueadas.length) continue

        // Mapeia apiname → item do achievements.json
        const indexPorApiname = new Map()
        for (const it of items) {
          if (it.apiname) indexPorApiname.set(it.apiname.toLowerCase(), it)
        }

        let atualizou = false
        for (const d of desbloqueadas) {
          const it = indexPorApiname.get(String(d.name).toLowerCase())
          if (!it) continue
          if (it.achieved) continue // ja estava marcado

          it.achieved = true
          it.unlock = Math.floor(d.unlockTime / 1000) || Math.floor(Date.now() / 1000)
          atualizou = true

          // Dispara o toast
          if (onUnlock) {
            onUnlock({
              appid,
              key: `${it.block}|${it.bit}`,
              title: it.title,
              desc: it.desc,
              icon: it.icon,
              percent: it.percent || 0,
              unlock: it.unlock,
            })
          }
        }

        if (atualizou) {
          store[appid] = { ...(store[appid] || {}), items }
          saveAchievements(store)
        }
      }

      // FLT: pasta com 1 arquivo por conquista
      const fltDir = path.join(
        prefixo,
        "drive_c",
        "users",
        USUARIO_WINE,
        "AppData",
        "Roaming",
        "FLT",
        appid,
      )
      if (fs.existsSync(fltDir)) {
        let mtime
        try {
          mtime = fs.statSync(fltDir).mtimeMs
        } catch {
          continue
        }

        // FLT tracking via contagem de arquivos (mtime da pasta muda ao criar)
        const ultimaFlt = cacheMtime.get(fltDir)
        if (ultimaFlt === mtime) continue
        cacheMtime.set(fltDir, mtime)

        const desbloqueadas = parseFLT(fltDir)
        if (!desbloqueadas.length) continue

        const indexPorApiname = new Map()
        for (const it of items) {
          if (it.apiname) indexPorApiname.set(it.apiname.toLowerCase(), it)
        }

        let atualizou = false
        for (const d of desbloqueadas) {
          const it = indexPorApiname.get(String(d.name).toLowerCase())
          if (!it) continue
          if (it.achieved) continue

          it.achieved = true
          it.unlock = Math.floor((d.unlockTime || Date.now()) / 1000)
          atualizou = true

          if (onUnlock) {
            onUnlock({
              appid,
              key: `${it.block}|${it.bit}`,
              title: it.title,
              desc: it.desc,
              icon: it.icon,
              percent: it.percent || 0,
              unlock: it.unlock,
            })
          }
        }

        if (atualizou) {
          store[appid] = { ...(store[appid] || {}), items }
          saveAchievements(store)
        }
      }

      // SteamData e 3DMGAME: pastas na raiz do jogo (perto do .exe),
      // comuns em repacks. Exe path vem do game_settings.json.
      const pastaExe = resolveExeDir(entry, appid)
      if (pastaExe && fs.existsSync(pastaExe)) {
        const exeFiles = [
          {
            file: path.join(pastaExe, "SteamData", "user_stats.ini"),
            parse: parseUserStats,
          },
          {
            file: path.join(pastaExe, "3DMGAME", "Player", "stats", "achievements.ini"),
            parse: parseCODEX,
          },
        ]
        for (const ef of exeFiles) {
          if (!fs.existsSync(ef.file)) continue
          let mtime
          try {
            mtime = fs.statSync(ef.file).mtimeMs
          } catch {
            continue
          }
          const ultima = cacheMtime.get(ef.file)
          if (ultima === mtime) continue
          cacheMtime.set(ef.file, mtime)
          const desbloqueadas = ef.parse(fs.readFileSync(ef.file, "utf-8"))
          if (!desbloqueadas || !desbloqueadas.length) continue
          const idx = new Map()
          for (const it of items) {
            if (it.apiname) idx.set(it.apiname.toLowerCase(), it)
          }
          let mudou = false
          for (const d of desbloqueadas) {
            const it = idx.get(String(d.name).toLowerCase())
            if (!it) continue
            if (it.achieved) continue
            it.achieved = true
            it.unlock = Math.floor(d.unlockTime / 1000) || Math.floor(Date.now() / 1000)
            mudou = true
            if (onUnlock) {
              onUnlock({
                appid,
                key: `${it.block}|${it.bit}`,
                title: it.title,
                desc: it.desc,
                icon: it.icon,
                percent: it.percent || 0,
                unlock: it.unlock,
              })
            }
          }
          if (mudou) {
            store[appid] = { ...(store[appid] || {}), items }
            saveAchievements(store)
          }
        }
      }
    }
  }

  // Roda uma vez no inicio para sincronizar o estado atual
  scan()

  // Polling
  const interval = setInterval(scan, INTERVALO_POLL)

  return () => clearInterval(interval)
}

module.exports = { iniciarVigia }
