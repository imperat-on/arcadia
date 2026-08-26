"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const path = require("node:path")
const { createRegistry } = require("../electron/themes/registry")
const { createThemeService } = require("../electron/themes/service")
const {
  BUILTIN_DEFAULT_ID,
  BUILTIN_AURORA_ID,
  REGISTRY_VERSION,
} = require("../electron/themes/constants")

// --- Fake filesystem para testes sem disco ---

function createFakeFs(initial = {}) {
  const files = new Map(Object.entries(initial))
  const dirs = new Set()
  return {
    readFileSync(p) {
      const key = String(p)
      if (!files.has(key)) throw new Error(`ENOENT: ${key}`)
      return files.get(key)
    },
    writeFileSync(p, data) {
      files.set(String(p), typeof data === "string" ? data : String(data))
    },
    renameSync(oldP, newP) {
      const data = files.get(String(oldP))
      files.set(String(newP), data)
      files.delete(String(oldP))
    },
    mkdirSync(p) {
      dirs.add(String(p))
    },
    lstatSync(p) {
      const key = String(p)
      if (!files.has(key) && !dirs.has(key)) throw new Error(`ENOENT: ${key}`)
      return {
        isSymbolicLink: () => false,
        isFile: () => files.has(key),
        isDirectory: () => dirs.has(key),
      }
    },
    readdirSync() {
      return []
    },
    rmSync() {},
    existsSync(p) {
      return files.has(String(p)) || dirs.has(String(p))
    },
    _files: files,
    _dirs: dirs,
  }
}

// --- Registry puro ---

test("registry começa com Default ativo e lastKnownGood Default", () => {
  const reg = createRegistry({ fsImpl: createFakeFs() })
  assert.equal(reg.getActiveId(), BUILTIN_DEFAULT_ID)
  assert.equal(reg.getLastKnownGoodId(), BUILTIN_DEFAULT_ID)
  assert.equal(reg.getPendingId(), null)
})

test("registry lista built-ins como os dois primeiros itens", () => {
  const reg = createRegistry({ fsImpl: createFakeFs() })
  const list = reg.list()
  assert.ok(list.length >= 2)
  assert.equal(list[0].id, BUILTIN_DEFAULT_ID)
  assert.equal(list[0].source, "builtin")
  assert.equal(list[0].active, true)
  assert.equal(list[1].id, BUILTIN_AURORA_ID)
  assert.equal(list[1].source, "builtin")
  assert.equal(list[1].active, false)
})

test("registry activate define pendingId e confirmActivation promove", () => {
  const reg = createRegistry({ fsImpl: createFakeFs() })
  const activateResult = reg.activate(BUILTIN_AURORA_ID)
  assert.equal(activateResult.ok, true)
  assert.equal(reg.getPendingId(), BUILTIN_AURORA_ID)
  assert.equal(reg.getActiveId(), BUILTIN_DEFAULT_ID) // ainda não confirmado

  const confirmResult = reg.confirmActivation(BUILTIN_AURORA_ID)
  assert.equal(confirmResult.ok, true)
  assert.equal(reg.getActiveId(), BUILTIN_AURORA_ID)
  assert.equal(reg.getLastKnownGoodId(), BUILTIN_AURORA_ID)
  assert.equal(reg.getPendingId(), null)
})

test("registry rollbackPending limpa pendingId sem mudar active", () => {
  const reg = createRegistry({ fsImpl: createFakeFs() })
  reg.activate(BUILTIN_AURORA_ID)
  assert.equal(reg.getPendingId(), BUILTIN_AURORA_ID)
  reg.rollbackPending()
  assert.equal(reg.getPendingId(), null)
  assert.equal(reg.getActiveId(), BUILTIN_DEFAULT_ID)
})

test("registry register/remove de tema externo", () => {
  const reg = createRegistry({ fsImpl: createFakeFs() })
  reg.register("author.theme", "1.0.0", "abc123")
  const entry = reg.getEntry("author.theme")
  assert.ok(entry)
  assert.equal(entry.version, "1.0.0")
  assert.equal(entry.digest, "abc123")
  assert.equal(entry.enabled, true)

  // Não pode remover built-in
  assert.equal(reg.remove(BUILTIN_DEFAULT_ID).ok, false)

  // Não pode remover tema ativo
  reg.activate("author.theme")
  reg.confirmActivation("author.theme")
  assert.equal(reg.remove("author.theme").ok, false)
  assert.equal(reg.remove("author.theme").error, "tema_ativo")

  // Troca para Default e remove
  reg.activate(BUILTIN_DEFAULT_ID)
  reg.confirmActivation(BUILTIN_DEFAULT_ID)
  assert.equal(reg.remove("author.theme").ok, true)
  assert.equal(reg.getEntry("author.theme"), null)
})

