import test from "node:test"
import assert from "node:assert/strict"
import { createRequire } from "node:module"

const require = createRequire(import.meta.url)
const { createPackagedUpdater } = require("../electron/updater-packaged.js")

const fakeUpdater = () => {
  const listeners = {}
  const up = {
    autoDownload: true,
    autoInstallOnAppQuit: true,
    instalou: false,
    chamouCheck: 0,
    baixou: 0,
    on: (ev, fn) => { (listeners[ev] ||= []).push(fn) },
    emitir: (ev, arg) => (listeners[ev] || []).forEach((fn) => fn(arg)),
    checkForUpdates: async () => {
      up.chamouCheck++
      return { updateInfo: { version: "9.9.9", files: [{ size: 104857600 }] } }
    },
    downloadUpdate: async () => { up.baixou++ },
    quitAndInstall: () => { up.instalou = true },
  }
  return up
}

const base = (over = {}) => ({
  app: { isPackaged: true, getVersion: () => "1.4.1" },
  autoUpdater: fakeUpdater(),
  env: {},
  temAppUpdateYml: true,
  platform: "win32",
  ...over,
})

test("canal: fonte quando nao empacotado", () => {
  const u = createPackagedUpdater(base({ app: { isPackaged: false, getVersion: () => "1.4.1" } }))
  assert.equal(u.canal(), "fonte")
  assert.equal(u.suportado(), false)
  assert.equal(u.estado().fase, "ocioso")
})

test("canal: appimage pela env APPIMAGE (linux)", () => {
  const u = createPackagedUpdater(base({ platform: "linux", env: { APPIMAGE: "/tmp/Arcadia.AppImage" } }))
  assert.equal(u.canal(), "appimage")
  assert.equal(u.suportado(), true)
})

test("canal: linux sem APPIMAGE nunca vira nsis (extraido/movido) (B4)", () => {
  const u = createPackagedUpdater(base({ platform: "linux", temAppUpdateYml: true }))
  assert.equal(u.canal(), "sem_suporte")
  assert.equal(u.suportado(), false)
  assert.equal(u.estado().fase, "sem_suporte")
})

test("canal: portable pela env PORTABLE_EXECUTABLE_FILE (win32)", () => {
  const u = createPackagedUpdater(base({ env: { PORTABLE_EXECUTABLE_FILE: "C:\\\\Arcadia.exe" } }))
  assert.equal(u.canal(), "portable")
  assert.equal(u.suportado(), false)
})

test("canal: zip quando falta o app-update.yml (win32)", () => {
  const u = createPackagedUpdater(base({ temAppUpdateYml: false }))
  assert.equal(u.canal(), "zip")
  assert.equal(u.suportado(), false)
})

test("canal: nsis com app-update.yml (win32)", () => {
  const u = createPackagedUpdater(base())
  assert.equal(u.canal(), "nsis")
  assert.equal(u.suportado(), true)
})

test("checar NAO baixa sozinho, extrai o tamanho e desliga o autoDownload (D4)", async () => {
  const up = fakeUpdater()
  const u = createPackagedUpdater(base({ autoUpdater: up }))
  const r = await u.checar()
  assert.equal(r.ok, true)
  assert.equal(r.disponivel, true)
  assert.equal(r.versao, "9.9.9")
  assert.equal(r.tamanho, 104857600)
  assert.equal(up.autoDownload, false)
  assert.equal(up.autoInstallOnAppQuit, false)
  assert.equal(up.baixou, 0, "nao pode baixar antes do usuario aceitar")
  assert.equal(u.estado().fase, "disponivel")
  assert.equal(u.estado().tamanho, 104857600)
})

test("baixar so roda quando chamado e emite progresso de verdade (M6)", async () => {
  const up = fakeUpdater()
  const vistos = []
  const u = createPackagedUpdater(base({ autoUpdater: up, onChange: (e) => vistos.push(e) }))
  await u.checar()
  assert.equal(up.baixou, 0)
  up.downloadUpdate = async () => {
    up.baixou++
    up.emitir("download-progress", { percent: 42.6 })
    up.emitir("update-downloaded", { version: "9.9.9" })
  }
  const r = await u.baixar()
  assert.equal(r.ok, true)
  assert.equal(up.baixou, 1)
  assert.ok(
    vistos.some((e) => e.fase === "baixando" && e.progresso === 43),
    "download-progress tem que aparecer no estado com o percentual arredondado",
  )
  assert.equal(u.estado().fase, "pronto")
  assert.equal(u.estado().progresso, 100)
})

test("update-downloaded deixa pronto e persiste o pendente (D6)", () => {
  const up = fakeUpdater()
  const salvos = []
  const u = createPackagedUpdater(base({ autoUpdater: up, salvarPendente: (v) => salvos.push(v) }))
  up.emitir("update-downloaded", { version: "9.9.9" })
  assert.equal(u.estado().fase, "pronto")
  assert.equal(u.estado().versaoNova, "9.9.9")
  assert.deepEqual(salvos, ["9.9.9"])
})

test("erro de fundo nao abre dialogo; erro manual abre (M3/N8)", async () => {
  const up = fakeUpdater()
  up.checkForUpdates = async () => { throw new Error("sem rede") }
  const u = createPackagedUpdater(base({ autoUpdater: up }))
  const r = await u.checar()
  assert.equal(r.ok, false)
  assert.equal(u.estado().fase, "erro")
  assert.match(String(u.estado().erro), /sem rede/)
  assert.equal(u.estado().erroDeFundo, true)
  assert.equal(u.estado().erroAcao, null, "erro automatico nao tem retry de acao")
  const r2 = await u.checar({ manual: true })
  assert.equal(r2.ok, false)
  assert.equal(u.estado().erroDeFundo, false)
  assert.equal(u.estado().erroAcao, "checar", "retry de check manual e' outro check")
  up.emitir("error", new Error("boom"))
  assert.equal(u.estado().erroDeFundo, true, "evento error fora de acao e' de fundo")
  assert.equal(u.estado().erroAcao, null)
})

