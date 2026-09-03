// Vigia de conquistas de jogos "Jack Sparrow" (crackeados/emulados).
// Polling a cada 15s nos arquivos de conquista dos crackers dentro do
// prefixo Wine de cada jogo. Formatos suportados:
//   Goldberg  → JSON  (earned / earned_time)
//   UPC/voices38 → JSON  (numeric id / earned / earned_time)
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
const { caminhoArquivoConta, conta } = require("./../supabase/conta")
const { dataPath } = require("./../runtime-paths")
const { readLibraryFile } = require("../library-store")
const {
  parseUPC,
  resolveUplayId,
  numericAchievementId,
  uplayRuntimePath,
  uplaySaveRoot,
} = require("./uplay")

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

function readSettings(entry, appid) {
  try {
    const settingsFile = caminhoArquivoConta("game_settings.json")
    if (!fs.existsSync(settingsFile)) return {}
    const settings = JSON.parse(fs.readFileSync(settingsFile, "utf-8"))
    const key = entry && entry.id ? entry.id : `steam:${appid}`
    return settings[key] || settings[String(appid)] || {}
  } catch {
    return {}
  }
}

function numericUplayDirs(prefixo) {
  const root = uplaySaveRoot(prefixo)
  if (!root) return []
  try {
    return fs
      .readdirSync(root, { withFileTypes: true })
      .filter((entry) => entry.isDirectory() && /^\d{1,18}$/.test(entry.name))
      .map((entry) => entry.name.replace(/^0+(?=\d)/, ""))
  } catch {
    return []
  }
}