test("registry disable marca tema como desabilitado", () => {
  const reg = createRegistry({ fsImpl: createFakeFs() })
  reg.register("author.theme", "1.0.0", "")
  assert.equal(reg.disable("author.theme").ok, true)
  assert.equal(reg.getEntry("author.theme").enabled, false)

  // Não pode ativar tema desabilitado
  assert.equal(reg.activate("author.theme").ok, false)
  assert.equal(reg.activate("author.theme").error, "tema_desabilitado")
})

test("registry recoverToLastKnownGood volta ao fallback", () => {
  const reg = createRegistry({ fsImpl: createFakeFs() })
  reg.activate(BUILTIN_AURORA_ID)
  reg.confirmActivation(BUILTIN_AURORA_ID)
  assert.equal(reg.getActiveId(), BUILTIN_AURORA_ID)

  const recovered = reg.recoverToLastKnownGood()
  assert.equal(recovered, BUILTIN_AURORA_ID) // era o lastKnownGood
  assert.equal(reg.getActiveId(), BUILTIN_AURORA_ID)
  assert.equal(reg.getPendingId(), null)
})

test("registry persiste e recarrega do disco", () => {
  const fs = createFakeFs()
  const themesDir = "/test/themes"

  const reg1 = createRegistry({ themesDir, fsImpl: fs })
  reg1.register("author.saved", "2.0.0", "sha256hash")
  reg1.activate("author.saved")
  reg1.confirmActivation("author.saved")

  // Simula reload: limpa cache e recria
  const reg2 = createRegistry({ themesDir, fsImpl: fs })
  assert.equal(reg2.getActiveId(), "author.saved")
  assert.equal(reg2.getLastKnownGoodId(), "author.saved")
  assert.ok(reg2.getEntry("author.saved"))
  assert.equal(reg2.getEntry("author.saved").version, "2.0.0")
})

test("registry rejeita escrita em arquivo symlinkado", () => {
  const fs = createFakeFs()
  const symlinkFs = {
    ...fs,
    lstatSync(p) {
      return {
        isSymbolicLink: () => true,
        isFile: () => false,
        isDirectory: () => false,
      }
    },
  }
  const reg = createRegistry({ themesDir: "/test/themes", fsImpl: symlinkFs })
  // activate tenta salvar → atomicWrite detecta symlink e lança erro
  assert.throws(() => reg.activate(BUILTIN_AURORA_ID), { message: "registro_symlink" })
})

// --- Service ---

test("service lista built-ins e temas instalados", () => {
  const service = createThemeService({ fsImpl: createFakeFs() })
  const list = service.list()
  assert.ok(list.length >= 2)
  assert.equal(list[0].id, BUILTIN_DEFAULT_ID)
  assert.equal(list[1].id, BUILTIN_AURORA_ID)
})

test("service get retorna built-in", () => {
  const service = createThemeService({ fsImpl: createFakeFs() })
  const aurora = service.get(BUILTIN_AURORA_ID)
  assert.ok(aurora)
  assert.equal(aurora.id, BUILTIN_AURORA_ID)
  assert.equal(aurora.source, "builtin")
})

test("service activate + confirmActivation funciona", () => {
  const service = createThemeService({ fsImpl: createFakeFs() })
  assert.equal(service.activate(BUILTIN_AURORA_ID).ok, true)
  assert.equal(service.confirmActivation(BUILTIN_AURORA_ID).ok, true)
  assert.equal(service.getActiveId(), BUILTIN_AURORA_ID)
})

test("service remove rejeita built-in", () => {
  const service = createThemeService({ fsImpl: createFakeFs() })
  assert.equal(service.remove(BUILTIN_DEFAULT_ID).ok, false)
  assert.equal(service.remove(BUILTIN_DEFAULT_ID).error, "built_in_nao_removivel")
})

test("service remove tema externo limpa registro e diretório", () => {
  const fs = createFakeFs()
  const themesDir = "/test/themes"
  const service = createThemeService({ themesDir, fsImpl: fs })

  // Registra um tema externo
  service.registry.register("author.removable", "1.0.0", "")

  // Cria o diretório fake
  const themeDir = path.join(themesDir, "fullscreen", "author.removable")
  fs.mkdirSync(themeDir, { recursive: true })

  assert.equal(service.remove("author.removable").ok, true)
  assert.equal(service.registry.getEntry("author.removable"), null)
})

test("service recoverToLastKnownGood funciona", () => {
  const service = createThemeService({ fsImpl: createFakeFs() })
  service.activate(BUILTIN_AURORA_ID)
  service.confirmActivation(BUILTIN_AURORA_ID)
  const recovered = service.recoverToLastKnownGood()
  assert.equal(recovered, BUILTIN_AURORA_ID)
})
