// Testes do REMOVER de um jogo adicionado com o SLSsteam ativo.
//
// Medido no sistema real (HOME isolado): o botão Remover chama
// store:removeFromLibrary, que tirava o jogo da biblioteca mas deixava a
// injeção no config.yaml/stplug-in do SLSsteam intacta. status().adicionados
// (que alimenta o "Na biblioteca" dos cards) continuava listando o appid, então
// o remove parecia não funcionar. E os handlers de remoção na Steam só agiam
// quando o remove na Steam dava ok — o lado do Arcadia ficava refém disso.
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const HOME = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-remove-home-"))
const DATA = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-remove-data-"))
process.env.HOME = HOME
process.env.ARCADIA_DATA_DIR = DATA

const root = path.join(__dirname, "..")
const ler = (rel) => fs.readFileSync(path.join(root, rel), "utf8")
const steamstore = require("../electron/steamstore.js")

test.after(() => {
  fs.rmSync(HOME, { recursive: true, force: true })
  fs.rmSync(DATA, { recursive: true, force: true })
})

function injetar(appid) {
  const steam = path.join(HOME, ".steam", "steam")
  const stplug = path.join(steam, "config", "stplug-in")
  fs.mkdirSync(path.join(steam, "steamapps"), { recursive: true })
  fs.mkdirSync(stplug, { recursive: true })
  fs.mkdirSync(path.join(HOME, ".config", "SLSsteam"), { recursive: true })
  fs.writeFileSync(
    path.join(HOME, ".config", "SLSsteam", "config.yaml"),
    [
      "AdditionalApps:",
      `  - ${appid}`,
      "AppTokens:",
      `  ${appid}: tok`,
      "DlcData:",
      `  ${appid}:`,
      '    999: "DLC"',
      "",
    ].join("\n"),
  )
  fs.writeFileSync(path.join(stplug, `${appid}.lua`), "addappid(999)\n")
}

test("removeFromSteam limpa a injeção: status.adicionados esvazia", async () => {
  injetar("12345")
  assert.ok(steamstore.appidsInjetados().has("12345"), "pré-condição: appid injetado")

  const r = steamstore.removeFromSteam("12345")

  assert.equal(r.ok, true, "removeFromSteam devolve ok")
  assert.equal(steamstore.appidsInjetados().has("12345"), false, "sai do config.yaml/stplug-in")
  assert.equal(
    (await steamstore.status()).adicionados.includes("12345"),
    false,
    "status().adicionados (fonte do 'Na biblioteca') deixa de listar",
  )
  const y = fs.readFileSync(path.join(HOME, ".config", "SLSsteam", "config.yaml"), "utf-8")
  assert.ok(!/^\s*-\s*12345\s*$/m.test(y), "AdditionalApps sem o appid")
  assert.ok(
    !fs.existsSync(path.join(HOME, ".steam", "steam", "config", "stplug-in", "12345.lua")),
    "lua da injeção removido",
  )
})

test("removeFromSteam de jogo que não está na Steam não é erro", () => {
  const r = steamstore.removeFromSteam("987654")
  assert.equal(r.ok, true, "appid ausente devolve ok (nada a limpar)")
})

test("removeFromLibrary limpa a injeção (senão o card fica 'Na biblioteca')", () => {
  const bloco = blocoHandler(ler("electron/main.js"), "store:removeFromLibrary")
  assert.ok(bloco.includes("appidsInjetados"), "só limpa quem está injetado")
  assert.ok(bloco.includes("steamstore.removeFromSteam(appid)"), "remove a injeção ao remover da biblioteca")
  assert.ok(bloco.includes("ownedRemove"), "e sempre tira a posse do Arcadia")
  assert.ok(bloco.includes("aviso"), "devolve aviso quando a limpeza na Steam falha")
})

test("remoção do Arcadia não fica refém do resultado da Steam", () => {
  const main = ler("electron/main.js")
  for (const canal of ["store:removeFromSteam", "store:removeDownloaded"]) {
    const bloco = blocoHandler(main, canal)
    assert.ok(bloco.includes("ownedRemove"), `${canal}: tira da posse do Arcadia`)
    assert.ok(bloco.includes("removerStubPendente"), `${canal}: remove o stub pendente`)
    assert.ok(bloco.includes("setOverride"), `${canal}: marca hidden (não reaparece)`)
    assert.ok(bloco.includes("agendarPush"), `${canal}: sincroniza a remoção com a conta`)
    assert.ok(bloco.includes("avisarBiblioteca"), `${canal}: avisa a UI`)
    assert.doesNotMatch(bloco, /^\s*if \(r\?\.ok\)/m, `${canal}: nada de gate no r.ok da Steam`)
    assert.match(
      bloco,
      /try\s*\{\s*r = steamstore\.remove(FromSteam|Downloaded)\(appid\)/,
      `${canal}: falha/erro da Steam é capturado, não derruba o handler`,
    )
  }
})

test("a UI mostra o aviso do que ficou na Steam", () => {
  const ui = ler("src/components/desktop/LibraryView.tsx")
  const bloco = ui.slice(ui.indexOf("onRemover={() =>"), ui.indexOf("onConfig={", ui.indexOf("onRemover={() =>")))
  assert.ok(bloco.includes("storeRemoveFromLibrary"), "continua removendo pela biblioteca")
  assert.ok(bloco.includes("aviso") && bloco.includes("window.alert(aviso)"), "aviso curto quando a Steam falha")
  assert.ok(bloco.includes("actions.refresh"), "e a biblioteca recarrega")
})

function blocoHandler(main, canal) {
  const inicio = main.indexOf(`ipcMain.handle("${canal}"`)
  assert.ok(inicio > 0, `handler ${canal} presente no main`)
  const proximo = main.indexOf("\n  ipcMain.handle(", inicio)
  return main.slice(inicio, proximo > inicio ? proximo : main.length)
}
