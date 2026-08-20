"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const { createLaunchLog } = require("../electron/launch-log")

test("launch-log grava comando e fecha descritor", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-launch-log-"))
  try {
    const log = createLaunchLog({ logDir: dir, now: () => new Date("2026-01-01T00:00:00.000Z") })
    const opened = log.open("steam:440", ["steam", "steam://rungameid/440"])
    assert.equal(opened.stdio[0], "ignore")
    assert.ok(opened.path.endsWith("steam_440.log"))
    assert.match(fs.readFileSync(opened.path, "utf8"), /2026-01-01T00:00:00.000Z/)
    opened.close()
    opened.close()
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

test("launch-log rotaciona logs maiores que 5 MiB", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-launch-log-"))
  try {
    const file = path.join(dir, "game.log")
    fs.writeFileSync(file, "x".repeat(5 * 1024 * 1024 + 1))
    const log = createLaunchLog({ logDir: dir })
    const opened = log.open("game", ["game"])
    assert.ok(fs.existsSync(`${file}.old`))
    opened.close()
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})
