// Sessão do backend persistida em disco (segredo — nunca vai pro config.json).
// Local: <dataDir>/session.json (padrão: ~/.local/share/arcadia)
// - Escrita atômica (tmp + rename, padrão do projeto)
// - Permissão 0600
// - Criptografada com safeStorage do Electron (keychain do SO) quando disponível;
//   fallback para texto puro quando não há keyring (ex.: testes, Linux sem KWallet)
//
// ARCADIA_DATA_DIR permite injetar outro diretório (usado nos testes).
"use strict"

const fs = require("fs")
const path = require("node:path")
const { getDataDir } = require("../runtime-paths")

function dataDir() {
  return getDataDir()
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

// Tokens nao devem cair em texto puro em uma instalacao real. O fallback fica
// disponivel apenas para testes ou quando o usuario opta explicitamente por ele.
function allowPlaintextFallback() {
  // Algumas instalações Linux não expõem um keyring ao Electron (KWallet/
  // Secret Service desativado). Nessa situação a sessão não pode simplesmente
  // desaparecer ao fechar o app: persistimos o envelope base64 com permissão
  // 0600 e mantemos a opção explícita para desativar esse fallback.
  if (process.env.ARCADIA_DISABLE_PLAINTEXT_SESSION === "1") return false
  return process.env.NODE_ENV === "test" || Boolean(process.versions?.electron) || process.env.ARCADIA_ALLOW_PLAINTEXT_SESSION === "1" || process.env.ARCADA_TEST_SESSION === "1"
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
  if (!canEncrypt() && !allowPlaintextFallback()) {
    console.error("[session] keyring indisponivel; sessao nao sera persistida")
    return
  }
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
