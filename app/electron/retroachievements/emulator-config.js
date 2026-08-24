"use strict"

// Escreve as credenciais RetroAchievements diretamente nos arquivos de
// config dos emuladores suportados (PCSX2, DuckStation, PPSSPP), replicando
// o formato que cada um usa nativamente. Nenhum emulador precisa estar
// rodando: escrevemos o arquivo antes do launch, e o próprio emulador lê
// isso ao iniciar (mesmo protocolo/campos que ele usaria se o usuário
// tivesse feito login pela própria UI dele).
//
// Importante: o valor persistido é sempre um TOKEN DE SESSÃO (obtido via
// retroachievements/client.js loginRequest), nunca a senha nem a Web API Key.
const fsDefault = require("node:fs")
const path = require("node:path")
const crypto = require("node:crypto")

const LOGIN_KEY_ROUNDS = 100

function normalizeAbsolute(value) {
  if (typeof value !== "string" || !value.trim() || value.includes("\u0000")) return ""
  return path.normalize(value.trim())
}

function safeMkdir(directory, fsImpl) {
  fsImpl.mkdirSync(directory, { recursive: true, mode: 0o700 })
}

function isSymlink(file, fsImpl) {
  try {
    return fsImpl.lstatSync(file).isSymbolicLink()
  } catch {
    return false
  }
}

// Escrita atômica (tmp + rename), mesmo contrato usado no resto do projeto
// (emulator-registry.js atomicWrite / achievements/steam_bin.js).
function atomicWriteText(file, text, fsImpl) {
  const directory = path.dirname(file)
  safeMkdir(directory, fsImpl)
  if (isSymlink(file, fsImpl)) throw new Error("arquivo_config_symlink")
  const temporary = `${file}.${process.pid}.${Date.now()}.tmp`
  if (isSymlink(temporary, fsImpl)) throw new Error("arquivo_temp_symlink")
  fsImpl.writeFileSync(temporary, text, { encoding: "utf8", mode: 0o600, flag: "wx" })
  if (isSymlink(temporary, fsImpl)) throw new Error("arquivo_temp_symlink")
  fsImpl.renameSync(temporary, file)
}

function readTextIfExists(file, fsImpl) {
  try {
    if (isSymlink(file, fsImpl)) return null
    return fsImpl.readFileSync(file, "utf8")
  } catch {
    return null
  }
}

// --- INI helpers -----------------------------------------------------------
//
// Parser/serializer minimalista: preserva todo o resto do arquivo (outras
// seções, comentários, ordem) e só toca nas chaves pedidas dentro da seção
// indicada. Cria a seção no final se ela não existir.

function upsertIniSection(text, sectionName, keyValues) {
  const lines = (text || "").split(/\r?\n/)
  const sectionHeader = `[${sectionName}]`
  let sectionStart = -1
  let sectionEnd = lines.length
  for (let i = 0; i < lines.length; i++) {
    const match = /^\s*\[([^\]]+)\]\s*$/.exec(lines[i])
    if (match) {
      if (sectionStart === -1 && match[1].trim() === sectionName) {
        sectionStart = i
      } else if (sectionStart !== -1 && i > sectionStart) {
        sectionEnd = i
        break
      }
    }
  }

  const pending = new Map(Object.entries(keyValues))

  if (sectionStart === -1) {
    // Seção não existe: acrescenta no final do arquivo.
    const block = [sectionHeader, ...[...pending.entries()].map(([k, v]) => `${k} = ${v}`)]
    const prefix = lines.length && lines[lines.length - 1] !== "" ? [...lines, ""] : lines
    return [...prefix, ...block, ""].join("\n")
  }

  const out = lines.slice(0, sectionStart + 1)
  for (let i = sectionStart + 1; i < sectionEnd; i++) {
    const line = lines[i]
    const keyMatch = /^\s*([^=\s][^=]*?)\s*=/.exec(line)
    const key = keyMatch ? keyMatch[1] : null
    if (key && pending.has(key)) {
      out.push(`${key} = ${pending.get(key)}`)
      pending.delete(key)
    } else {
      out.push(line)
    }
  }
  for (const [k, v] of pending) out.push(`${k} = ${v}`)
  out.push(...lines.slice(sectionEnd))
  return out.join("\n")
}

