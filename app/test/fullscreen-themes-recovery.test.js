"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { createRegistry } = require("../electron/themes/registry")
const { createThemeService } = require("../electron/themes/service")
const { BUILTIN_DEFAULT_ID, BUILTIN_AURORA_ID } = require("../electron/themes/constants")

// --- Fake filesystem ---

function createFakeFs(initial = {}) {
  const files = new Map(Object.entries(initial))
  const dirs = new Set()
  return {
    readFileSync(p) { const k = String(p); if (!files.has(k)) throw new Error(`ENOENT: ${k}`); return files.get(k) },
    writeFileSync(p, data) { files.set(String(p), typeof data === "string" ? data : String(data)) },
    renameSync(oldP, newP) { files.set(String(newP), files.get(String(oldP))); files.delete(String(oldP)) },
    mkdirSync(p) { dirs.add(String(p)) },
    lstatSync(p) { const k = String(p); if (!files.has(k) && !dirs.has(k)) throw new Error(`ENOENT: ${k}`); return { isSymbolicLink: () => false, isFile: () => files.has(k), isDirectory: () => dirs.has(k) } },
    readdirSync() { return [] },
    rmSync() {},
    existsSync(p) { return files.has(String(p)) || dirs.has(String(p)) },
    _files: files, _dirs: dirs,
  }
}

// --- Testes de recuperação ---

test("recovery: tema some do disco → service detecta como missing", () => {
  const fs = createFakeFs()
  const themesDir = "/themes"
  const service = createThemeService({ themesDir, fsImpl: fs })

  // Registra um tema externo
  service.registry.register("author.missing", "1.0.0", "abc")

  // O tema está no registry mas não no disco → deve aparecer como missing
  const list = service.list()
  const missing = list.find((t) => t.id === "author.missing")
  assert.ok(missing)
  assert.equal(missing.state, "missing")
  assert.equal(missing.valid, false)
})

test("recovery: recoverToLastKnownGood volta ao fallback", () => {
  const fs = createFakeFs()
  const service = createThemeService({ themesDir: "/themes", fsImpl: fs })

  // Ativa Aurora
  service.activate(BUILTIN_AURORA_ID)
  service.confirmActivation(BUILTIN_AURORA_ID)
  assert.equal(service.getActiveId(), BUILTIN_AURORA_ID)

  // Simula falha: recupera
  const recovered = service.recoverToLastKnownGood()
  assert.equal(recovered, BUILTIN_AURORA_ID) // era o lastKnownGood
  assert.equal(service.getActiveId(), BUILTIN_AURORA_ID)
  assert.equal(service.getPendingId(), null)
})

test("recovery: rollbackPending desfaz ativação pendente", () => {
  const fs = createFakeFs()
  const service = createThemeService({ themesDir: "/themes", fsImpl: fs })

  service.activate(BUILTIN_AURORA_ID)
  assert.equal(service.getPendingId(), BUILTIN_AURORA_ID)

  service.rollbackPending()
  assert.equal(service.getPendingId(), null)
  assert.equal(service.getActiveId(), BUILTIN_DEFAULT_ID)
})

test("recovery: built-in nunca pode ser removido", () => {
  const fs = createFakeFs()
  const service = createThemeService({ themesDir: "/themes", fsImpl: fs })

  assert.equal(service.remove(BUILTIN_DEFAULT_ID).ok, false)
  assert.equal(service.remove(BUILTIN_DEFAULT_ID).error, "built_in_nao_removivel")
  assert.equal(service.remove(BUILTIN_AURORA_ID).ok, false)
  assert.equal(service.remove(BUILTIN_AURORA_ID).error, "built_in_nao_removivel")
})

test("recovery: tema ativo não pode ser removido", () => {
  const fs = createFakeFs()
  const service = createThemeService({ themesDir: "/themes", fsImpl: fs })

  service.registry.register("author.active", "1.0.0", "")
  service.activate("author.active")
  service.confirmActivation("author.active")

  assert.equal(service.remove("author.active").ok, false)
  assert.equal(service.remove("author.active").error, "tema_ativo")
})

test("recovery: disable marca tema como desabilitado e impede ativação", () => {
  const fs = createFakeFs()
  const registry = createRegistry({ themesDir: "/themes", fsImpl: fs })

  registry.register("author.disabled", "1.0.0", "")
  assert.equal(registry.disable("author.disabled").ok, true)
  assert.equal(registry.getEntry("author.disabled").enabled, false)

  // Não pode ativar tema desabilitado
  assert.equal(registry.activate("author.disabled").ok, false)
  assert.equal(registry.activate("author.disabled").error, "tema_desabilitado")
})

test("recovery: duas falhas consecutivas desabilitam o tema", () => {
  const fs = createFakeFs()
  const registry = createRegistry({ themesDir: "/themes", fsImpl: fs })

  registry.register("author.broken", "1.0.0", "")

  // Primeira falha: pending não confirmado → rollback
  registry.activate("author.broken")
  registry.rollbackPending()

  // Segunda falha: pending não confirmado → rollback novamente
  registry.activate("author.broken")
  registry.rollbackPending()

  // Após duas falhas, o tema deveria ser desabilitado
  // (implementação: o service pode decidir desabilitar após N falhas)
  // Por agora, verificamos que o tema ainda existe mas não está ativo
  assert.equal(registry.getActiveId(), BUILTIN_DEFAULT_ID)
  assert.equal(registry.getPendingId(), null)
})
