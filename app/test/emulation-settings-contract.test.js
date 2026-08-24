"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.resolve(__dirname, "..")
const section = fs.readFileSync(path.join(root, "src", "components", "desktop", "EmulationSection.tsx"), "utf8")
const installer = fs.readFileSync(path.join(root, "src", "components", "desktop", "EmulatorInstallDialog.tsx"), "utf8")
const sidebar = fs.readFileSync(path.join(root, "src", "components", "desktop", "Sidebar.tsx"), "utf8")
const settings = fs.readFileSync(path.join(root, "src", "components", "desktop", "SettingsView.tsx"), "utf8")

test("Emulação é uma subaba global de Configurações", () => {
  assert.match(sidebar, /"emulacao"/)
  assert.match(sidebar, /settings\.emulacao/)
  assert.match(sidebar, /settings\.emulacao\.novo/)
  assert.match(settings, /EmulationSection/)
  for (const api of ["emulatorsList", "emulatorsDetect", "emulatorsStatus", "emulatorsRomIndex", "emulatorProfileSet", "emulatorProfileRemove", "pickFile", "pickFolder"]) {
    assert.match(section, new RegExp(api), api)
  }
  assert.match(section, /Configuração do/)
  assert.match(section, /Encontrar .* no seu computador/)
  assert.match(section, /Explorar manualmente/)
  assert.match(section, /Não tenho o/)
  assert.match(installer, /Instalar \{name\}/)
  assert.match(installer, /openExternal/)
  assert.match(installer, /flatpak install flathub/)
  for (const asset of ["dolphin.svg", "ppsspp.svg", "melonds.svg", "desmume.svg", "retroarch.svg", "ps1.png", "ps2.png", "ps3.png"]) {
    assert.match(section, new RegExp(asset.replace(".", "\\.")), asset)
  }
})

test("a tela global não usa shell nem binário no renderer", () => {
  assert.doesNotMatch(section, /ipcRenderer|child_process|spawn\(/)
  assert.doesNotMatch(installer, /ipcRenderer|child_process|spawn\(/)
  assert.match(section, /launchMode|emulatorsRoms/)
})
