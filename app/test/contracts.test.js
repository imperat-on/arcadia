"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const {
  CONTRACT_VERSION,
  normalizeLibrary,
  safeAccountSession,
  safeAuthResult,
  safeAccountStatus,
  safeAccountEvent,
  normalizeLibrarySyncItems,
  normalizePlaytimeItems,
} = require("../../contracts")

test("contrato de biblioteca normaliza jogos e preserva metadados", () => {
  const jogos = normalizeLibrary([
    {
      id: "steam:10",
      title: "  Portal 2  ",
      launcher: "steam",
      launch_cmd: ["steam", 10, null, "steam://rungameid/10"],
      customProviderFlag: true,
    },
    { id: "custom:1", title: "Jogo local", exe: "/tmp/game.exe" },
    { id: "", title: "invalido" },
    null,
  ])

  assert.equal(CONTRACT_VERSION, 1)
  assert.equal(jogos.length, 2)
  assert.equal(jogos[0].title, "Portal 2")
  assert.deepEqual(jogos[0].launch_cmd, ["steam", "steam://rungameid/10"])
  assert.equal(jogos[0].customProviderFlag, true)
  assert.equal(jogos[1].launcher, "custom")
  assert.deepEqual(jogos[1].launch_cmd, [])
})

test("contrato de sync rejeita payloads inválidos e limita itens", () => {
  assert.deepEqual(
    normalizeLibrarySyncItems([
      { appid: "steam:10", title: "Portal", platform: "linux" },
      { appid: "custom:rom", title: "ROM local", platform: "emulator" },
      { appid: "steam:11", removed: true, title: "ignorado" },
      { appid: "steam:12", removed: "false", title: "não remover" },
      { appid: "", title: "descartado" },
      { appid: "steam:13", title: "sem plataforma", platform: "unknown" },
    ]),
    [
      { appid: "steam:10", title: "Portal", platform: "linux" },
      { appid: "custom:rom", title: "ROM local", platform: "emulator" },
      { appid: "steam:11", removed: true },
      { appid: "steam:12", title: "não remover", platform: "windows" },
      { appid: "steam:13", title: "sem plataforma", platform: "windows" },
    ],
  )
  assert.deepEqual(
    normalizePlaytimeItems([
      { appid: "steam:10", minutes: "30" },
      { appid: "steam:11", minutes: 0 },
      { appid: "steam:12", minutes: true },
      { appid: "steam:13", minutes: 999999 },
    ]),
    [
      { appid: "steam:10", minutes: 30 },
      { appid: "steam:13", minutes: 999999 },
    ],
  )
})


test("sessão pública remove access/refresh tokens na fronteira IPC", () => {
  const session = safeAccountSession({
    access_token: "access-secret",
    refresh_token: "refresh-secret",
    user: {
      id: "user-1",
      email: "user@example.com",
      user_metadata: { username: "player", secret: "não expor" },
    },
  })
  assert.deepEqual(session, {
    user: { id: "user-1", email: "user@example.com", username: "player" },
  })
  assert.equal("access_token" in session, false)
  assert.equal("refresh_token" in session, false)

  const result = safeAuthResult({
    ok: true,
    session: { access_token: "access-secret", refresh_token: "refresh-secret" },
    user: { id: "user-1" },
    usernameReal: "player",
  })
  assert.deepEqual(result, { ok: true, usernameReal: "player" })
  assert.equal(JSON.stringify(result).includes("access-secret"), false)
  assert.equal(JSON.stringify(result).includes("refresh-secret"), false)

  assert.deepEqual(
    safeAccountStatus({
      session: { access_token: "access-secret", user: { id: "user-1" } },
      error: null,
      leaked: "never",
    }),
    { session: { user: { id: "user-1" } }, error: null },
  )
  assert.deepEqual(
    safeAccountEvent("SIGNED_IN", {
      refresh_token: "refresh-secret",
      user: { id: "user-1", user_metadata: { username: "player" } },
    }),
    { event: "SIGNED_IN", session: { user: { id: "user-1", username: "player" } } },
  )
})
