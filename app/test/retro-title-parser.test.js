"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")

const { normalizeRetroTitle, parseRetroTitle } = require("../electron/retro-title-parser")

test("região entre parênteses sai do título canônico", () => {
  for (const region of ["USA", "Europe", "Japan", "World", "BR"]) {
    assert.equal(normalizeRetroTitle(`Sonic the Hedgehog (${region})`), "Sonic the Hedgehog")
  }
})

test("tags de dump e metadados saem do título canônico", () => {
  const raws = [
    "Chrono Trigger [!]",
    "Chrono Trigger (Rev 1)",
    "Chrono Trigger (v1.1)",
    "Chrono Trigger [b]",
    "Chrono Trigger (Beta)",
    "Chrono Trigger (Proto)",
  ]
  for (const raw of raws) {
    assert.equal(normalizeRetroTitle(raw), "Chrono Trigger", raw)
  }
})

test("grupo de release no fim sai do título canônico", () => {
  assert.equal(normalizeRetroTitle("007 First Light-CODEX"), "007 First Light")
  assert.equal(normalizeRetroTitle("007 First Light-RUNE"), "007 First Light")
  assert.equal(normalizeRetroTitle("007 First Light-SKIDROW"), "007 First Light")
  assert.equal(normalizeRetroTitle("Final Vendetta (MULTi11) [FitGirl Repack]"), "Final Vendetta")
  assert.equal(normalizeRetroTitle("Super Metroid (T-En by X)"), "Super Metroid")
})

test("subtítulo legítimo em caixa alta não é confundido com grupo de release", () => {
  assert.equal(normalizeRetroTitle("Armored Core - NEXUS [RUS|NTSC]"), "Armored Core NEXUS")
  assert.equal(normalizeRetroTitle("Sesame Street - ABC (USA)"), "Sesame Street ABC")
  assert.equal(normalizeRetroTitle("Aokana - Four Rhythms Across the Blue - EXTRA2"), "Aokana Four Rhythms Across the Blue EXTRA2")
})

test("artigo inicial não diferencia o título canônico", () => {
  assert.equal(normalizeRetroTitle("The Legend of Zelda (USA)"), "Legend of Zelda")
  assert.equal(normalizeRetroTitle("Legend of Zelda (USA)"), "Legend of Zelda")
})

test("números romanos multi-letra e arábicos convergem", () => {
  assert.equal(normalizeRetroTitle("Final Fantasy VII (USA)"), "Final Fantasy 7")
  assert.equal(normalizeRetroTitle("Final Fantasy 7 (USA)"), "Final Fantasy 7")
  assert.equal(normalizeRetroTitle("Final Fantasy XVI (USA)"), "Final Fantasy 16")
  assert.equal(normalizeRetroTitle("Adventure Island II (USA) (Beta)"), "Adventure Island 2")
  // Palavras que por acaso são romanas válidas (1..50) não viram número.
  assert.equal(normalizeRetroTitle("MIX (USA)"), "MIX")
})

test("romano de uma letra fica intacto para não colidir com sequências distintas", () => {
  // Sem plataforma, "Mega Man X" e "Mega Man 10" são jogos diferentes: não fundir.
  assert.equal(normalizeRetroTitle("Mega Man X (USA)"), "Mega Man X")
  assert.equal(normalizeRetroTitle("Mega Man 10 (USA)"), "Mega Man 10")
  assert.equal(normalizeRetroTitle("Grand Theft Auto V (USA)"), "Grand Theft Auto V")
})

test("acentos e cedilha convergem para ASCII", () => {
  assert.equal(normalizeRetroTitle("Ação Total (BR)"), "Acao Total")
  assert.equal(normalizeRetroTitle("Acao Total (BR)"), "Acao Total")
  assert.equal(normalizeRetroTitle("Pokémon Red (USA)"), "Pokemon Red")
  assert.equal(normalizeRetroTitle("Pokemon Red (USA)"), "Pokemon Red")
})

test("pontuação, espaços múltiplos, & e vs convergem", () => {
  assert.equal(normalizeRetroTitle("Spider-Man   vs.   The Hulk"), "Spider Man versus The Hulk")
  assert.equal(normalizeRetroTitle("Spider Man versus The Hulk"), "Spider Man versus The Hulk")
  assert.equal(normalizeRetroTitle("Conan & Rygar"), "Conan and Rygar")
  assert.equal(normalizeRetroTitle("Conan and Rygar"), "Conan and Rygar")
})

test("abreviações com pontos não perdem o artigo", () => {
  assert.equal(normalizeRetroTitle("A.C.E.: Another Century's Episode [JAP|NTSC]"), "A C E Another Century s Episode")
})

test("títulos crus reais das sources normalizam para o canônico", () => {
  const cases = [
    ["Bully (Canis Canem Edit) [RUS|NTSC] [ViT Company]", "Bully"],
    ["Ace Combat 04: Shattered Skies (Distant Thunder) [ENG|NTSC]", "Ace Combat 04 Shattered Skies"],
    ["7 Blades (Seven Blades) [Multi3|PAL]", "7 Blades"],
    ["Resident Evil 4 (Biohazard 4) (password: psxroms.pro)", "Resident Evil 4"],
    ["1000 Stars (World) (Proto) (Byte-Off 2020) (Aftermarket) (Unl)", "1000 Stars"],
    ["EMPYRE: Dukes of the Far Frontier [FitGirl Repack]", "EMPYRE Dukes of the Far Frontier"],
    ["007 First Light-RUNE", "007 First Light"],
    ["Mortal Shell II v.92935+1300 [Папка игры] (2026)", "Mortal Shell 2"],
    ["Lou’s Lagoon: Deluxe Edition – v1.0.4-43683 + 3 DLCs/Bonuses", "Lou s Lagoon Deluxe Edition"],
    [
      "The Elder Scrolls V: Skyrim Anniversary Edition Free Download (v1.7.104.0.8)",
      "Elder Scrolls V Skyrim Anniversary Edition",
    ],
  ]
  for (const [raw, canonical] of cases) {
    assert.equal(normalizeRetroTitle(raw), canonical, raw)
  }
})

test("parseRetroTitle preserva o título original e entrega o canônico", () => {
  const parsed = parseRetroTitle("The Legend of Zelda (USA) (Rev 1)")
  assert.equal(parsed.originalTitle, "The Legend of Zelda (USA) (Rev 1)")
  assert.equal(parsed.title, "Legend of Zelda")
})
