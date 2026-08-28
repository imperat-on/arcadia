"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { isSteamInstallUri, shouldTrackGameSession } = require("../electron/game-session")

test("steam://install não inicia acompanhamento de jogo", () => {
  assert.equal(isSteamInstallUri("steam://install/123"), true)
  assert.equal(shouldTrackGameSession(["steam", "steam://install/123"]), false)
  assert.equal(shouldTrackGameSession(["steam", "STEAM://INSTALL/123"]), false)
})

test("steam://run e comandos arbitrários continuam acompanhados", () => {
  assert.equal(shouldTrackGameSession(["steam", "steam://rungameid/123"]), true)
  assert.equal(shouldTrackGameSession(["/games/game", "--fullscreen"]), true)
})
