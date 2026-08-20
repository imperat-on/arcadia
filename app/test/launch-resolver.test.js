"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { resolveLaunchRequest } = require("../electron/launch-resolver")

function deps(overrides = {}) {
  return {
    findGame: () => ({ id: "steam:440", launch_cmd: ["steam", "steam://rungameid/440"] }),
    customLaunchCmd: () => ({ cmd: ["wine", "/games/custom.exe"], env: { WINEPREFIX: "/tmp/pfx" } }),
    getGameSettings: () => ({ exePath: "/games/override.exe" }),
    exeLaunchCmd: (_id, exe) => ({ cmd: ["wine", exe], env: { WINEPREFIX: "/tmp/pfx" } }),
    ...overrides,
  }
}

test("gameId sempre usa comando da biblioteca, não cmd arbitrário do renderer", () => {
  const result = resolveLaunchRequest(
    { gameId: "steam:440", cmd: ["rm", "-rf", "/"] },
    deps({ getGameSettings: () => ({}) }),
  )
  assert.deepEqual(result.rawCmd, ["steam", "steam://rungameid/440"])
})

test("gameId desconhecido e jogo sem comando são rejeitados", () => {
  assert.equal(resolveLaunchRequest({ gameId: "steam:999", cmd: ["evil"] }, deps({ findGame: () => null })).ok, false)
  const result = resolveLaunchRequest(
    { gameId: "steam:999", cmd: ["evil"] },
    deps({ findGame: () => ({ id: "steam:999", launch_cmd: [] }) }),
  )
  assert.match(result.error, /Sem comando/)
})

test("legado aceita somente steam://install|run numérico", () => {
  assert.deepEqual(resolveLaunchRequest(["steam", "steam://run/440"], deps()).rawCmd, ["steam", "steam://run/440"])
  assert.equal(resolveLaunchRequest(["bash", "-c", "id"], deps()).ok, false)
  assert.equal(resolveLaunchRequest(["steam", "steam://run/abc"], deps()).ok, false)
})

test("custom e override exe usam callbacks controlados pelo main", () => {
  const custom = resolveLaunchRequest({ gameId: "custom:one", cmd: ["evil"] }, deps())
  assert.deepEqual(custom.rawCmd, ["wine", "/games/custom.exe"])
  assert.deepEqual(custom.envExtra, { WINEPREFIX: "/tmp/pfx" })

  const exe = resolveLaunchRequest({ gameId: "steam:440", mode: "exe" }, deps())
  assert.deepEqual(exe.rawCmd, ["wine", "/games/override.exe"])
  const steam = resolveLaunchRequest({ gameId: "steam:440", mode: "steam" }, deps())
  assert.deepEqual(steam.rawCmd, ["steam", "steam://rungameid/440"])
})
