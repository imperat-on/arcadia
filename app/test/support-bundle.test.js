"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const { createSupportBundle, redactText } = require("../electron/support-bundle")

test("support bundle redige credenciais, paths e limita logs", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-support-"))
  const data = path.join(root, "data")
  const output = path.join(root, "out")
  fs.mkdirSync(path.join(data, "logs"), { recursive: true })
  fs.writeFileSync(path.join(data, "logs", "app.log"), `path=${data} token=abc123 Bearer secret-value\n`)
  try {
    assert.equal(redactText("password: hunter2 access_token=abc", { dataDir: data, homeDir: "/home/test" }).includes("hunter2"), false)
    const service = createSupportBundle({ dataDir: data, homeDir: "/home/test", now: () => new Date("2026-01-01T00:00:00.000Z") })
    const result = service.create({ outputDir: output, report: { storage: { path: data }, token: "secret" } })
    assert.equal(result.ok, true)
    assert.ok(result.path.startsWith(output))
    const log = fs.readFileSync(path.join(result.path, "logs", "app.log"), "utf8")
    assert.equal(log.includes("abc123"), false)
    assert.equal(log.includes("<DATA_DIR>"), true)
    assert.equal(fs.readFileSync(path.join(result.path, "manifest.json"), "utf8").includes('"redacted": true'), true)
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})