test("jogo rodando adia a checagem automatica; manual ignora (M6/M8)", async () => {
  const up = fakeUpdater()
  const u = createPackagedUpdater(base({ autoUpdater: up, isJogoRodando: () => true }))
  const r = await u.checar()
  assert.equal(r.ok, true)
  assert.equal(r.disponivel, false)
  assert.equal(r.motivo, "jogo_rodando")
  assert.equal(up.chamouCheck, 0)
  assert.equal(u.estado().jogoRodando, true)
  const r2 = await u.checar({ manual: true })
  assert.equal(r2.disponivel, true)
  assert.equal(up.chamouCheck, 1)
})

test("comparacao de versao e numerica, nao lexicografica (M6)", async () => {
  const up = fakeUpdater()
  up.checkForUpdates = async () => ({ updateInfo: { version: "1.4.10", files: [] } })
  const u = createPackagedUpdater(
    base({ autoUpdater: up, app: { isPackaged: true, getVersion: () => "1.4.9" } }),
  )
  assert.equal((await u.checar()).disponivel, true)
  up.checkForUpdates = async () => ({ updateInfo: { version: "1.4.9", files: [] } })
  const u2 = createPackagedUpdater(
    base({ autoUpdater: up, app: { isPackaged: true, getVersion: () => "1.4.9" } }),
  )
  assert.equal((await u2.checar()).disponivel, false)
})

test("ja avisei le do config e a checagem manual reabre (D7/M6)", async () => {
  const up = fakeUpdater()
  const salvos = []
  const u = createPackagedUpdater(
    base({
      autoUpdater: up,
      jaAvisado: (v) => v === "9.9.9",
      salvarJaAvisado: (v) => salvos.push(v),
    }),
  )
  await u.checar()
  assert.equal(u.estado().jaAvisado, true)
  assert.equal(u.marcarJaAvisado("9.9.9").ok, true)
  assert.deepEqual(salvos, ["9.9.9"])
  await u.checar({ manual: true })
  assert.equal(u.estado().jaAvisado, false, "manual mostra mesmo com 'ja avisei'")
})

test("pendente no boot reaproveita o cache e fica pronto (D6)", async () => {
  const up = fakeUpdater()
  up.downloadUpdate = async () => {
    up.baixou++
    up.emitir("update-downloaded", { version: "9.9.9" })
  }
  const u = createPackagedUpdater(base({ autoUpdater: up, pendente: () => "9.9.9" }))
  const r = await u.checar()
  assert.equal(r.disponivel, true)
  assert.equal(up.baixou, 1, "o cache e' revalidado pelo electron-updater, sem nova pergunta")
  assert.equal(u.estado().fase, "pronto")
})

test("update-not-available limpa o pendente (D6)", async () => {
  const up = fakeUpdater()
  const limpou = []
  up.checkForUpdates = async () => {
    up.emitir("update-not-available", { version: "1.4.1" })
    return { updateInfo: { version: "1.4.1", files: [] } }
  }
  const u = createPackagedUpdater(base({ autoUpdater: up, salvarPendente: (v) => limpou.push(v) }))
  const r = await u.checar()
  assert.equal(r.disponivel, false)
  assert.equal(u.estado().fase, "ocioso")
  assert.deepEqual(limpou, [null])
})

test("instalar so depois do pronto e chama quitAndInstall (M6)", async () => {
  const up = fakeUpdater()
  const u = createPackagedUpdater(base({ autoUpdater: up }))
  assert.deepEqual(u.instalar(), { ok: false, erro: "sem_download" })
  await u.checar()
  await u.baixar()
  assert.equal(u.estado().fase, "pronto")
  assert.deepEqual(u.instalar(), { ok: true })
  assert.equal(up.instalou, true)
})

test("falha no download vira erro de acao com retry Baixar (N8)", async () => {
  const up = fakeUpdater()
  const u = createPackagedUpdater(base({ autoUpdater: up }))
  await u.checar()
  up.downloadUpdate = async () => { throw new Error("caiu a rede") }
  const r = await u.baixar()
  assert.equal(r.ok, false)
  assert.equal(u.estado().fase, "erro")
  assert.equal(u.estado().erroAcao, "baixar")
  assert.equal(u.estado().erroDeFundo, false)
})

test("falha no quitAndInstall vira erro de acao com link (N7)", async () => {
  const up = fakeUpdater()
  const u = createPackagedUpdater(base({ autoUpdater: up }))
  await u.checar()
  await u.baixar()
  up.quitAndInstall = () => { throw new Error("sem permissao de escrita") }
  const r = u.instalar()
  assert.equal(r.ok, false)
  assert.equal(u.estado().fase, "erro")
  assert.equal(u.estado().erroAcao, "instalar")
  assert.equal(u.estado().erroDeFundo, false)
})

test("canal nao suportado nao checa e nao quebra", async () => {
  const up = fakeUpdater()
  const u = createPackagedUpdater(base({ autoUpdater: up, env: { PORTABLE_EXECUTABLE_FILE: "x" } }))
  const r = await u.checar()
  assert.equal(r.ok, false)
  assert.equal(r.disponivel, false)
  assert.equal(r.motivo, "canal_nao_suportado")
  assert.equal(u.estado().fase, "sem_suporte")
  assert.equal(up.chamouCheck, 0)
})
