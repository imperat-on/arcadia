"use strict"

// Contratos das correções visuais do hub do Big Picture (Game Overview):
//
//  1. layout: a primeira linha do grid do hub precisa dimensionar pelo
//     CONTEÚDO (max-content). Com `auto` num grid de altura fixa, a linha vira
//     a "sobra" e a atividade é centralizada transbordando por cima das linhas
//     seguintes — a descrição grande empurra o botão Instalar/Jogar para baixo
//     e o bloco de progresso (e as conquistas) cobrem o botão.
//
//  2. animação: ↓ abre o hub; ↑ fecha pela MESMA animação inversa. O gesto novo
//     só troca a entrada, o caminho de saída continua sendo closeOverview, que
//     aplica a classe `is-closing` (keyframe ps5OverviewExit).

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")
const read = (name) => fs.readFileSync(path.join(__dirname, "../src", name), "utf8")

test("hub do Big Picture dimensiona a primeira linha pelo conteúdo", () => {
  const css = read("index.css")
  // A linha da atividade/produto nunca fica menor que o conteúdo (por isso
  // `max-content` como mínimo), mas ainda preenche a tela quando sobra espaço.
  assert.match(
    css,
    /\.retro-big-picture \.ps5-overview-main \{[^}]*grid-template-rows:minmax\(max-content, ?1fr\)/,
  )
  // E não volta a ficar flexível/auto, que deixaria a atividade transbordar.
  assert.doesNotMatch(
    css,
    /\.retro-big-picture \.ps5-overview-main \{[^}]*grid-template-rows:(?:auto|1fr);/,
  )
})

test("seta ↑ fecha o hub com a mesma animação inversa do ↓", () => {
  const shell = read("components/ps5-launcher/PS5Launcher.tsx")
  const overview = read("components/ps5-launcher/GameOverview.tsx")
  const nav = read("components/ps5-launcher/useGamepadNav.ts")
  const css = read("index.css")

  // ↓ abre o hub.
  assert.match(shell, /if \(selectedGameRef\.current\) openOverview\(\)/)
  // ↑ (D-pad/analógico) reusa o closeOverview via `onUp` — memoizado para não
  // re-registrar o laço do controle a cada render.
  assert.match(
    shell,
    /const overviewNavExtras = useMemo\(\(\) => \(\{ onUp: closeOverview \}\), \[closeOverview\]\)/,
  )
  assert.match(
    shell,
    /useGamepadNav\(overviewRef, overviewNavActive, closeOverview, false, overviewNavExtras\)/,
  )
  assert.match(nav, /if \(dy < 0 && extras\?\.onUp\)/)
  // ↑ também fecha pelo teclado, mas só quando o hub está no controle.
  assert.match(shell, /event\.key === "Escape" \|\| event\.key === "ArrowUp"/)
  assert.match(shell, /if \(!overviewNavActive\) return/)

  // O fechamento aplica a classe que reverte a animação de entrada.
  assert.match(overview, /closing \? "is-closing" : ""/)
  assert.match(css, /@keyframes ps5OverviewEnter \{ from \{ transform:translateY\(100vh\); \} to \{ transform:translateY\(0\); \} \}/)
  assert.match(css, /@keyframes ps5OverviewExit \{ from \{ transform:translateY\(0\); \} to \{ transform:translateY\(100vh\); \} \}/)
  assert.match(css, /\.arcadia-overview\.is-closing \{ animation:ps5OverviewExit/)
})
