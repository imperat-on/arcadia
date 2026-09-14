"use strict"

// Horas da Steam no servidor: soma entre CONTAS, nunca entre fontes.
//
// A mesma conta aparece em duas fontes (o arquivo local desta máquina e o que já
// subiu), então por (jogo, conta) vale o MAIOR — somar as fontes contaria a mesma
// hora duas vezes, que é exatamente o erro que este módulo existe para impedir.

const test = require("node:test")
const assert = require("node:assert")
const path = require("node:path")

const {
  chaveAppid,
  locaisPorConta,
  mesclarHoras,
  somarPorAppid,
  itensParaEnviar,
} = require(path.join(__dirname, "..", "electron", "steam-horas-servidor.js"))

const CONTA_A = "76561197960265728"
const CONTA_B = "76561198000004541"

test("a chave é o número do appid, venha ele do id da biblioteca ou da loja", () => {
  assert.strictEqual(chaveAppid("steam:990080"), "990080")
  assert.strictEqual(chaveAppid("990080"), "990080")
  assert.strictEqual(chaveAppid(990080), "990080")
  assert.strictEqual(chaveAppid("epic:abc"), "")
  assert.strictEqual(chaveAppid(undefined), "")
})

test("contas DIFERENTES somam: 81h47 + 30h = 111h47", () => {
  const locais = locaisPorConta({ "steam:990080": 4907 }, CONTA_A)
  const mapa = mesclarHoras([{ appid: "990080", steamid: CONTA_B, minutes: 1800 }], locais)
  assert.deepStrictEqual(mapa, {
    990080: { [CONTA_A]: 4907, [CONTA_B]: 1800 },
  })
  assert.strictEqual(somarPorAppid(mapa)[990080], 6707, "soma das contas")
})

test("a MESMA conta em duas fontes não conta duas vezes (maior vence)", () => {
  const locais = locaisPorConta({ "steam:990080": 4907 }, CONTA_A)
  // o servidor já tinha um total maior dessa mesma conta (lido em outro momento)
  const mapa = mesclarHoras([{ appid: "990080", steamid: CONTA_A, minutes: 5000 }], locais)
  assert.strictEqual(somarPorAppid(mapa)[990080], 5000, "vence o maior, não a soma (9907)")
})

test("total menor lido agora não apaga o que já subiu", () => {
  const locais = locaisPorConta({ "steam:990080": 100 }, CONTA_A)
  const mapa = mesclarHoras([{ appid: "990080", steamid: CONTA_A, minutes: 4907 }], locais)
  assert.strictEqual(somarPorAppid(mapa)[990080], 4907)
})

test("jogo sem Steam nenhuma fica de fora (a tela usa o tempo do Arcadia)", () => {
  const mapa = mesclarHoras([], locaisPorConta({}, CONTA_A))
  assert.deepStrictEqual(mapa, {})
})

test("minutos zerados ou negativos não criam linha", () => {
  const mapa = mesclarHoras(
    [
      { appid: "990080", steamid: CONTA_A, minutes: 0 },
      { appid: "990080", steamid: CONTA_B, minutes: -10 },
      { appid: "990080", steamid: "", minutes: 60 },
      { appid: "lixo", steamid: CONTA_A, minutes: 60 },
    ],
    {},
  )
  assert.deepStrictEqual(mapa, {})
})

test("o envio é o TOTAL da conta (absoluto), com a chave numérica", () => {
  const itens = itensParaEnviar({ "steam:990080": 4907, "steam:400": 0 }, CONTA_A)
  assert.deepStrictEqual(itens, [
    { appid: "990080", steamid: CONTA_A, minutes: 4907 },
  ])
  assert.deepStrictEqual(itensParaEnviar({ "steam:990080": 10 }, ""), [], "sem conta, nada sobe")
})

test("a soma final é a mesma que o servidor calcula (mesma regra nos dois lados)", () => {
  // 3 contas, com a CONTA_A repetida em duas fontes
  const locais = locaisPorConta({ "steam:990080": 4907, "steam:220": 120 }, CONTA_A)
  const doServidor = [
    { appid: "990080", steamid: CONTA_A, minutes: 4907 },
    { appid: "990080", steamid: CONTA_B, minutes: 1800 },
    { appid: "990080", steamid: "76561198000009999", minutes: 60 },
  ]
  const somado = somarPorAppid(mesclarHoras(doServidor, locais))
  assert.strictEqual(somado[990080], 4907 + 1800 + 60, "três contas, a repetida contada uma vez")
  assert.strictEqual(somado[220], 120, "jogo só desta máquina continua entrando")
})
