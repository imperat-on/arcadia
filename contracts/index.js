"use strict"

// Contratos pequenos e deliberadamente sem dependências. O main process e o
// backend usam as mesmas normalizações antes de aceitar dados vindos de disco,
// IPC ou rede. Campos adicionais são preservados para permitir evolução
// compatível sem perder metadados específicos de um provider.

const CONTRACT_VERSION = 1
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

/** Contrato reduzido usado por push_library/pull_library. */
function normalizeLibrarySyncItem(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null
  const appid = text(value.appid, MAX_ID_LENGTH)
  if (!appid) return null
  if (value.removed) return { appid, removed: true }
  return {
    appid,
    title: text(value.title, MAX_TITLE_LENGTH) || appid,
    platform: value.platform === "linux" ? "linux" : "windows",
  }
}

function normalizeLibrarySyncItems(value) {
  if (!Array.isArray(value)) return []
  return value.slice(0, MAX_SYNC_ITEMS).map(normalizeLibrarySyncItem).filter(Boolean)
}

function normalizePlaytimeItem(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null
  const appid = text(value.appid, MAX_ID_LENGTH)
  const minutes = Number(value.minutes)
  if (!appid || !Number.isSafeInteger(minutes) || minutes <= 0 || minutes > 999999) return null
  return { appid, minutes }
}

function normalizePlaytimeItems(value) {
  if (!Array.isArray(value)) return []
  return value.slice(0, MAX_SYNC_ITEMS).map(normalizePlaytimeItem).filter(Boolean)
}

module.exports = {
  CONTRACT_VERSION,
  MAX_ID_LENGTH,
  MAX_TITLE_LENGTH,
  MAX_COMMAND_ARGS,
  MAX_SYNC_ITEMS,
  normalizeGame,
  normalizeLibrary,
  normalizeLibrarySyncItem,
  normalizeLibrarySyncItems,
  normalizePlaytimeItem,
  normalizePlaytimeItems,
}
