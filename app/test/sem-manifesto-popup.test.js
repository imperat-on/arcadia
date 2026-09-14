"use strict"

// Catraca do popup "sem manifesto" e do aviso fixo da loja.
//
// São duas coisas diferentes e as duas precisam continuar certas:
//  1. o popup (console) é UM só, compartilhado por "baixar" (não dá para baixar
//     pelo Arcadia) e "adicionar" (o jogo ENTROU na biblioteca, mas não foi
//     injetado na Steam) — sem distinguir, quem clica em Adicionar lê o texto do
//     download e conclui que nada aconteceu;
//  2. o aviso FIXO (página do jogo) é onde se explica o estado "integração ligada
//     sem a chave do Hubcap": é ele que avisa, e não um toast a cada clique.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.join(__dirname, "..")
const ler = (rel) => fs.readFileSync(path.join(root, rel), "utf8")

test("cada fluxo avisa a ação ao popup", () => {
  const acoes = ler("src/components/useStoreActions.ts")
  assert.match(acoes, /semManifestoRef\.current\(jogo, motivo, "baixar"\)/)
  assert.ok(
    /acao: "adicionar" \| "baixar"/.test(acoes),
    "a tipagem do gancho carrega as ações que o popup sabe explicar",
  )
})

test("o popup escolhe o texto pela ação", () => {
  const ps5 = ler("src/components/ps5-launcher/PS5Launcher.tsx")
  assert.ok(ps5.includes('semManifesto.acao === "adicionar"'), "texto próprio do Add")
  assert.ok(ps5.includes("ps5.sem_manifesto.explicacao"), "texto próprio do Baixar")
  assert.ok(
    /acao\?: "adicionar" \| "baixar"/.test(ps5),
    "o estado do popup carrega as duas ações",
  )
})

test("o aviso tem cara de aviso (âmbar) em TODAS as telas com Add", () => {
  for (const rel of [
    "src/components/desktop/StoreView.tsx",
    "src/components/desktop/HomeView.tsx",
    "src/components/ps5-launcher/PS5Launcher.tsx",
  ]) {
    const fonte = ler(rel)
    assert.ok(fonte.includes("ehAviso("), `${rel}: o toast precisa distinguir aviso de sucesso`)
    assert.ok(fonte.includes("amber"), `${rel}: o aviso aparece em âmbar`)
  }
  const ps5 = ler("src/components/ps5-launcher/PS5Launcher.tsx")
  assert.ok(ps5.includes("acoesLoja.toast"), "o console mostra o aviso das ações da loja")
})

test("o texto do Add diz que o jogo entrou na biblioteca, nos 3 idiomas", () => {
  for (const lang of ["pt-BR", "en-US", "es-ES"]) {
    const d = JSON.parse(ler(`src/i18n/${lang}.json`))
    const texto = d["ps5.sem_manifesto.adicionado"]
    assert.ok(typeof texto === "string" && texto.length > 20, `${lang} sem o texto do Add`)
    assert.ok(texto.includes("{motivo}"), `${lang}: o motivo tem que aparecer`)
    assert.match(
      texto,
      /biblioteca|library/i,
      `${lang}: o texto precisa dizer que o jogo ENTROU na biblioteca`,
    )
  }
})

test("o aviso fixo diz o estado e nada além disso, nos 3 idiomas", () => {
  for (const lang of ["pt-BR", "en-US", "es-ES"]) {
    const d = JSON.parse(ler(`src/i18n/${lang}.json`))
    const titulo = d["loja.aviso_sem_chave_titulo"]
    const texto = d["loja.aviso_sem_chave_texto"]
    assert.match(titulo, /Hubcap/, `${lang}: o título nomeia a chave que falta`)
    assert.match(
      texto,
      /biblioteca|library/i,
      `${lang}: o texto diz onde o jogo entra sem a chave`,
    )
    // Texto curto, por pedido explícito do dono: sem instruções de "onde colar".
    assert.ok(texto.length < 70, `${lang}: o texto do aviso fixo é curto`)
  }
})
