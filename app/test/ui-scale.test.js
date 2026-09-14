// Escala de UI: AUTOMÁTICA, derivada da tela. Estes testes fixam as bordas da
// conta (para a adaptação não virar mágica sem prova) e garantem que o padrão
// continua idêntico ao de antes: escala 1 → fator da skin, capa 1.6.
"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")

const {
  BASE_POR_SKIN,
  REFERENCIA,
  ESCALA_MIN,
  ESCALA_MAX,
  MANUAL_MIN,
  MANUAL_MAX,
  CHAVE_MIGRADA,
  escalaAutomatica,
  escalaManual,
  escalaEfetiva,
  fatorDeZoom,
  escalaDaCapa,
  migrarEscala,
} = require("../electron/ui-scale")

test("padrão continua exatamente o de antes (desktop 1.2, console 1.3)", () => {
  assert.equal(fatorDeZoom("desktop", 1), 1.2)
  assert.equal(fatorDeZoom("console", 1), 1.3)
  // o desconhecido nunca explode: cai no desktop
  assert.equal(fatorDeZoom("qualquer-coisa", undefined), 1.2)
  // e a capa na escala 1 tem o tamanho que sempre teve
  assert.equal(escalaDaCapa(1), 1.6)
})

test("escala automática: a resolução lógica manda, pela MENOR dimensão", () => {
  // 1920x1080 (referência, e também o que um 4K a 200% reporta): fator neutro
  assert.equal(escalaAutomatica({ width: 1920, height: 1080 }), 1)
  // 4K a 150% -> ~2560x1440
  assert.equal(Number(escalaAutomatica({ width: 2560, height: 1440 }).toFixed(4)), 1.3333)
  // 4K puro
  assert.equal(escalaAutomatica({ width: 3840, height: 2160 }), ESCALA_MAX)
  // notebook pequeno / telas de mão: nunca encolhe abaixo do tamanho base
  assert.equal(escalaAutomatica({ width: 1366, height: 768 }), ESCALA_MIN)
  // ultrawide 3440x1440: a altura (menor) é quem limita, senão estouraria
  assert.equal(Number(escalaAutomatica({ width: 3440, height: 1440 }).toFixed(4)), 1.3333)
  assert.equal(escalaAutomatica({ width: 1280, height: 800 }), ESCALA_MIN)
})

test("escala automática: entrada inválida cai na referência, nunca em NaN", () => {
  assert.equal(escalaAutomatica(undefined), 1)
  assert.equal(escalaAutomatica({}), 1)
  assert.equal(escalaAutomatica({ width: 0, height: 0 }), 1)
  assert.equal(escalaAutomatica({ width: "abc", height: null }), 1)
  assert.equal(escalaAutomatica({ width: -5000, height: -5000 }), 1)
})

test("sobreposição por ambiente: só valor válido conta; 'auto' é o automático", () => {
  assert.equal(escalaManual(undefined), null)
  assert.equal(escalaManual(null), null)
  assert.equal(escalaManual(""), null)
  assert.equal(escalaManual("auto"), null)
  assert.equal(escalaManual("AUTO"), null)
  assert.equal(escalaManual("abc"), null)
  assert.equal(escalaManual(0), null)
  assert.equal(escalaManual(-2), null)
  assert.equal(escalaManual("1.25"), 1.25)
  // a faixa da sobreposição é mais larga que a da automática, de propósito
  assert.equal(escalaManual(9), MANUAL_MAX)
  assert.equal(escalaManual(0.1), MANUAL_MIN)
})

test("escala efetiva: o ambiente manda quando existe, senão a tela decide", () => {
  const tela = { width: 2560, height: 1440 }
  assert.equal(Number(escalaEfetiva(tela, undefined).toFixed(4)), 1.3333)
  assert.equal(escalaEfetiva(tela, "1.1"), 1.1)
  assert.equal(escalaEfetiva(tela, "auto"), 1.3333333333333333)
})

test("fator final: base da skin × escala, dentro da faixa", () => {
  assert.equal(fatorDeZoom("console", ESCALA_MAX).toFixed(2), "2.21")
  assert.equal(fatorDeZoom("desktop", ESCALA_MIN), 1.2)
  // piso e teto
  assert.equal(fatorDeZoom("desktop", 0), 1.2, "escala inválida é neutra")
  assert.equal(fatorDeZoom("desktop", NaN), 1.2)
  assert.equal(fatorDeZoom("desktop", 1e6), 2.4)
})

test("capa acompanha a tela e fica na faixa 0.9–1.9", () => {
  const r = (n) => Number(n.toFixed(4))
  assert.equal(r(escalaDaCapa(0.85)), 1.36)
  assert.equal(r(escalaDaCapa(1.3333)), 1.9, "sem trava seria grande demais (2.13)")
  assert.equal(r(escalaDaCapa(1.7)), 1.9)
  assert.equal(r(escalaDaCapa(0.1)), 0.9)
  assert.equal(r(escalaDaCapa(undefined)), 1.6, "sem aviso da tela, o padrão de sempre")
  assert.equal(r(escalaDaCapa(NaN)), 1.6)
})

test("migração: aposenta as preferências de escala e de capa", () => {
  const patch = migrarEscala({ ui_scale: 1.15, card_scale: 1.4, console_ui_scale: 1.5 })
  assert.ok(patch, "deve migrar na primeira vez")
  assert.equal(patch[CHAVE_MIGRADA], true)
  assert.equal(patch.ui_scale, undefined, "escala manual é aposentada")
  assert.equal(patch.card_scale, undefined, "tamanho de capa manual é aposentado")
  assert.equal(patch.console_ui_scale, undefined, "chave legada também")
  // e nada sobra para o config guardar
  const sobrou = Object.values(patch).filter((v) => v !== undefined && v !== true)
  assert.deepEqual(sobrou, [], "só a marcadora sobrevive")
})

test("migração: rodar duas vezes é no-op, e config estranho não quebra", () => {
  assert.equal(migrarEscala({ [CHAVE_MIGRADA]: true, ui_scale: 1.5 }), null)
  assert.equal(migrarEscala(undefined)[CHAVE_MIGRADA], true)
  assert.equal(migrarEscala(null)[CHAVE_MIGRADA], true)
  assert.equal(migrarEscala("lixo")[CHAVE_MIGRADA], true)
  assert.equal(migrarEscala({ card_scale: "abc" })[CHAVE_MIGRADA], true)
})

test("BASE_POR_SKIN é a única fonte das bases", () => {
  assert.deepEqual(BASE_POR_SKIN, { console: 1.3, desktop: 1.2 })
  assert.deepEqual(REFERENCIA, { width: 1920, height: 1080 })
})
