"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const { getRunningEmulatorStatus, preflightRunningEmulator } = require("../electron/emulator-runtime")

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-proc-"))
  const proc = path.join(root, "proc")
  fs.mkdirSync(path.join(proc, "4242"), { recursive: true })
  fs.writeFileSync(path.join(proc, "4242", "comm"), "rpcs3\n")
  return { root, proc }
}

test("detecta RPCS3 já executando sem criar processos", () => {
  const f = fixture()
  try {
    const status = getRunningEmulatorStatus({ emulatorId: "rpcs3", procRoot: f.proc })
    assert.deepEqual(status, { ok: true, emulatorId: "rpcs3", running: true, pid: 4242 })
    const blocked = preflightRunningEmulator({ emulatorId: "rpcs3", procRoot: f.proc })
    assert.equal(blocked.error, "EMULATOR_ALREADY_RUNNING")
    assert.equal(blocked.code, "EMULATOR_ALREADY_RUNNING")
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("ignora nomes parecidos, PID inválido e /proc indisponível", () => {
  const f = fixture()
  fs.mkdirSync(path.join(f.proc, "bad"))
  fs.writeFileSync(path.join(f.proc, "4242", "comm"), "not-rpcs3\n")
  try {
    assert.equal(getRunningEmulatorStatus({ emulatorId: "rpcs3", procRoot: f.proc }).running, false)
    assert.equal(getRunningEmulatorStatus({ emulatorId: "rpcs3", procRoot: path.join(f.root, "missing") }).running, false)
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})