function caminhosPrefixados(prefixo, appid, entry) {
  // Raízes por plataforma: no Linux são derivadas do prefixo Wine; no Windows
  // nativo, das pastas reais do usuário (%APPDATA%, %PUBLIC%, etc.).
  const raizes = raizesCrack(prefixo)
  const u = raizes.user
  const a = raizes.appdata
  const pub = raizes.publicDocs
  const prog = raizes.programData
  const docs = raizes.userDocs
  const local = raizes.localAppData
  const settings = readSettings(entry, appid)
  const uplayIds = new Set()
  const configuredUplayId = resolveUplayId(appid, settings, entry)
  if (configuredUplayId) uplayIds.add(configuredUplayId)
  // No Windows não há prefixo Wine: todos os jogos compartilham a mesma pasta
  // de saves (%APPDATA%\Goldberg UplayEmu Saves), então enumerar os diretórios
  // numéricos vazaria o uplayId de UM jogo para TODOS os outros (ex.: um
  // cruzamento do runtime do Black Flag no Cyberpunk). Só enumeramos quando o
  // jogo é UPC confirmado — aí o ID da pasta é dele. Para jogos sem relação
  // UPC confirmada, o próprio configuredUplayId (via KNOWN_UPC_IDS/settings)
  // já resolve o runtime correto.
  if (configuredUplayId) {
    for (const id of numericUplayDirs(prefixo)) uplayIds.add(id)
  }

  const upcRecords = [...uplayIds].map((uplayId) => ({
    name: "upc",
    uplayId,
    file: uplayRuntimePath(prefixo, uplayId),
    parse: parseUPC,
  }))

  return [
    // UPC/voices38 (schema na raiz do jogo; runtime na pasta de saves).
    ...upcRecords,
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
      file: path.join(local, "SKIDROW", appid, "SteamEmu", "UserStats", "achiev.ini"),
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

// Raízes das pastas de save dos crackers, por plataforma. No Linux entram
// dentro do prefixo Wine (drive_c/users/steamuser ou Public); no Windows são
// as pastas reais do usuário. Mesma árvore, base diferente.
function raizesCrack(prefixo) {
  if (process.platform === "win32") {
    const home = os.homedir()
    const appdata = process.env.APPDATA || path.join(home, "AppData", "Roaming")
    const localAppData = process.env.LOCALAPPDATA || path.join(home, "AppData", "Local")
    const publicDocs = process.env.PUBLIC
      ? path.join(process.env.PUBLIC, "Documents")
      : path.join("C:", "Users", "Public", "Documents")
    const programData = process.env.PROGRAMDATA || path.join("C:", "ProgramData")
    return {
      user: appdata, // base do UPC — em Windows, também vive em %APPDATA%
      appdata,
      localAppData,
      publicDocs,
      programData,
      // OneDrive move a pasta Documents real — resolve por existência, com
      // fallback para o path clássico quando nada existe ainda.
      userDocs: pastaDocumentosWindows(home),
    }
  }
  const u = path.join(prefixo, "drive_c", "users", USUARIO_WINE)
  return {
    user: u,
    appdata: path.join(u, "AppData", "Roaming"),
    localAppData: path.join(u, "AppData", "Local"),
    publicDocs: path.join(prefixo, "drive_c", "users", "Public", "Documents"),
    programData: path.join(prefixo, "drive_c", "ProgramData"),
    userDocs: path.join(u, "Documents"),
  }
}

// Documents pode estar redirecionado (OneDrive/empresa). Devolve a pasta que
// existe; senão, o path clássico como fallback determinístico.
function pastaDocumentosWindows(home) {
  const candidatos = [
    path.join(home, "Documents"),
    path.join(home, "OneDrive", "Documents"),
    path.join(home, "OneDrive - Personal", "Documents"),
  ]
  for (const c of candidatos) {
    try {
      if (fs.statSync(c).isDirectory()) return c
    } catch {}
  }
  return candidatos[0]
}

// --- Resolvedor de prefixo ---

function resolvePrefixo(appid, entry) {
  const settings = readSettings(entry, appid)
  // `prefixPath` é o nome usado pelo diálogo atual. `winePrefixPath` fica
  // aceito apenas para instalações antigas.
  if (settings.prefixPath || settings.winePrefixPath) {
    return settings.prefixPath || settings.winePrefixPath
  }
  // O Arcadia usa prefixos próprios por gameId. É importante passar o ID
  // completo (steam:3751950), pois winemanager transforma ':' em '_'.
  try {
    const { prefixOf } = require("../winemanager")
    const prefix = prefixOf(entry && entry.id ? entry.id : `steam:${appid}`)
    if (prefix) return prefix
  } catch {}
  // Fallback legado para instalações que ainda usam compatdata da Steam.
  return path.join(COMPATDATA, String(appid), "pfx")
}

// Resolve um registro do UPC para o item do Arcadia. O loader atual grava
// somente a chave decimal ("40"), enquanto o catálogo Steam usa, neste jogo,
// ACObsidian_Ach_40. Nunca usamos a posição do array quando há um ID explícito.
function itemParaDesbloqueio(items, desbloqueio, registro) {
  const byName = new Map()
  for (const item of items) {
    if (item && item.apiname) byName.set(String(item.apiname).toLowerCase(), item)
  }
  if (registro?.name === "upc") {
    const id = numericAchievementId(desbloqueio.id)
    if (id !== null) {
      for (const item of items) {
        const itemId = [item?.uplayId, item?.upcId, item?.apiname, item?.name]
          .map((value) => numericAchievementId(value))
          .find((value) => value !== null)
        if (itemId === id) return item
      }
    }
  }
  const names = [desbloqueio.name, desbloqueio.apiname, desbloqueio.id]
  for (const name of names) {
    const item = byName.get(String(name ?? "").toLowerCase())
    if (item) return item
  }
  return null
}

function payloadParaDesbloqueio(appid, item, desbloqueio, registro) {
  return {
    appid,
    key:
      item.block != null && item.bit != null
        ? `${item.block}|${item.bit}`
        : String(item.apiname || desbloqueio.id || ""),
    apiname: item.apiname,
    provider: registro?.name || "cracked",
    title: item.title,
    desc: item.desc,
    icon: item.icon,
    percent: item.percent || 0,
    unlock: Math.floor((desbloqueio.unlockTime || 0) / 1000) || Math.floor(Date.now() / 1000),
  }
}

// --- Lógica principal ---

function lerLibrary() {
  const raw = readLibraryFile(dataPath("library.json"))
  const games = Array.isArray(raw?.games) ? raw.games.slice() : []
  const byId = new Map(games.filter((game) => game && game.id).map((game) => [String(game.id), game]))
  // Um jogo pode estar configurado e ainda não aparecer no snapshot global da
  // biblioteca (por exemplo, enquanto o pull da Steam está pendente). As
  // configurações por conta são uma fonte segura adicional para o watcher:
  // sem exePath/runtime não há arquivo para ler, então não há falso unlock.
  try {
    const settingsFile = caminhoArquivoConta("game_settings.json")
    const settings = JSON.parse(fs.readFileSync(settingsFile, "utf-8"))
    for (const [id, cfg] of Object.entries(settings || {})) {
      if (!/^steam:\d+$/.test(id) || !cfg || (!cfg.exePath && !cfg.prefixPath && !cfg.uplayId && !cfg.upcId)) continue
      const old = byId.get(id)
      if (old) {
        old.installed = true
      } else {
        const game = { id, installed: true, exe: cfg.exePath }
        games.push(game)
        byId.set(id, game)
      }
    }
  } catch {}
  return games
}

function extrairAppid(entry) {
  // id = "steam:1091500" → "1091500"
  if (!entry || !entry.id) return null
  const m = /^steam:(\d+)$/.exec(String(entry.id))
  return m ? m[1] : null
}

function resolveExeDir(entry, appid) {
  // Diretório do executável (onde SteamData/ e 3DMGAME/ costumam ficar).
  const cfg = readSettings(entry, appid)
  const exePath = cfg && cfg.exePath ? String(cfg.exePath) : entry && entry.exe
  return exePath ? path.dirname(exePath) : null
}

function iniciarVigia(onUnlock) {
  const cacheMtime = new Map() // account + filePath → mtimeMs
  const cacheKey = (filePath) => `${conta() || "__guest__"}\0${filePath}`

  const scan = () => {
    const library = lerLibrary()
    if (!library || !library.length) return

    for (const entry of library) {
      if (!entry.installed) continue
      const appid = extrairAppid(entry)
      if (!appid) continue

      const prefixo = resolvePrefixo(appid, entry)
      // No Windows nativo não há prefixo Wine — as raízes reais (APPDATA,
      // PUBLIC, PROGRAMDATA...) são resolvidas dentro de raizesCrack. O gate
      // de existência do prefixo vale só para o Linux.
      if (process.platform !== "win32" && !fs.existsSync(prefixo)) continue

      // Carrega o índice de conquistas pra mapear apiname → item
      const store = loadAchievements()
      const items = store[appid] && store[appid].items ? store[appid].items : []
      if (!items.length) continue // sem schema, sem o que detectar

      const registros = caminhosPrefixados(prefixo, appid, entry)

      for (const reg of registros) {
        if (!reg.file) continue
        if (!fs.existsSync(reg.file)) continue

        let mtime
        try {
          mtime = fs.statSync(reg.file).mtimeMs
        } catch {
          continue
        }

        const ultima = cacheMtime.get(cacheKey(reg.file))
        if (ultima === mtime) continue // sem mudanca
        cacheMtime.set(cacheKey(reg.file), mtime)

        // Parseia o arquivo
        let conteudo
        try {
          conteudo = fs.readFileSync(reg.file, "utf-8")
        } catch {
          continue
        }

        const desbloqueadas = reg.parse(conteudo)
        if (!desbloqueadas || !desbloqueadas.length) continue

        let atualizou = false
        for (const d of desbloqueadas) {
          const it = itemParaDesbloqueio(items, d, reg)
          if (!it) continue
          if (it.achieved) continue // ja estava marcado

          const payload = payloadParaDesbloqueio(appid, it, d, reg)
          it.achieved = true
          it.unlock = payload.unlock
          atualizou = true

          // Dispara o toast
          if (onUnlock) onUnlock(payload)
        }

        if (atualizou) {
          store[appid] = { ...(store[appid] || {}), items }
          saveAchievements(store)
        }
      }

      // FLT: pasta com 1 arquivo por conquista
      const raizes = raizesCrack(prefixo)
      const fltDir = path.join(raizes.appdata, "FLT", appid)
      if (fs.existsSync(fltDir)) {
        let mtime
        try {
          mtime = fs.statSync(fltDir).mtimeMs
        } catch {
          continue
        }

        // FLT tracking via contagem de arquivos (mtime da pasta muda ao criar)
        const ultimaFlt = cacheMtime.get(cacheKey(fltDir))
        if (ultimaFlt === mtime) continue
        cacheMtime.set(cacheKey(fltDir), mtime)

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
          const ultima = cacheMtime.get(cacheKey(ef.file))
          if (ultima === mtime) continue
          cacheMtime.set(cacheKey(ef.file), mtime)
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

module.exports = {
  iniciarVigia,
  parseINI,
  parseGoldbergJSON,
  parseGoldberg,
  parseUPC,
  parseRazor1911,
  parseFLT,
  parseCODEX,
  parseSkidrow,
  parseUserStats,
  parseOnlineFix,
  parseRLD,
  parseCreamAPI,
  caminhosPrefixados,
  raizesCrack,
  resolvePrefixo,
  resolveExeDir,
  lerLibrary,
  extrairAppid,
  itemParaDesbloqueio,
  payloadParaDesbloqueio,
  INTERVALO_POLL,
}
