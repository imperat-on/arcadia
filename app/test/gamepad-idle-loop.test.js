"use strict"

// Contrato do loop de gamepad: quando não há controle conectado (ou a janela
// perdeu o foco) o loop NÃO pode continuar rodando a 60fps para sempre —
// ele re-agenda por timer lento, e o unmount cancela frame e timer.
//
// Os dois laços (navegação dos overlays e o trilho do Big Picture) repetem a
// mesma estrutura; o teste cobre os dois arquivos.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.join(__dirname, "..")
const read = (file) => fs.readFileSync(path.join(root, file), "utf8")

const ARQUIVOS = [
  "src/components/ps5-launcher/useGamepadNav.ts",
  "src/components/ps5-launcher/PS5Launcher.tsx",
]

for (const arquivo of ARQUIVOS) {
  test(`${arquivo}: loop ocioso desacelera em vez de queimar 60fps`, () => {
    const src = read(arquivo)
    // Agendador com caminho lento (timer) e caminho ativo (frame).
    assert.match(src, /const schedule = \(delay = 0\) =>/, "agendador presente")
    assert.match(src, /retryTimer = window\.setTimeout\(/, "caminho lento usa timer")
    assert.match(src, /raf = requestAnimationFrame\(loop\)/, "caminho ativo usa frame")
    // Sem gamepad: re-agenda devagar.
    assert.match(src, /if \(!gp\) \{[\s\S]{0,200}?schedule\(250\)/, "sem gamepad desacelera")
    // Sem foco: idem.
    assert.match(src, /hasFocus\(\)\) \{[\s\S]{0,200}?schedule\(250\)|!appFocusedRef\.current\) \{[\s\S]{0,220}?schedule\(250\)/, "sem foco desacelera")
    // Não empilha timers quando um já está pendente.
    assert.match(src, /if \(delay > 0 && retryTimer !== null\) return/, "sem timers empilhados")
  })

  test(`${arquivo}: unmount cancela frame e timer`, () => {
    const src = read(arquivo)
    assert.match(
      src,
      /running = false[\s\S]{0,120}?cancelAnimationFrame\(raf\)[\s\S]{0,160}?clearTimeout\(retryTimer\)/,
      "cleanup completo",
    )
    assert.match(src, /const loop = \(\) => \{\s*if \(!running\) return/, "loop não roda após unmount")
  })
}

test("PS5Launcher mantém o loop pausado durante launch pendente", () => {
  const src = read("src/components/ps5-launcher/PS5Launcher.tsx")
  assert.match(src, /launchPendingRef\.current\) \{[\s\S]{0,220}?schedule\(100\)/, "espera curta durante o IPC")
})
