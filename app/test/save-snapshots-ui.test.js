"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const APP = path.join(__dirname, "..")
const PANEL = fs.readFileSync(
  path.join(APP, "src/components/desktop/SaveSnapshotsPanel.tsx"),
  "utf8",
)
const SETTINGS = fs.readFileSync(
  path.join(APP, "src/components/desktop/GameSettingsDialog.tsx"),
  "utf8",
)

// O painel é renderer-only: este teste protege a ponte existente contra uma
// refatoração que deixe a aba visual sem chamar os handlers novos de saves.
test("painel de salvamentos usa o IPC de snapshots e o seletor de pastas", () => {
  assert.match(SETTINGS, /SaveSnapshotsPanel/)
  assert.match(SETTINGS, /aba === "SALVAMENTOS"/)
  assert.match(PANEL, /launcherAPI\.savesList\(game\.id\)/)
  assert.match(PANEL, /launcherAPI\.savesCreate\(\{[\s\S]*sourceDir/)
  assert.match(
    PANEL,
    /launcherAPI\.savesRestore\(\{[\s\S]*snapshotId: snapshot\.id[\s\S]*targetDir/,
  )
  assert.match(
    PANEL,
    /launcherAPI\.savesDelete\(\{[\s\S]*gameId: game\.id[\s\S]*snapshotId: snapshot\.id[\s\S]*\}\)/,
  )
  assert.match(PANEL, /launcherAPI\?\.pickFolder/)
})

test("as três traduções têm as mensagens do painel", () => {
  const keys = [
    "salvamentos_aba",
    "salvamentos_titulo",
    "salvamentos_desc",
    "salvamentos_criar_botao",
    "salvamentos_restaurar_botao",
    "salvamentos_remover",
    "salvamentos_vazio",
  ]
  for (const lang of ["pt-BR", "en-US", "es-ES"]) {
    const messages = JSON.parse(fs.readFileSync(path.join(APP, `src/i18n/${lang}.json`), "utf8"))
    for (const key of keys)
      assert.equal(typeof messages[`gamesettings.${key}`], "string", `${lang}: ${key}`)
  }
})
