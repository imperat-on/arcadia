// Escala de UI: uma chave só. Estes testes fixam o comportamento que garante
// que a troca das chaves antigas NÃO muda o tamanho da interface na tela.
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")

const {
  BASE_POR_SKIN,
  clampRelativo,
  fatorDeZoom,
  migrarEscala,
  CHAVE_MIGRADA,
} = require("../electron/ui-scale")

test("padrão mantém exatamente o fator de antes (desktop 1.2, console 1.3)", () => {
  assert.equal(fatorDeZoom("desktop", 1), 1.2)
  assert.equal(fatorDeZoom("console", 1), 1.3)
  // o desconhecido nunca explode: cai no desktop
  assert.equal(fatorDeZoom("qualquer-coisa", undefined), 1.2)
})

test("a escala da janela multiplica a base (mesma conta de antes)", () => {
  assert.equal(fatorDeZoom("desktop", 1, 1.25), 1.5)
  assert.equal(fatorDeZoom("console", 1, 1.25), 1.625)
  // valor inválido de escala da janela é neutro
  assert.equal(fatorDeZoom("desktop", 1, 0), 1.2)
  assert.equal(fatorDeZoom("desktop", 1, NaN), 1.2)
})

test("relativo é preso na faixa 0.7–1.6 e inválido vira 1", () => {
  assert.equal(clampRelativo(5), 1.6)
  assert.equal(clampRelativo(0.1), 0.7)
  assert.equal(clampRelativo("1.25"), 1.25)
  assert.equal(clampRelativo(0), 1)
  assert.equal(clampRelativo(-3), 1)
  assert.equal(clampRelativo(undefined), 1)
  assert.equal(clampRelativo("abc"), 1)
})

test("o fator final fica dentro de 0.7–2", () => {
  assert.equal(fatorDeZoom("console", 1.6, 1.55), 2)
  assert.equal(fatorDeZoom("desktop", 0.7, 0.5), 0.7)
})

test("migração: sem chave antiga, tudo vira 1 e os marcadores somem", () => {
  const patch = migrarEscala({ ui_scale: 1.1, desktop_font_scale_v3: false })
  assert.ok(patch, "deve migrar na primeira vez")
  assert.equal(patch[CHAVE_MIGRADA], true)
  assert.equal(patch.ui_scale, 1, "ui_scale antigo era decorativo: volta para 1")
  assert.equal(patch.console_ui_scale, undefined, "chave legada é removida")
  assert.equal(patch.desktop_font_scale_v3, undefined, "marcador é removido")
  assert.equal(patch.big_picture_scale_defaults_v3, undefined)
  // e o fator final é o mesmo de antes da migração
  assert.equal(fatorDeZoom("desktop", patch.ui_scale), 1.2)
})

test("migração: console customizado vira o relativo e preserva o tamanho", () => {
  const patch = migrarEscala({ console_ui_scale: 1.5 })
  assert.equal(Number(patch.ui_scale.toFixed(4)), Number((1.5 / 1.3).toFixed(4)))
  assert.equal(
    Number(fatorDeZoom("console", patch.ui_scale).toFixed(4)),
    1.5,
    "o Big Picture continua do mesmo tamanho que estava",
  )
})

test("migração: rodar duas vezes é no-op", () => {
  assert.equal(migrarEscala({ [CHAVE_MIGRADA]: true, console_ui_scale: 1.5 }), null)
  assert.equal(migrarEscala({ [CHAVE_MIGRADA]: true }), null)
})

test("migração: config vazio/estranho não quebra", () => {
  assert.equal(migrarEscala(undefined).ui_scale, 1)
  assert.equal(migrarEscala(null).ui_scale, 1)
  assert.equal(migrarEscala("lixo").ui_scale, 1)
  assert.equal(migrarEscala({ console_ui_scale: "abc" }).ui_scale, 1)
  assert.equal(migrarEscala({ console_ui_scale: -1 }).ui_scale, 1)
})

test("BASE_POR_SKIN é a única fonte das bases", () => {
  assert.deepEqual(BASE_POR_SKIN, { console: 1.3, desktop: 1.2 })
})
