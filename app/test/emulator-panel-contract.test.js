"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.resolve(__dirname, "..")
const panel = fs.readFileSync(
  path.join(root, "src", "components", "desktop", "EmulatorProfilesPanel.tsx"),
  "utf8",
)
const dialog = fs.readFileSync(
  path.join(root, "src", "components", "desktop", "GameSettingsDialog.tsx"),
  "utf8",
)

test("painel de emuladores lista, detecta e persiste perfil/ROM via bridge", () => {
  for (const method of [
    "emulatorsList",
    "emulatorsDetect",
    "emulatorProfileSet",
    "emulatorProfileRemove",
    "emulatorsRoms",
    "emulatorsStatus",
    "customGameAdd",
    "gameSettingsSet",
    "pickFile",
    "pickFolder",
    "gameSettingsSet",
  ])
    assert.match(panel + dialog, new RegExp(method), method)
  assert.match(panel, /sem shell/i)
  assert.match(panel, /emulatorArgs/)
  assert.match(dialog, /EmulatorProfilesPanel/)
  const addGame = fs.readFileSync(
    path.join(root, "src", "components", "desktop", "AddGameDialog.tsx"),
    "utf8",
  )
  assert.match(addGame, /Emulador \(ROM\/ISO\)/)
  assert.match(addGame, /emulatorProfileSet/)
  assert.match(addGame, /emulatorsResolve/)
})

test("painel não executa comandos nem acessa ipcRenderer", () => {
  assert.doesNotMatch(panel, /ipcRenderer|child_process|spawn\(/)
  assert.doesNotMatch(panel, /access_token|refresh_token|authorization/i)
})
