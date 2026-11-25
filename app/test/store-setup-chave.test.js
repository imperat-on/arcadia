"use strict"

// Catraca da costura "validar / remover chave do Hubcap".
//
// Três peças precisam continuar ligadas, senão a tela volta a mentir:
// 1. o renderer manda `null` para apagar (o `""` antigo não apagava);
// 2. o main recebe o pedido de validação e sabe usar a chave do disco quando o
//    formulário manda a máscara;
// 3. o módulo da loja expõe o teste, e o main passa a lista de remoção para o
//    writeConfig (que faz merge e traria a chave de volta).

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.join(__dirname, "..")
const ler = (rel) => fs.readFileSync(path.join(root, rel), "utf8")

test("a tela oferece validar e remover, e o remover manda null", () => {
  const tela = ler("src/components/desktop/StoreSetup.tsx")
  assert.ok(tela.includes("storeValidarChaveHubcap"), "validação ligada na tela")
  assert.ok(tela.includes("removerKey"), "existe ação de remover")
  assert.match(tela, /hubcap_api_key: null/, "remover manda null (o vazio não apagava)")
  assert.ok(
    /valor === "" \? null : valor/.test(tela),
    "salvar com o campo limpo também apaga",
  )
})

test("o preload e a tipagem expõem o canal de validação", () => {
  assert.ok(ler("electron/preload.js").includes('invoke("store:validarChaveHubcap"'))
  assert.ok(ler("src/global.d.ts").includes("storeValidarChaveHubcap"))
})

test("o main trata o canal e usa a chave do disco quando vem máscara", () => {
  const main = ler("electron/main.js")
  assert.ok(main.includes('ipcMain.handle("store:validarChaveHubcap"'))
  assert.ok(main.includes('!informada.includes("•")'), "máscara não serve como chave")
  assert.ok(main.includes("cfg.hubcap_api_key"), "cai para a chave salva")
})

test("a remoção chega ao writeConfig (senão o merge ressuscita a chave)", () => {
  const main = ler("electron/main.js")
  assert.ok(
    main.includes("resolverSegredos(cfg, atual, redigirSegredos, SEGREDOS)"),
    "config:set passa pela regra explícita",
  )
  assert.ok(main.includes("writeConfig(cfg, remover)"), "a lista de remoção é entregue")
  assert.match(
    main,
    /function writeConfig\(partial, removerChaves\)/,
    "writeConfig aceita apagar chaves",
  )
})

test("o módulo da loja exporta a validação", () => {
  const ss = ler("electron/steamstore.js")
  assert.ok(ss.includes("async function validarChaveHubcap("))
  assert.ok(ss.includes("validarChaveHubcap,"), "exportado junto do getManifest")
  // Não baixa o zip: cancela o corpo e decide pelo status.
  assert.ok(ss.includes("r.body.cancel") || ss.includes("body && typeof r.body.cancel"))
})

test("os textos das ações existem nos três catálogos", () => {
  const chaves = [
    "store_setup.validar",
    "store_setup.validando",
    "store_setup.remover_chave",
    "store_setup.chave_removida",
    "store_setup.teste_ok",
    "store_setup.teste_invalida",
    "store_setup.teste_indisponivel",
    "store_setup.teste_sem_chave",
  ]
  for (const lang of ["pt-BR", "en-US", "es-ES"]) {
    const d = JSON.parse(ler(`src/i18n/${lang}.json`))
    for (const k of chaves) {
      assert.ok(typeof d[k] === "string" && d[k].length > 2, `${lang} sem ${k}`)
    }
  }
})