// --- DuckStation: AES-128-CBC do token, replicando Achievements::EncryptLoginToken ---
//
// key = SHA256(machineKey + username) (ou só username, em modo portátil),
// mais 100 rounds extras de SHA256. AES key = key[0:16], IV = key[16:32].
// Padding com zeros (não PKCS7), depois base64.

function sha256(buffer) {
  return crypto.createHash("sha256").update(buffer).digest()
}

function readMachineKey(fsImpl) {
  try {
    const id = fsImpl.readFileSync("/etc/machine-id", "utf8").trim()
    return id ? Buffer.from(id, "utf8") : Buffer.alloc(0)
  } catch {
    return Buffer.alloc(0)
  }
}

function duckstationLoginKey(username, { portable = false, fsImpl = fsDefault } = {}) {
  const usernameBuf = Buffer.from(String(username), "utf8")
  const machineKey = portable ? Buffer.alloc(0) : readMachineKey(fsImpl)
  let key = sha256(Buffer.concat([machineKey, usernameBuf]))
  for (let i = 0; i < LOGIN_KEY_ROUNDS; i++) key = sha256(key)
  return key // 32 bytes
}

function encryptDuckstationToken(token, username, opts = {}) {
  const key = duckstationLoginKey(username, opts)
  const aesKey = key.subarray(0, 16)
  const iv = key.subarray(16, 32)
  const plaintext = Buffer.from(String(token), "utf8")
  const padLength = (16 - (plaintext.length % 16)) % 16
  const padded = Buffer.concat([plaintext, Buffer.alloc(padLength)])
  const cipher = crypto.createCipheriv("aes-128-cbc", aesKey, iv)
  cipher.setAutoPadding(false)
  const encrypted = Buffer.concat([cipher.update(padded), cipher.final()])
  return encrypted.toString("base64")
}

// --- Caminhos de config por emulador ---------------------------------------

function pcsx2ConfigPaths(home) {
  return [
    path.join(home, ".config", "PCSX2", "inis", "PCSX2.ini"),
    path.join(home, ".var", "app", "net.pcsx2.PCSX2", "config", "PCSX2", "inis", "PCSX2.ini"),
  ]
}

function duckstationConfigPaths(home) {
  return [
    path.join(home, ".local", "share", "duckstation", "settings.ini"),
    path.join(home, ".var", "app", "org.duckstation.DuckStation", "data", "duckstation", "settings.ini"),
  ]
}

function ppsspConfigDir(home, env = process.env) {
  const xdgConfig = env.XDG_CONFIG_HOME
  const base = xdgConfig && xdgConfig.trim() ? xdgConfig.trim() : path.join(home, ".config")
  return path.join(base, "ppsspp")
}

function ppsspIniPath(home, env) {
  return path.join(ppsspConfigDir(home, env), "ppsspp.ini")
}

function ppsspSecretPath(home, env) {
  // GetSysDirectory(DIRECTORY_SYSTEM) = <memStickDirectory>/PSP/SYSTEM; no
  // Linux o memStickDirectory padrão é o mesmo diretório de config.
  return path.join(ppsspConfigDir(home, env), "PSP", "SYSTEM", "ppsspp_retroachievements.dat")
}

// --- Escritores por emulador -------------------------------------------------

function applyToExistingDirs(candidatePaths, fsImpl, write) {
  const touched = []
  for (const file of candidatePaths) {
    const directoryExists = (() => {
      try {
        return fsImpl.lstatSync(path.dirname(file)).isDirectory()
      } catch {
        return false
      }
    })()
    if (!directoryExists) continue
    write(file)
    touched.push(file)
  }
  return touched
}

/**
 * Configura PCSX2 (seção [Achievements], chaves Username/Token/Enabled).
 * Token em texto puro, sem criptografia (é assim que o próprio PCSX2 grava).
 */
