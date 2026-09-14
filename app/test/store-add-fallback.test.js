"use strict"

// Contrato do "Adicionar" da loja (decidido pelo dono, depois de três idas e
// voltas — este é o que vale):
//
//   integração DESLIGADA        -> entra na biblioteca do Arcadia
//   integração LIGADA + chave   -> entra na biblioteca E injeta na Steam
//   integração LIGADA sem chave -> entra na biblioteca, sem injeção e SEM AVISO
//
// O último caso é estado normal, não erro: sem chave o provedor de manifesto é
// pulado e não há como injetar, então o jogo (que é o que a pessoa pediu) entra
// na biblioteca. Quem avisa que falta a chave é o aviso FIXO da página do jogo
// (`AvisoSemChaveHubcap`), não um toast/popup a cada clique.

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

test("integração LIGADA sem chave: entra na biblioteca, sem aviso no clique", () => {
  const bloco = blocoAdicionar()
  assert.ok(
    !bloco.includes("hubcap_api_key"),
    "o Add não olha mais a chave: sem chave ele simplesmente não injeta",
  )
  assert.ok(!bloco.includes('"sem_chave"'), "nada de popup/toast de 'sem chave' no clique")
  assert.ok(
    !bloco.includes("adicionado_sem_chave") && !bloco.includes("sem_chave_hubcap"),
    "as mensagens de bloqueio saíram",
  )
  // Falhou o manifesto (o caso normal sem chave): cai na biblioteca, sem marcar
  // a adição como falha nem carregar motivo para toast de aviso.
  const posManifesto = bloco.indexOf("} else {", bloco.indexOf("obterInfo"))
  const trecho = bloco.slice(posManifesto, bloco.indexOf("} else {\n          r = await paraBiblioteca()"))
  assert.ok(trecho.includes("await paraBiblioteca()"), "sem manifesto, adiciona à biblioteca")
  assert.ok(!trecho.includes("injecao:"), "sem manifesto não é falha marcada")
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
