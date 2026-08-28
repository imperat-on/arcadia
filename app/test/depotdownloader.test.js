"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const crypto = require("node:crypto")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const {
  createDepotDownloaderManager,
  DEPOT_RELEASE_URL,
  REQUIRED_FILES,
} = require("../electron/depotdownloader")

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-depotdownloader-"))
  return {
    root,
    depsDir: path.join(root, "bin", "deps", "depotdownloader"),
    tmpDir: path.join(root, "bin", "tmp"),
  }
}

function response(body, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    async arrayBuffer() {
      return body
    },
  }
}

function extractedArchive(target, names = REQUIRED_FILES) {
  fs.mkdirSync(target, { recursive: true })
  for (const name of names) fs.writeFileSync(path.join(target, name), name)
}

test("ensure baixa a variante compatível, renomeia o projeto e instala o framework completo", async () => {
  const f = fixture()
  const archive = Buffer.from("PK\x03\x04 fake archive")
  const calls = []
  const fetchImpl = async (url) => {
    calls.push(url)
    return response(archive)
  }
  const execFileImpl = (_cmd, args, callback) => {
    extractedArchive(args.at(-1), [
      "DepotDownloaderMod.dll",
      "DepotDownloaderMod.deps.json",
      "DepotDownloaderMod.runtimeconfig.json",
      ...REQUIRED_FILES.slice(3),
    ])
    callback(null, "", "")
  }
  const manager = createDepotDownloaderManager({
    depsDir: f.depsDir,
    tmpDir: f.tmpDir,
    fetchImpl,
    execFileImpl,
    archiveSha256: crypto.createHash("sha256").update(archive).digest("hex"),
    now: () => 123,
    pid: 456,
  })

  try {
    assert.equal(manager.installed(), false)
    const result = await manager.ensure()
    assert.deepEqual(result, { ok: true, path: path.join(f.depsDir, "DepotDownloader.dll") })
    assert.equal(manager.installed(), true)
    assert.deepEqual(calls, [DEPOT_RELEASE_URL])
    for (const name of REQUIRED_FILES)
      assert.equal(fs.existsSync(path.join(f.depsDir, name)), true, name)
    assert.deepEqual(fs.readdirSync(f.tmpDir), [])
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("ensure compartilha uma instalação concorrente e não baixa duas vezes", async () => {
  const f = fixture()
  const archive = Buffer.from("PK\x03\x04 concurrent")
  let downloads = 0
  let extraction
  const fetchImpl = async () => {
    downloads++
    await new Promise((resolve) => setImmediate(resolve))
    return response(archive)
  }
  const execFileImpl = (_cmd, args, callback) => {
    extraction = args.at(-1)
    extractedArchive(extraction)
    callback(null, "", "")
  }
  const manager = createDepotDownloaderManager({
    depsDir: f.depsDir,
    tmpDir: f.tmpDir,
    fetchImpl,
    execFileImpl,
    archiveSha256: crypto.createHash("sha256").update(archive).digest("hex"),
  })

  try {
    const [first, second] = await Promise.all([manager.ensure(), manager.ensure()])
    assert.equal(downloads, 1)
    assert.deepEqual(first, second)
    assert.equal(manager.installed(), true)
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})

test("ensure rejeita release alterada e deixa o diretório sem runtime parcial", async () => {
  const f = fixture()
  const archive = Buffer.from("PK\x03\x04 altered")
  const manager = createDepotDownloaderManager({
    depsDir: f.depsDir,
    tmpDir: f.tmpDir,
    fetchImpl: async () => response(archive),
    execFileImpl: () => assert.fail("não deve extrair hash inválido"),
    archiveSha256: "0".repeat(64),
  })

  try {
    const result = await manager.ensure()
    assert.equal(result.ok, false)
    assert.match(result.error, /hash da release não confere/)
    assert.equal(manager.installed(), false)
    assert.equal(fs.existsSync(f.depsDir), false)
  } finally {
    fs.rmSync(f.root, { recursive: true, force: true })
  }
})
