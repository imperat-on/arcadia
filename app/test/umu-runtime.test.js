"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const crypto = require("node:crypto")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const {
  UMU_ARCHIVE_SHA256,
  ensureUmuLauncher,
  findUmuLauncher,
  managedUmuPath,
} = require("../electron/umu-runtime")

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex")
}

function tarWithUmu(executable) {
  const header = Buffer.alloc(512)
  header.write("umu/umu-run")
  header.write(executable.length.toString(8).padStart(11, "0") + "\0", 124, "ascii")
  header[156] = 48
  return Buffer.concat([
    header,
    executable,
    Buffer.alloc((512 - (executable.length % 512)) % 512),
    Buffer.alloc(1024),
  ])
}

test("runtime UMU rejeita cópia gerenciada adulterada e mantém fallback externo", () => {
  const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-umu-valid-"))
  const target = managedUmuPath(dataDir)
  const externalDir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-umu-external-"))
  const external = path.join(externalDir, "umu-run")
  fs.mkdirSync(path.dirname(target), { recursive: true })
  fs.writeFileSync(target, "adulterado")
  fs.chmodSync(target, 0o755)
  fs.writeFileSync(external, "#!/bin/sh\n")
  fs.chmodSync(external, 0o755)
  assert.equal(findUmuLauncher({ dataDir, home: "/nonexistent", envPath: externalDir }), external)
})

test("runtime UMU rejeita download com digest diferente", async () => {
  const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-umu-invalid-"))
  const archive = tarWithUmu(Buffer.from("#!/bin/sh\n"))
  assert.notEqual(sha256(archive), UMU_ARCHIVE_SHA256)
  const result = await ensureUmuLauncher({
    dataDir,
    home: "/nonexistent",
    envPath: "",
    fetchImpl: async () => ({ ok: true, arrayBuffer: async () => archive }),
  })
  assert.equal(result.ok, false)
  assert.match(result.error, /SHA-256/)
  assert.equal(fs.existsSync(managedUmuPath(dataDir)), false)
})
