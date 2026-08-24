"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const {
  LIBRARY_SCHEMA_VERSION,
  createLibraryRepository,
  parseLibraryDocument,
  readLibraryFile,
  writeLibraryFile,
} = require("../electron/library-repository")

function fixture() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-library-repository-"))
  let active = null
  const account = {
    conta: () => active,
    caminhoArquivoConta: (name) => {
      const target = active ? path.join(dir, "contas", active, path.basename(name)) : path.join(dir, path.basename(name))
      if (active) fs.mkdirSync(path.dirname(target), { recursive: true })
      return target
    },
  }
  const repository = createLibraryRepository({ dataDir: dir, account })
  return {
    dir,
    repository,
    account,
    use(username) {
      active = username || null
    },
    cleanup() {
      fs.rmSync(dir, { recursive: true, force: true })
    },
  }
}

const GAMES = [
  { id: "steam:1", title: "Primeiro", launcher: "steam" },
  { id: "custom:2", title: "Segundo", launcher: "custom" },
  { id: "steam:3", title: "Terceiro", launcher: "steam" },
]

test("repository lê documento legado e envelope sem perder o contrato", () => {
  const legacy = parseLibraryDocument([{ id: "custom:1", title: "Local" }])
  assert.equal(legacy.version, 0)
  assert.equal(legacy.legacy, true)
  assert.equal(legacy.games[0].launcher, "custom")

  const current = parseLibraryDocument({
    version: LIBRARY_SCHEMA_VERSION,
    generated_at: 42,
    sources: { steam: 1 },
    games: GAMES,
  })
  assert.equal(current.version, LIBRARY_SCHEMA_VERSION)
  assert.equal(current.generatedAt, 42)
  assert.deepEqual(current.sources, { steam: 1 })
  assert.deepEqual(current.errors, [])
  assert.equal(current.games.length, 3)
})

test("repository escreve biblioteca versionada com rename atômico", () => {
  const f = fixture()
  try {
    const file = path.join(f.dir, "nested", "library.json")
    const payload = writeLibraryFile(file, [{ id: "custom:1", title: "Local" }], {
      generatedAt: 123,
      sources: { custom: 1 },
    })
    assert.equal(payload.version, LIBRARY_SCHEMA_VERSION)
    assert.deepEqual(readLibraryFile(file), {
      version: 1,
      legacy: false,
      generatedAt: 123,
      sources: { custom: 1 },
      errors: [],
      games: [{ id: "custom:1", title: "Local", launcher: "custom", launch_cmd: [] }],
    })
    assert.deepEqual(fs.readdirSync(path.dirname(file)), ["library.json"])
  } finally {
    f.cleanup()
  }
})

test("guest vê toda a biblioteca e nunca materializa owned na raiz", () => {
  const f = fixture()
  try {
    f.repository.writeLibrary(GAMES, { generatedAt: 1 })
    f.use(null)
    assert.deepEqual(f.repository.readLibrary().map((game) => game.id), ["steam:1", "custom:2", "steam:3"])
    assert.equal(f.repository.ownedSet().size, 0)
    assert.equal(fs.existsSync(path.join(f.dir, "owned_games.json")), false)
  } finally {
    f.cleanup()
  }
})

test("factory aceita resolver de conta e escopa owned sem Electron", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-library-repository-account-"))
  try {
    const repository = createLibraryRepository({ dataDir: dir, account: () => "alice" })
    writeLibraryFile(path.join(dir, "library.json"), GAMES)
    const owned = path.join(dir, "contas", "alice", "owned_games.json")
    fs.mkdirSync(path.dirname(owned), { recursive: true })
    fs.writeFileSync(owned, JSON.stringify(["steam:3"]))
    assert.deepEqual(repository.readLibrary().map((game) => game.id), ["steam:3"])
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

test("conta filtra owned e troca de arquivo aparece na leitura seguinte", () => {
  const f = fixture()
  try {
    f.repository.writeLibrary(GAMES, { generatedAt: 1 })
    f.use("alice")
    const owned = f.repository.ownedPath()
    fs.writeFileSync(owned, JSON.stringify(["custom:2"]))
    assert.deepEqual(f.repository.readLibrary().map((game) => game.id), ["custom:2"])

    fs.writeFileSync(owned, JSON.stringify(["steam:3", "steam:1"]))
    assert.deepEqual(f.repository.readLibrary().map((game) => game.id), ["steam:1", "steam:3"])
  } finally {
    f.cleanup()
  }
})

test("conta antiga materializa posse ausente sem bloquear a leitura", () => {
  const f = fixture()
  try {
    f.repository.writeLibrary(GAMES, { generatedAt: 1 })
    f.use("alice")
    const owned = path.join(f.dir, "contas", "alice", "owned_games.json")
    assert.equal(fs.existsSync(owned), false)
    assert.deepEqual(f.repository.readLibrary().map((game) => game.id), ["steam:1", "custom:2", "steam:3"])
    assert.deepEqual(JSON.parse(fs.readFileSync(owned, "utf8")), ["steam:1", "custom:2", "steam:3"])
  } finally {
    f.cleanup()
  }
})

test("materialização e filtro não escrevem posse para guest", () => {
  const f = fixture()
  try {
    f.repository.writeLibrary(GAMES)
    f.use(null)
    assert.equal(f.repository.materializeOwned(GAMES), false)
    assert.equal(f.repository.writeOwned(["steam:1"]), null)
    assert.equal(f.repository.addOwned("steam:1"), false)
    assert.equal(f.repository.removeOwned("steam:1"), false)
    assert.equal(fs.existsSync(path.join(f.dir, "owned_games.json")), false)
  } finally {
    f.cleanup()
  }
})

test("ownedAdd/ownedRemove usam escrita atômica e não duplicam ids", () => {
  const f = fixture()
  try {
    f.use("alice")
    assert.equal(f.repository.addOwned("steam:1"), true)
    assert.equal(f.repository.addOwned("steam:1"), false)
    assert.equal(f.repository.addOwned("custom:2"), true)
    assert.deepEqual([...f.repository.ownedSet()], ["steam:1", "custom:2"])
    assert.equal(f.repository.removeOwned("steam:1"), true)
    assert.equal(f.repository.removeOwned("missing"), false)
    assert.deepEqual([...f.repository.ownedSet()], ["custom:2"])
    assert.deepEqual(JSON.parse(fs.readFileSync(f.repository.ownedPath(), "utf8")), ["custom:2"])
  } finally {
    f.cleanup()
  }
})

test("falha no rename mantém biblioteca anterior e limpa temporário", () => {
  const f = fixture()
  try {
    const file = path.join(f.dir, "library.json")
    writeLibraryFile(file, [{ id: "custom:old", title: "Antigo" }])
    const failingFs = {
      ...fs,
      renameSync() {
        throw new Error("rename falhou")
      },
    }
    assert.throws(
      () => writeLibraryFile(file, [{ id: "custom:new", title: "Novo" }], { fsImpl: failingFs }),
      /rename falhou/,
    )
    assert.deepEqual(readLibraryFile(file).games.map((game) => game.id), ["custom:old"])
    assert.deepEqual(fs.readdirSync(f.dir), ["library.json"])
  } finally {
    f.cleanup()
  }
})
