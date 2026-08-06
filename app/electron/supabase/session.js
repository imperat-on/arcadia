// Sessão do Supabase persistida em disco (segredo — nunca vai pro config.json).
// Local: <dataDir>/session.json (padrão: ~/.local/share/arcadia)
// - Escrita atômica (tmp + rename, padrão do projeto)
// - Permissão 0600
// - Criptografada com safeStorage do Electron (keychain do SO) quando disponível;
//   fallback para texto puro quando não há keyring (ex.: testes, Linux sem KWallet)
//
// ARCADIA_DATA_DIR permite injetar outro diretório (usado nos testes).
"use strict"

const fs = require("fs")
const path = require("path")

const DEFAULT_DIR = path.join(
  process.env.HOME || process.env.USERPROFILE || ".",
  ".local",
  "share",
  "arcadia",
)

function dataDir() {
  return process.env.ARCADIA_DATA_DIR || DEFAULT_DIR
}

function sessionPath() {
  return path.join(dataDir(), "session.json")
}

// safeStorage só existe dentro do Electron; em Node puro (testes) é undefined.
function getSafeStorage() {
  try {
    const es = require("electron")
    return es && es.safeStorage ? es.safeStorage : null
  } catch {
    return null
  }
}

function canEncrypt() {
  const ss = getSafeStorage()
  return !!ss && typeof ss.isEncryptionAvailable === "function" && ss.isEncryptionAvailable()
}

function b64(s) {
  return Buffer.from(s, "utf8").toString("base64")
}
function unb64(s) {
  return Buffer.from(s, "base64").toString("utf8")
}

// Grava de forma atômica (tmp + rename) com permissão 0600.
function writeAtomic(file, content) {
  fs.mkdirSync(path.dirname(file), { recursive: true })
  const tmp = `${file}.tmp-${process.pid}-${Date.now()}`
  fs.writeFileSync(tmp, content, { encoding: "utf8", mode: 0o600 })
  fs.renameSync(tmp, file)
  try {
    fs.chmodSync(file, 0o600)
  } catch {
    /* best effort */
  }
}

/** Salva a sessão (objeto do supabase-js) em disco. */
function saveSession(session) {
  if (!session) return
  const payload = JSON.stringify(session)
  if (canEncrypt()) {
    const ss = getSafeStorage()
    const enc = ss.encryptString(payload).toString("base64")
    writeAtomic(sessionPath(), JSON.stringify({ v: 2, enc: true, data: enc }))
  } else {
    writeAtomic(sessionPath(), JSON.stringify({ v: 2, enc: false, data: b64(payload) }))
  }
}

/** Carrega a sessão do disco; devolve null se não existir/corrompida. */
function loadSession() {
  let raw
  try {
    raw = fs.readFileSync(sessionPath(), "utf8")
  } catch {
    return null
  }
  try {
    const box = JSON.parse(raw)
    if (!box || typeof box !== "object") return null
    if (box.v !== 2) return null
    if (box.enc) {
      const ss = getSafeStorage()
      if (!ss || !ss.decryptString) return null
      return JSON.parse(ss.decryptString(Buffer.from(box.data, "base64")).toString("utf8"))
    }
    return JSON.parse(unb64(box.data))
  } catch (err) {
    // arquivo corrompido ou chave mudou (keyring): apaga (auditoria A-13 —
    // o .bak antigo ficava pra sempre no disco com tokens de sessão), loga o
    // erro, e devolve null (usuário faz login de novo)
    console.error("[session] Falha ao decriptar sessão — keyring do SO mudou?", err?.message || err)
    try {
      fs.unlinkSync(sessionPath())
    } catch {
      /* ignore */
    }
    return null
  }
}

/** Remove a sessão do disco. */
function clearSession() {
  try {
    fs.unlinkSync(sessionPath())
  } catch {
    /* já não existe */
  }
}

module.exports = { saveSession, loadSession, clearSession, sessionPath }
