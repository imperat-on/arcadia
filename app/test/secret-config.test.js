"use strict"

// Catraca da regra de gravação das chaves secretas (auditoria A-06).
//
// O renderer nunca vê a chave real: `config:get` devolve a máscara. A regra
// antiga só sabia dizer "isto é a máscara, mantém o valor real" — não existia
// sinal para APAGAR. Resultado relatado pelo dono: limpar o campo e salvar
// deixava a chave antiga no lugar, e qualquer gravação que reenviasse o
// formulário "ressuscitava" a chave. O primeiro teste aqui reproduz a regra
// antiga para provar que ela realmente não apagava.

const test = require("node:test")
const assert = require("node:assert/strict")

const { resolverSegredos } = require("../electron/secret-config")

const SEGREDOS = ["hubcap_api_key", "steam_api_key"]
const redigir = (cfg) => {
  const out = { ...cfg }
  for (const k of SEGREDOS) {
    const v = out[k]
    if (typeof v === "string" && v) out[k] = v.length > 8 ? v.slice(0, 3) + "•••" + v.slice(-2) : "•••"
  }
  return out
}

test("a máscara órfã NUNCA vira chave — era isso que a deixava 'ativa'", () => {
  // Sem chave real no disco, o formulário reenvia a máscara de um config antigo.
  // A regra antiga comparava com a máscara do valor atual (vazia) e, não batendo,
  // gravava a máscara como se fosse chave: ela "continuava ativa" e a requisição
  // ao Hubcap tomava 401. Aqui ela é descartada, não gravada.
  const r = resolverSegredos({ hubcap_api_key: "abc•••yz" }, {}, redigir, SEGREDOS)
  assert.ok(!("hubcap_api_key" in r.cfg), "máscara não pode virar chave")
  assert.deepEqual(r.remover, [])
})

test("máscara já gravada no disco é lixo e sai na próxima gravação", () => {
  const noDisco = { hubcap_api_key: "abc•••yz", language: "pt-BR" }
  // Outra tela salva outra coisa qualquer: a chave nem aparece no patch.
  const r = resolverSegredos({ language: "en-US" }, noDisco, redigir, SEGREDOS)
  assert.deepEqual(r.remover, ["hubcap_api_key"])
  assert.equal(r.cfg.language, "en-US")
})

test("apagar de verdade: null e string vazia entram na lista de remoção", () => {
  const noDisco = { hubcap_api_key: "chave-real-123456", language: "pt-BR" }

  const comNull = resolverSegredos({ hubcap_api_key: null }, noDisco, redigir, SEGREDOS)
  assert.deepEqual(comNull.remover, ["hubcap_api_key"])
  assert.ok(!("hubcap_api_key" in comNull.cfg), "não pode sobrar a chave no patch")

  const comVazio = resolverSegredos({ hubcap_api_key: "   " }, noDisco, redigir, SEGREDOS)
  assert.deepEqual(comVazio.remover, ["hubcap_api_key"])
  assert.ok(!("hubcap_api_key" in comVazio.cfg))
})

test("a máscara que volta significa 'não mexi' e mantém o valor real", () => {
  const noDisco = { hubcap_api_key: "chave-real-123456" }
  const mascara = redigir(noDisco).hubcap_api_key
  const r = resolverSegredos({ hubcap_api_key: mascara }, noDisco, redigir, SEGREDOS)
  assert.equal(r.cfg.hubcap_api_key, "chave-real-123456")
  assert.deepEqual(r.remover, [])
})

test("uma chave nova é gravada como veio (com trim)", () => {
  const noDisco = { hubcap_api_key: "antiga" }
  const r = resolverSegredos({ hubcap_api_key: "  nova-chave  " }, noDisco, redigir, SEGREDOS)
  assert.equal(r.cfg.hubcap_api_key, "nova-chave")
  assert.deepEqual(r.remover, [])
})

test("chaves que não são segredo passam intactas", () => {
  const r = resolverSegredos(
    { language: "en-US", accent: "#22d3ee" },
    { hubcap_api_key: "x" },
    redigir,
    SEGREDOS,
  )
  assert.equal(r.cfg.language, "en-US")
  assert.equal(r.cfg.accent, "#22d3ee")
  assert.deepEqual(r.remover, [])
})

test("segredo ausente do patch não é removido nem inventado", () => {
  const noDisco = { hubcap_api_key: "x", steam_api_key: "y" }
  const r = resolverSegredos({ language: "pt-BR" }, noDisco, redigir, SEGREDOS)
  assert.deepEqual(r.remover, [])
  assert.ok(!("hubcap_api_key" in r.cfg))
  assert.ok(!("steam_api_key" in r.cfg))
})

test("valor não-texto é ignorado (nada de apagar por acidente)", () => {
  const r = resolverSegredos({ hubcap_api_key: 42 }, { hubcap_api_key: "x" }, redigir, SEGREDOS)
  assert.deepEqual(r.remover, [])
  assert.equal(r.cfg.hubcap_api_key, 42)
})
