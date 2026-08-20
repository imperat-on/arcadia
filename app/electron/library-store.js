"use strict"

const fs = require("node:fs")
const path = require("node:path")
const { LIBRARY_SCHEMA_VERSION, normalizeLibrary } = require("../../contracts")

function parseLibraryDocument(value) {
  if (Array.isArray(value)) {
    return { version: 0, legacy: true, games: normalizeLibrary(value) }
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return { version: null, legacy: false, games: [], error: "formato_invalido" }
  }
  if (value.version !== LIBRARY_SCHEMA_VERSION || !Array.isArray(value.games)) {
    return { version: value.version ?? null, legacy: false, games: [], error: "versao_incompativel" }
  }
  return {
    version: LIBRARY_SCHEMA_VERSION,
    legacy: false,
    generatedAt: value.generated_at ?? value.generatedAt ?? null,
    sources: value.sources && typeof value.sources === "object" ? value.sources : {},
    games: normalizeLibrary(value.games),
  }
}

function readLibraryFile(filePath, fsImpl = fs) {
  try {
    return parseLibraryDocument(JSON.parse(fsImpl.readFileSync(filePath, "utf8")))
  } catch (error) {
    return { version: null, legacy: false, games: [], error: error.code === "ENOENT" ? "ausente" : "leitura_falhou" }
  }
}

function writeLibraryFile(filePath, games, { sources = {}, generatedAt = Math.floor(Date.now() / 1000), fsImpl = fs } = {}) {
  const payload = {
    version: LIBRARY_SCHEMA_VERSION,
    generated_at: generatedAt,
    sources: sources && typeof sources === "object" ? sources : {},
    games: normalizeLibrary(games),
  }
  const temporary = `${filePath}.tmp`
  fsImpl.mkdirSync(path.dirname(filePath), { recursive: true })
  fsImpl.writeFileSync(temporary, JSON.stringify(payload, null, 2))
  fsImpl.renameSync(temporary, filePath)
  return payload
}

module.exports = {
  LIBRARY_SCHEMA_VERSION,
  parseLibraryDocument,
  readLibraryFile,
  writeLibraryFile,
}
