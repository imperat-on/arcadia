"use strict"

// Catraca da consolidação de conquistas.
//
// Caso real que originou isto: Hogwarts Legacy (appid 990080) aparecia com 47
// itens e 44 desbloqueadas, quando o jogo tem 45 conquistas e a conta do dono 42.
// Causa: o app leu o jogo primeiro SEM o bin da Steam e criou o schema sintético
// (ach_01, ach_02...); depois veio o schema real (PFA_*), o casamento por título
// gravou o vínculo em aliases, mas os dois itens ficaram na lista — e cada par
// contava dos dois lados.

const test = require("node:test")
const assert = require("node:assert/strict")

const { consolidarItens, normalizar, ligados } = require("../electron/achievements/consolidar")

test("normalizar título ignora acento, caixa e espaço extra", () => {
  assert.equal(normalizar("  Um Seletivo   Começo de Ano "), "um seletivo comeco de ano")
  assert.equal(normalizar("Leite Derramado"), "leite derramado")
  assert.equal(normalizar(undefined), "")
})

test("o caso do Hogwarts Legacy: 47 itens viram 45, 44 desbloqueadas viram 42", () => {
  const itens = [
    // par 1 — sintético + real, mesmo título, ligados pelo alias
    { apiname: "ach_01", title: "Um Seletivo Começo de Ano", achieved: true, aliases: ["PFA_1"] },
    { apiname: "PFA_1", title: "Um Seletivo Começo de Ano", block: 1, bit: 0, achieved: true },
    // par 2 — o vínculo está no remoteApiname do sintético
    { apiname: "PFA_33", title: "Leite Derramado", block: 2, bit: 0, achieved: true },
    { apiname: "ach_37", title: "Leite Derramado", block: 36, bit: 0, achieved: true, remoteApiname: "PFA_33" },
    // 5 conquistas reais que não têm par (45 no total na vida real)
    { apiname: "PFA_2", title: "Outra conquista", block: 1, bit: 1, achieved: true },
    { apiname: "PFA_3", title: "Mais uma", block: 1, bit: 2, achieved: false },
  ]
  const reais = new Set(["PFA_1", "PFA_2", "PFA_3", "PFA_33"])
  const out = consolidarItens(itens, reais)
  assert.equal(out.length, 4, "os dois pares viram um item cada")
  assert.equal(out.filter((i) => i.achieved).length, 3, "desbloqueadas sem contar duas vezes")
})

test("sobrevive o item do schema real, e ele herdou o que o outro sabia", () => {
  const itens = [
    { apiname: "ach_01", title: "X", achieved: true, unlock: 111, aliases: ["PFA_1"], icon: "a" },
    { apiname: "PFA_1", title: "X", block: 1, bit: 0, achieved: false, unlock: 0, percent: 7 },
  ]
  const out = consolidarItens(itens, new Set(["PFA_1"]))
  assert.equal(out.length, 1)
  const unico = out[0]
  assert.equal(unico.apiname, "PFA_1", "o apiname real é o que fica")
  assert.equal(unico.block, 1, "mantém o block/bit do schema real")
  assert.equal(unico.achieved, true, "desbloqueio vale em qualquer espaço de chaves")
  assert.equal(unico.unlock, 111, "o unlock não-zero é preservado")
  assert.equal(unico.percent, 7)
  assert.equal(unico.icon, "a", "ícone herdado de quem tinha")
  assert.deepEqual(unico.aliases, ["ach_01"], "o apiname que saiu vira apelido")
})

test("NÃO funde título igual sem vínculo de apiname", () => {
  const itens = [
    { apiname: "A", title: "Complete the game", achieved: true },
    { apiname: "B", title: "Complete the game", achieved: false },
  ]
  const out = consolidarItens(itens, new Set())
  assert.equal(out.length, 2, "dois achievements diferentes podem ter o mesmo título")
})

test("NÃO funde vínculo de apiname com títulos diferentes", () => {
  const itens = [
    { apiname: "A", title: "Primeira", aliases: ["B"] },
    { apiname: "B", title: "Segunda" },
  ]
  const out = consolidarItens(itens, new Set())
  assert.equal(out.length, 2)
})

test("lista sem duplicatas passa intacta, na mesma ordem", () => {
  const itens = [
    { apiname: "PFA_1", title: "Um", block: 0, bit: 0, achieved: true },
    { apiname: "PFA_2", title: "Dois", block: 0, bit: 1, achieved: false },
  ]
  const out = consolidarItens(itens, new Set(["PFA_1", "PFA_2"]))
  assert.deepEqual(out.map((i) => i.apiname), ["PFA_1", "PFA_2"])
})

test("ligados() reconhece apelido, remoteApiname e apiname", () => {
  assert.ok(ligados({ apiname: "a", aliases: ["b"] }, { apiname: "b" }))
  assert.ok(ligados({ apiname: "a" }, { apiname: "x", remoteApiname: "a" }))
  assert.ok(ligados({ apiname: "a" }, { apiname: "a" }))
  assert.ok(!ligados({ apiname: "a" }, { apiname: "b" }))
})
