"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const {
  CONTRACT_VERSION,
  normalizeLibrary,
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
      { appid: "steam:11", removed: true, title: "ignorado" },
      { appid: "", title: "descartado" },
      { appid: "steam:12", title: "sem plataforma", platform: "unknown" },
    ]),
    [
      { appid: "steam:10", title: "Portal", platform: "linux" },
      { appid: "steam:11", removed: true },
      { appid: "steam:12", title: "sem plataforma", platform: "windows" },
    ],
  )
  assert.deepEqual(
    normalizePlaytimeItems([
      { appid: "steam:10", minutes: "30" },
      { appid: "steam:11", minutes: 0 },
      { appid: "steam:12", minutes: 999999 },
    ]),
    [
      { appid: "steam:10", minutes: 30 },
      { appid: "steam:12", minutes: 999999 },
    ],
  )
})
