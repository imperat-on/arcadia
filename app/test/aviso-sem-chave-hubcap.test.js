"use strict"

// Catraca do aviso "integração ligada, mas sem chave do Hubcap".
//
// Esse estado não dá erro: o Add entrega o jogo na biblioteca e segue. Sem o
// aviso, a pessoa acha que a injeção na Steam aconteceu — foi exatamente o
// relato ("adicionei e não injetou, sem aviso nenhum").

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.join(__dirname, "..")
const ler = (rel) => fs.readFileSync(path.join(root, rel), "utf8")

test("o aviso existe e só aparece no estado certo (ligado E sem chave)", () => {
  const fonte = ler("src/components/desktop/AvisoSemChaveHubcap.tsx")
  assert.ok(fonte.includes("hubcap_api_key"), "tem que consultar a chave do Hubcap")
  assert.ok(
    /if \(!ativo \|\| temChave !== false\) return null/.test(fonte),
    "com chave presente, ou integração desligada, o aviso não pode aparecer",
  )
  // getConfig() devolve a chave mascarada; presença é o que importa.
  assert.ok(fonte.includes("getConfig"), "a chave vem do config do app")
})

test("a página do jogo e a lista da loja mostram o aviso", () => {
  const pagina = ler("src/components/desktop/StoreGamePage.tsx")
  assert.ok(
    pagina.includes("<AvisoSemChaveHubcap ativo={slsAtivo} />"),
    "StoreGamePage: é onde o Add é clicado, o aviso tem que estar aqui",
  )
  const lista = ler("src/components/desktop/StoreView.tsx")
  assert.ok(
    lista.includes("<AvisoSemChaveHubcap ativo={slsAtivo} />"),
    "StoreView: ver a situação antes de abrir o jogo",
  )
})

test("as chaves do aviso existem nos três catálogos", () => {
  for (const lang of ["pt-BR", "en-US", "es-ES"]) {
    const d = JSON.parse(ler(`src/i18n/${lang}.json`))
    for (const k of ["loja.aviso_sem_chave_titulo", "loja.aviso_sem_chave_texto"]) {
      assert.ok(typeof d[k] === "string" && d[k].length > 20, `${lang} sem ${k}`)
    }
    // O toast de "entrou só na biblioteca" tem que se anunciar como aviso.
    assert.match(
      d["store.adicionado_sem_manifesto"],
      /^(Aviso|Warning): /,
      `${lang}: mensagem de sucesso disfarçada de aviso`,
    )
  }
})
