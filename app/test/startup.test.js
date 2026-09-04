"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { EventEmitter } = require("node:events")
const fs = require("node:fs")
const path = require("node:path")
const { resolveLauncherMode, ignoreBrokenPipe } = require("../electron/startup")

const root = path.join(__dirname, "..")

test("resolve modo respeita modo explícito antes dos fallbacks", () => {
  assert.equal(resolveLauncherMode({}), "desktop")
  assert.equal(resolveLauncherMode({ PS5_FULLSCREEN: "1" }), "console")
  assert.equal(resolveLauncherMode({ ARCADIA_MODE: "console" }), "console")
  assert.equal(resolveLauncherMode({ ARCADIA_MODE: "desktop", PS5_FULLSCREEN: "1" }), "desktop")
})

test("forçar desktop e preferência vencem o fullscreen legado", () => {
  assert.equal(resolveLauncherMode({ ARCADIA_FORCE_DESKTOP: "1", PS5_FULLSCREEN: "1" }), "desktop")
  assert.equal(resolveLauncherMode({ PS5_FULLSCREEN: "1" }, { start_in_console_mode: false }), "desktop")
  assert.equal(resolveLauncherMode({}, { start_in_console_mode: true }), "console")
  assert.equal(resolveLauncherMode({ ARCADIA_MODE: "console", ARCADIA_FORCE_DESKTOP: "1" }), "console")
})

test("stdout fechado não derruba o main, mas outros erros continuam visíveis", () => {
  const stream = new EventEmitter()
  ignoreBrokenPipe(stream)
  assert.doesNotThrow(() => stream.emit("error", Object.assign(new Error("closed"), { code: "EPIPE" })))
  assert.throws(() => stream.emit("error", Object.assign(new Error("disk"), { code: "EIO" })), /disk/)
})

test("preload e contexto mantêm desktop como fallback e isolam a seed do modo", () => {
  const preload = fs.readFileSync(path.join(root, "electron", "preload.js"), "utf8")
  const renderer = fs.readFileSync(path.join(root, "src", "main.tsx"), "utf8")
  const context = fs.readFileSync(path.join(root, "src", "components", "ModeContext.tsx"), "utf8")
  assert.match(preload, /ARCADIA_MODE \|\| "desktop"/)
  assert.match(renderer, /ModeProvider/)
  assert.match(renderer, /modeRef\.current === "console" \? "desktop" : "console"/)
  assert.match(context, /window\.launcherMode === "console" \? "console" : "desktop"/)
  assert.equal((renderer.match(/launcherMode/g) || []).length, 0)
})

test("main resolve a preferência antes de criar a janela e mantém o shell trocável", () => {
  const main = fs.readFileSync(path.join(root, "electron", "main.js"), "utf8")
  assert.match(main, /resolveLauncherMode\(process\.env, readConfig\(\)\)/)
  assert.match(main, /fullscreen: launcherMode === "console"/)
  assert.match(main, /frame: false/)
  assert.match(main, /ipcMain\.handle\("app:setMode"/)
})


test("gamescope usa opções suportadas e preserva handoff externo", () => {
  const main = fs.readFileSync(path.join(root, "electron", "main.js"), "utf8")
  const session = fs.readFileSync(path.join(root, "electron", "gamescope-session.js"), "utf8")
  assert.doesNotMatch(main, /["']--disable-gamemode["']/)
  assert.match(session, /--keep-alive/)
  assert.match(session, /RemainAfterExit=yes/)
  assert.match(session, /TimeoutStopSec=5s/)
  assert.match(session, /--setenv=\$\{key\}/)
  assert.match(session, /--wait/)
  assert.match(main, /buildExternalGamescopeCommand\(finalCmd/)
})


test("gamescope fica disponível no catálogo unificado e é filtrado pelo modo real", () => {
  const dialog = fs.readFileSync(path.join(root, "src", "components", "desktop", "GameSettingsDialog.tsx"), "utf8")
  const main = fs.readFileSync(path.join(root, "electron", "main.js"), "utf8")
  assert.match(dialog, /k="gamescope"/)
  assert.doesNotMatch(dialog, /isSteamGame|disabled=\{isSteamGame\}/)
  // O gate ganhou o guard win32 e o strip de ".exe" no port para Windows
  // (steam.exe nao e wrapper gamescope; no Windows gamescope nao existe).
  // A checagem exata e feita pelo indexOf abaixo (string literal, sem regex).
  const externalGamescopeGate = main.indexOf(
    'process.platform !== "win32" && s.gamescope && path.basename(String(cmd[0])).replace(/\\.exe$/, "") !== "steam"',
  )
  const hdrEnvironmentAssignment = main.indexOf("env.ENABLE_GAMESCOPE_WSI")
  assert.ok(externalGamescopeGate >= 0 && externalGamescopeGate < hdrEnvironmentAssignment)
})

test("Proton externo usa o runtime UMU gerenciado pelo Arcadia", () => {
  const main = fs.readFileSync(path.join(root, "electron", "main.js"), "utf8")
  assert.match(main, /ensureUmuLauncher\(\)/)
  assert.match(main, /cmd: \[umu, g\.exe\]/)
  assert.match(main, /PROTONPATH: v\.path/)
  assert.match(main, /PROTON_VERB: "waitforexitandrun"/)
  assert.match(main, /cwd: path\.dirname\(executable\)/)
})

test("Gamescope expõe campos canônicos na UI, tipos e backend", () => {
  const dialog = fs.readFileSync(
    path.join(root, "src", "components", "desktop", "GameSettingsDialog.tsx"),
    "utf8",
  )
  const types = fs.readFileSync(path.join(root, "src", "global.d.ts"), "utf8")
  const main = fs.readFileSync(path.join(root, "electron", "main.js"), "utf8")
  const session = fs.readFileSync(path.join(root, "electron", "gamescope-session.js"), "utf8")

  for (const field of ["gsHdr", "gsWindowMode", "gsFramerateLimit"]) {
    assert.match(dialog, new RegExp(field), `UI: ${field}`)
    assert.match(types, new RegExp(field), `types: ${field}`)
    assert.match(main, new RegExp(field), `backend: ${field}`)
  }
  assert.match(dialog, /2560x1600/)
  assert.match(dialog, /1920x1080/)
  for (const mode of ["fullscreen", "borderless", "windowed"])
    assert.match(dialog, new RegExp(`['"]${mode}['"]`), `mode: ${mode}`)

  assert.match(session, /--framerate-limit/)
  assert.match(session, /--hdr-enabled/)
  assert.match(session, /windowMode/)
  assert.match(main, /ENABLE_GAMESCOPE_WSI/)
  assert.match(main, /windowMode: s\.gsWindowMode \?\? "fullscreen"/)
  assert.match(main, /framerateLimit: s\.gsFramerateLimit/)
})

test("todas as mensagens Gamescope usadas pelo diálogo existem nas três línguas", () => {
  const dialog = fs.readFileSync(
    path.join(root, "src", "components", "desktop", "GameSettingsDialog.tsx"),
    "utf8",
  )
  const keys = [...dialog.matchAll(/t\(["'](gamesettings\.[^"']+)["']/g)].map((match) => match[1])
  assert.ok(keys.length > 0)
  for (const lang of ["pt-BR", "en-US", "es-ES"]) {
    const messages = JSON.parse(fs.readFileSync(path.join(root, `src/i18n/${lang}.json`), "utf8"))
    for (const key of new Set(keys)) assert.equal(typeof messages[key], "string", `${lang}: ${key}`)
  }
})