function configurePcsx2({ username, token, enabled = true, home, fsImpl = fsDefault } = {}) {
  const candidates = pcsx2ConfigPaths(home)
  const write = (file) => {
    const current = readTextIfExists(file, fsImpl) || ""
    const next = upsertIniSection(current, "Achievements", {
      Enabled: enabled ? "true" : "false",
      Username: username,
      Token: token,
    })
    atomicWriteText(file, next, fsImpl)
  }
  // Sem instalação detectada ainda: cria o caminho nativo padrão mesmo assim,
  // porque o usuário provavelmente vai instalar o emulador depois de salvar
  // a credencial, e a config já fica pronta pro primeiro launch.
  const touched = applyToExistingDirs(candidates, fsImpl, write)
  if (!touched.length) {
    write(candidates[0])
    touched.push(candidates[0])
  }
  return { ok: true, files: touched }
}

/**
 * Configura DuckStation (seção [Cheevos], chaves Username/Token/Enabled).
 * Token criptografado com AES-128-CBC (ver encryptDuckstationToken).
 */
function configureDuckstation({ username, token, enabled = true, home, fsImpl = fsDefault, portable = false } = {}) {
  const candidates = duckstationConfigPaths(home)
  const encryptedToken = encryptDuckstationToken(token, username, { portable, fsImpl })
  const write = (file) => {
    const current = readTextIfExists(file, fsImpl) || ""
    const next = upsertIniSection(current, "Cheevos", {
      Enabled: enabled ? "true" : "false",
      Username: username,
      Token: encryptedToken,
    })
    atomicWriteText(file, next, fsImpl)
  }
  const touched = applyToExistingDirs(candidates, fsImpl, write)
  if (!touched.length) {
    write(candidates[0])
    touched.push(candidates[0])
  }
  return { ok: true, files: touched }
}

/**
 * Configura PPSSPP: usuário no ppsspp.ini (chave AchievementsUserName), e o
 * token no "secret" fora do ini (ppsspp_retroachievements.dat), replicando
 * NativeSaveSecret/GetSecretPath do próprio PPSSPP. Texto puro, sem
 * criptografia — mesma política do PPSSPP no Linux.
 */
function configurePpsspp({ username, token, enabled = true, home, fsImpl = fsDefault, env = process.env } = {}) {
  const iniFile = ppsspIniPath(home, env)
  const secretFile = ppsspSecretPath(home, env)

  const current = readTextIfExists(iniFile, fsImpl) || ""
  const next = upsertIniSection(current, "Achievements", {
    AchievementsEnable: enabled ? "true" : "false",
    AchievementsUserName: username,
  })
  atomicWriteText(iniFile, next, fsImpl)
  atomicWriteText(secretFile, token, fsImpl)
  return { ok: true, files: [iniFile, secretFile] }
}

/**
 * Ponto de entrada único: aplica a credencial no emulador certo conforme o
 * emulatorId resolvido pelo emulator-registry (pcsx2 | duckstation | ppsspp).
 * Emuladores sem client RA embutido (rpcs3, melonds, desmume, retroarch)
 * devolvem ok:false com um erro claro em vez de escrever algo sem efeito.
 */
function configureEmulatorCredentials(emulatorId, { username, token, enabled = true, home, fsImpl = fsDefault } = {}) {
  if (!username || !token) return { ok: false, error: "credenciais_vazias" }
  const resolvedHome = home || require("node:os").homedir()
  switch (emulatorId) {
    case "pcsx2":
      return configurePcsx2({ username, token, enabled, home: resolvedHome, fsImpl })
    case "duckstation":
      return configureDuckstation({ username, token, enabled, home: resolvedHome, fsImpl })
    case "ppsspp":
      return configurePpsspp({ username, token, enabled, home: resolvedHome, fsImpl })
    default:
      return { ok: false, error: "emulador_sem_suporte_retroachievements" }
  }
}

module.exports = {
  upsertIniSection,
  encryptDuckstationToken,
  pcsx2ConfigPaths,
  duckstationConfigPaths,
  ppsspIniPath,
  ppsspSecretPath,
  configurePcsx2,
  configureDuckstation,
  configurePpsspp,
  configureEmulatorCredentials,
}
