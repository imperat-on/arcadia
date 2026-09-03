"use strict"

// Resolve a raiz de dados do Arcadia em um unico lugar.
//
// O app pode ser executado do clone, de uma instalacao em
// ~/.local/share/arcadia ou de um diretorio temporario de testes. Todos os
// subsistemas que gravam estado devem usar este modulo em vez de reconstruir
// ~/.local/share/arcadia por conta propria.
const os = require("node:os")
const path = require("node:path")

const DATA_DIR_ENV = "ARCADIA_DATA_DIR"

function getDefaultDataDir() {
  if (process.platform === "win32") {
    return path.join(process.env.LOCALAPPDATA || path.join(os.homedir(), "AppData", "Local"), "arcadia")
  }
  return path.join(os.homedir(), ".local", "share", "arcadia")
}

const DEFAULT_DATA_DIR = getDefaultDataDir()

function resolveDataDir(value = process.env[DATA_DIR_ENV]) {
  const raw = typeof value === "string" ? value.trim() : ""
  return path.resolve(raw || DEFAULT_DATA_DIR)
}

function getDataDir() {
  return resolveDataDir()
}

function dataPath(...parts) {
  return path.join(getDataDir(), ...parts)
}

function accountPath(username, ...parts) {
  const user = String(username || "").trim()
  if (!user) return dataPath(...parts)
  return dataPath("contas", user, ...parts)
}

module.exports = {
  DATA_DIR_ENV,
  DEFAULT_DATA_DIR,
  resolveDataDir,
  getDataDir,
  dataPath,
  accountPath,
}
