"use strict"

const syncConflicts = require("./sync")

// Contratos pequenos e deliberadamente sem dependências. O main process e o
// backend usam as mesmas normalizações antes de aceitar dados vindos de disco,
// IPC ou rede. Campos adicionais são preservados para permitir evolução
// compatível sem perder metadados específicos de um provider.

const CONTRACT_VERSION = 1
const LIBRARY_SCHEMA_VERSION = 1
const MAX_ID_LENGTH = 512
const MAX_TITLE_LENGTH = 1024
const MAX_COMMAND_ARGS = 64
const MAX_SYNC_ITEMS = 1000

function text(value, maxLength) {
  if (typeof value !== "string" && typeof value !== "number") return null
  const result = String(value).trim()
  if (!result || result.length > maxLength) return null
  return result
}

function command(value) {
  if (!Array.isArray(value)) return []
  return value
    .filter((part) => typeof part === "string")
    .slice(0, MAX_COMMAND_ARGS)
}

/** Normaliza uma entrada de biblioteca sem remover campos de providers. */
function normalizeGame(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null
  const id = text(value.id, MAX_ID_LENGTH)
  const title = text(value.title, MAX_TITLE_LENGTH)
  if (!id || !title) return null

  return {
    ...value,
    id,
    title,
    launcher: text(value.launcher, 64) || "custom",
    launch_cmd: command(value.launch_cmd),
  }
}

function normalizeLibrary(value) {
  if (!Array.isArray(value)) return []
  return value.map(normalizeGame).filter(Boolean)
}

/** Remove tokens e metadados arbitrários antes de cruzar a fronteira IPC. */
function safeAccountSession(session) {
  const user = session?.user
  const id = text(user?.id, MAX_ID_LENGTH)
  if (!id) return null
  const safeUser = { id }
  const email = text(user.email, 320)
  const username = text(user.user_metadata?.username ?? user.username, 64)
  if (email) safeUser.email = email
  if (username) safeUser.username = username
  return { user: safeUser }
}

/** Resultado público de login/cadastro: tokens ficam somente no main process. */
function safeAuthResult(result) {
  if (!result || typeof result !== "object" || Array.isArray(result)) {
    return { ok: false, error: "resposta_invalida" }
  }
  const safe = { ok: result.ok === true }
  if (result.error != null) safe.error = String(result.error)
  if (result.usernameReal != null) {
    const username = text(result.usernameReal, 64)
    if (username) safe.usernameReal = username
  }
  return safe
}

function safeAccountStatus(result) {
  if (!result || typeof result !== "object" || Array.isArray(result)) {
    return { session: null, error: "resposta_invalida" }
  }
  return {
    session: safeAccountSession(result.session),
    error: result.error == null ? null : String(result.error),
  }
}

function safeAccountEvent(event, session) {
  return {
    event: String(event || ""),
    session: safeAccountSession(session),
  }
}

/** Contrato reduzido usado por push_library/pull_library. */
function normalizeLibrarySyncItem(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null
  const appid = text(value.appid, MAX_ID_LENGTH)
  if (!appid) return null
  if (value.removed === true) return { appid, removed: true }
  return {
    appid,
    title: text(value.title, MAX_TITLE_LENGTH) || appid,
    platform: value.platform === "emulator" ? "emulator" : value.platform === "linux" ? "linux" : "windows",
  }
}

function normalizeLibrarySyncItems(value) {
  if (!Array.isArray(value)) return []
  return value.slice(0, MAX_SYNC_ITEMS).map(normalizeLibrarySyncItem).filter(Boolean)
}

function normalizePlaytimeItem(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null
  const appid = text(value.appid, MAX_ID_LENGTH)
  const rawMinutes = value.minutes
  if (
    !appid ||
    (typeof rawMinutes !== "number" && typeof rawMinutes !== "string") ||
    (typeof rawMinutes === "string" && !/^-?[0-9]+$/.test(rawMinutes.trim()))
  ) return null
  const minutes = Number(rawMinutes)
  if (!Number.isSafeInteger(minutes) || minutes <= 0 || minutes > 999999) return null
  return { appid, minutes }
}

function normalizePlaytimeItems(value) {
  if (!Array.isArray(value)) return []
  return value.slice(0, MAX_SYNC_ITEMS).map(normalizePlaytimeItem).filter(Boolean)
}

module.exports = {
  CONTRACT_VERSION,
  LIBRARY_SCHEMA_VERSION,
  MAX_ID_LENGTH,
  MAX_TITLE_LENGTH,
  MAX_COMMAND_ARGS,
  MAX_SYNC_ITEMS,
  normalizeGame,
  normalizeLibrary,
  safeAccountSession,
  safeAuthResult,
  safeAccountStatus,
  safeAccountEvent,
  normalizeLibrarySyncItem,
  normalizeLibrarySyncItems,
  normalizePlaytimeItem,
  normalizePlaytimeItems,
  ...syncConflicts,
}
