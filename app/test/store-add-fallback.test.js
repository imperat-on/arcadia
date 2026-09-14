"use strict"

// Catraca do "Adicionar" da loja.
//
// O botão se chama "adicionar à biblioteca" e tem que terminar SEMPRE com o jogo
// na biblioteca. O bug que estes testes fixam: com a integração local (SLSsteam)
// ativa, o caminho pedia o manifesto e, quando ele faltava, abortava num
// `return null` — o jogo não entrava na biblioteca NEM na Steam, sem nada
// acontecer além de um toast. Como os provedores de manifesto falham com
// frequência (403/521/sem .manifest), isso deixava o botão inútil na prática.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.join(__dirname, "..")
const fonte = fs.readFileSync(path.join(root, "src", "components", "useStoreActions.ts"), "utf8")

/** O bloco da ação `adicionar` (do início do useCallback até o fechamento dele). */
function blocoAdicionar() {
  const inicio = fonte.indexOf("const adicionar = useCallback(")
  assert.ok(inicio > 0, "adicionar() presente no arquivo")
  const fim = fonte.indexOf("[obterInfo, slsAtivo],", inicio)
  assert.ok(fim > inicio, "fechamento do useCallback presente")
  return fonte.slice(inicio, fim)
}

test("o caminho do componente ativo NUNCA sai sem adicionar à biblioteca", () => {
  const bloco = blocoAdicionar()
  assert.ok(
    bloco.includes("storeAddToLibrary"),
    "sem manifesto / injeção falhando, o jogo tem que entrar na biblioteca mesmo assim",
  )
  assert.ok(
    !bloco.includes("return null"),
    "o aborto silencioso (return null) não pode voltar: era ele que deixava o jogo em lugar nenhum",
  )
})

test("a injeção na Steam continua sendo tentada quando o componente está ativo", () => {
  const bloco = blocoAdicionar()
  assert.ok(bloco.includes("storeAddToSteam"), "com manifesto, injeta na Steam")
  assert.ok(bloco.includes("obterInfo"), "e é o manifesto que decide o caminho")
})

test("quando não dá para injetar, a pessoa recebe o MOTIVO (e não um aviso genérico)", () => {
  const bloco = blocoAdicionar()
  assert.ok(bloco.includes('injecao: "sem_manifesto"'), "marca que faltou manifesto")
  assert.ok(bloco.includes('injecao: "falhou"'), "marca quando a injeção falhou")
  assert.ok(/motivo/.test(bloco), "o motivo viaja junto para a mensagem")
})

test("as mensagens usadas existem nos três catálogos", () => {
  const chaves = [
    "store.adicionado_biblioteca",
    "store.adicionado_sem_injecao",
    "store.adicionado_sem_manifesto",
    "store.falha_adicionar",
    "store.falha_adicionar_detalhe",
    "store.sem_manifesto_motivo",
  ]
  for (const lang of ["pt-BR", "en-US", "es-ES"]) {
    const d = JSON.parse(fs.readFileSync(path.join(root, "src", "i18n", `${lang}.json`), "utf8"))
    for (const k of chaves) {
      assert.ok(typeof d[k] === "string" && d[k].length > 0, `${lang} sem ${k}`)
    }
  }
})

test("a chave do Hubcap não vai na URL (só no header)", () => {
  const fonteSteam = fs.readFileSync(path.join(root, "electron", "steamstore.js"), "utf8")
  const inicio = fonteSteam.indexOf("const PROVEDORES = [")
  const fim = fonteSteam.indexOf("]", inicio)
  const bloco = fonteSteam.slice(inicio, fim)
  // A chave ia em `?api_key=` e ficava registrada em log de proxy/CDN. O certo é
  // só o header `Authorization`, que o `headers: (cfg) =>` do provedor monta.
  const linhasUrl = bloco.split("\n").filter((l) => l.trim().startsWith("url:"))
  for (const l of linhasUrl) {
    assert.ok(!l.includes("api_key"), `chave na URL de novo: ${l.trim()}`)
  }
  assert.ok(bloco.includes('nome: "Hubcap"'), "provedor renomeado para Hubcap (ex-Morrenus)")
})
