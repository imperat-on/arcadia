"use strict"

// Catraca dos três estados do "Adicionar" da loja, definidos pelo dono:
//
//   integração DESLIGADA        -> entra na biblioteca do Arcadia (como sempre)
//   integração LIGADA + chave   -> entra na biblioteca E injeta na Steam
//   integração LIGADA sem chave -> NÃO entra em lugar nenhum: avisa e para
//
// O último estado é o que mais engana: sem chave não existe manifesto, e
// "adicionar pela metade" faz a pessoa achar que a injeção na Steam aconteceu.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.join(__dirname, "..")
const ler = (rel) => fs.readFileSync(path.join(root, rel), "utf8")
const fonte = ler("src/components/useStoreActions.ts")

function blocoAdicionar() {
  const inicio = fonte.indexOf("const adicionar = useCallback(")
  assert.ok(inicio > 0, "adicionar() presente no arquivo")
  const fim = fonte.indexOf("[obterInfo, slsAtivo],", inicio)
  assert.ok(fim > inicio, "fechamento do useCallback presente")
  return fonte.slice(inicio, fim)
}

test("integração LIGADA sem chave: avisa e NÃO adiciona nada", () => {
  const bloco = blocoAdicionar()
  const guarda = "if (!cfg?.hubcap_api_key) {"
  assert.ok(bloco.includes(guarda), "o caminho com integração ligada precisa olhar a chave")

  // O trecho da guarda vai até o manifesto: não pode existir biblioteca ali.
  const inicio = bloco.indexOf(guarda)
  const fim = bloco.indexOf("const info = await obterInfo(", inicio)
  assert.ok(fim > inicio, "depois da guarda vem o manifesto")
  const trecho = bloco.slice(inicio, fim)
  assert.match(trecho, /return\b/, "a guarda tem que PARAR o fluxo")
  assert.ok(
    !trecho.includes("storeAddToLibrary"),
    "sem chave o jogo não pode entrar na biblioteca (era o que parecia sucesso)",
  )
  assert.ok(trecho.includes('"sem_chave"'), "o aviso é o do caso sem chave")
  // O toast é o aviso GARANTIDO: antes ele vivia no `else` do popup, então onde
  // havia popup o clique ficava sem aviso nenhum.
  assert.ok(trecho.includes("setToast("), "o aviso tem que sair SEMPRE, não só sem popup")
})

test("integração LIGADA com chave: injeta na Steam e cai para a biblioteca só se falhar", () => {
  const bloco = blocoAdicionar()
  assert.ok(bloco.includes("storeAddToSteam"), "com chave, tenta injetar")
  assert.ok(bloco.includes("obterInfo"), "e é o manifesto que decide o caminho")
  assert.ok(
    bloco.includes("paraBiblioteca = () =>") && bloco.includes("storeAddToLibrary"),
    "o helper é quem cria o stub na biblioteca",
  )
  const posInjecao = bloco.indexOf("storeAddToSteam")
  const posFallback = bloco.indexOf("await paraBiblioteca()", posInjecao)
  assert.ok(
    posFallback > posInjecao,
    "quando a injeção falha, o fallback da biblioteca vem DEPOIS da tentativa",
  )
})

test("integração DESLIGADA: só biblioteca", () => {
  const bloco = blocoAdicionar()
  const posSls = bloco.indexOf("if (slsAtivo) {")
  const posElse = bloco.indexOf("} else {", posSls)
  const ultimoTrecho = bloco.slice(posElse)
  assert.ok(ultimoTrecho.includes("paraBiblioteca"), "sem integração, adiciona à biblioteca")
  assert.ok(
    !ultimoTrecho.includes("storeAddToSteam"),
    "sem integração, nada de injeção na Steam",
  )
})

test("a chave do Hubcap não vai na URL (só no header)", () => {
  const fonteSteam = ler("electron/steamstore.js")
  const inicio = fonteSteam.indexOf("const PROVEDORES = [")
  const fim = fonteSteam.indexOf("]", inicio)
  const bloco = fonteSteam.slice(inicio, fim)
  const linhasUrl = bloco.split("\n").filter((l) => l.trim().startsWith("url:"))
  for (const l of linhasUrl) {
    assert.ok(!l.includes("api_key"), `chave na URL de novo: ${l.trim()}`)
  }
  assert.ok(bloco.includes('nome: "Hubcap"'), "provedor renomeado para Hubcap (ex-Morrenus)")
})

test("as mensagens do bloqueio existem nos três catálogos", () => {
  const chaves = [
    "store.sem_chave_hubcap",
    "store.adicionado_sem_chave",
    "ps5.sem_manifesto.sem_chave",
    "ps5.sem_manifesto.adicionado",
  ]
  for (const lang of ["pt-BR", "en-US", "es-ES"]) {
    const d = JSON.parse(ler(`src/i18n/${lang}.json`))
    for (const k of chaves) {
      assert.ok(typeof d[k] === "string" && d[k].length > 20, `${lang} sem ${k}`)
    }
  }
})
