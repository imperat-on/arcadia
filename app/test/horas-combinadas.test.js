"use strict"

// Catraca das horas combinadas.
//
// A regra é: o total da Steam MANDA quando existe, porque o jogo do Arcadia abre
// pela Steam e o cliente já conta aquele tempo. O que o Arcadia mediu entra só
// quando a Steam não tem o jogo (crackeado/emulador). SOMAR os dois daria um
// número maior que o cliente mostra — contagem dupla — e é isso que este teste
// impede de voltar por engano.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.join(__dirname, "..")
const ler = (rel) => fs.readFileSync(path.join(root, rel), "utf8")

test("o componente escolhe Steam; Arcadia é o que sobra", () => {
  const fonte = ler("src/components/desktop/HorasNaSteam.tsx")
  assert.ok(fonte.includes("minutosSteam > 0"), "decide pelo valor da Steam")
  assert.ok(
    /const total = daSteam \? minutosSteam :/.test(fonte),
    "Steam manda; Arcadia só quando a Steam não tem",
  )
  assert.ok(
    !/minutosSteam\s*\+\s*(Number\()?minutosArcadia/.test(fonte),
    "somar as duas fontes é contagem dupla — não pode voltar",
  )
})

test("as horas aparecem na loja, na biblioteca e no diálogo de detalhes", () => {
  assert.ok(ler("src/components/desktop/StoreGamePage.tsx").includes("<HorasNaSteam"))
  assert.ok(ler("src/components/desktop/GamePage.tsx").includes("<HorasNaSteam"))
  const dialog = ler("src/components/desktop/GameDetailsDialog.tsx")
  assert.ok(dialog.includes("steamHorasDoJogo"), "o diálogo lê as horas da Steam")
  assert.ok(dialog.includes("minutosSteam > 0"), "e usa a mesma regra")
})

test("os textos existem nos três catálogos", () => {
  for (const lang of ["pt-BR", "en-US", "es-ES"]) {
    const d = JSON.parse(ler(`src/i18n/${lang}.json`))
    for (const k of ["steam_captura.horas_na_steam", "steam_captura.tempo_jogo", "steam_captura.na_steam"]) {
      assert.ok(typeof d[k] === "string" && d[k].length > 3, `${lang} sem ${k}`)
    }
  }
})
