"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const {
  buildCatalog,
  isEnglishDisplayTitle,
  libretroFilename,
  normalizeTitle,
  releaseTitleCandidates,
  titleFromThumbnailPath,
  isIndividualGame,
} = require("../src/retro-service")

test("normaliza título editorial e rejeita nomes não ingleses na vitrine", () => {
  assert.equal(normalizeTitle("Final Fantasy VII (USA) [ENG].chd"), "Final Fantasy VII")
  assert.equal(normalizeTitle("Final Fantasy X (password: psxroms.pro)"), "Final Fantasy X")
  assert.equal(isEnglishDisplayTitle("Final Fantasy VII"), true)
  assert.equal(isEnglishDisplayTitle("Взломщик кодов для Playstation"), false)
  assert.equal(normalizeTitle("Ace Combat Advance (USA, Europe)"), "Ace Combat Advance")
})

test("nome de thumbnail usa a convenção segura do Libretro", () => {
  assert.equal(libretroFilename("Dungeons & Dragons: Heroes"), "Dungeons _ Dragons_ Heroes")
  assert.equal(titleFromThumbnailPath("Named_Boxarts/Chrono Trigger (USA).png"), "Chrono Trigger (USA)")
  assert.equal(titleFromThumbnailPath("Named_Titles/Chrono Trigger.png"), null)
})

test("gera chaves editoriais conservadoras para releases No-Intro", () => {
  assert.deepEqual(
    releaseTitleCandidates("Diddy Kong Racing (USA) (En,Fr) (Rev 1)"),
    [
      "diddy kong racing",
    ],
  )
  assert.deepEqual(releaseTitleCandidates("Final Fantasy VII"), ["final fantasy vii"])
})

test("remove coleções e mantém somente jogos individuais", () => {
  assert.equal(isIndividualGame("Metal Gear Solid 2"), true)
  for (const title of [
    "Final Fantasy Collection",
    "Tomb Raider Trilogy",
    "Best of PlayStation",
    "[10 in 1] PSone Games",
    "Mega Pack Games",
    "Doom Anthology",
    "2 Disney Games - Disney Sports Skateboarding",
    "2 Game Pack! - Hot Wheels World Race",
    "2 Games in 1 - Brother Bear + The Lion King",
    "2-IN-1 Fun Pack: Madagascar + Shrek 2",
    "4-in-1 Fun Pak",
    "2 Jeux en 1 - Titeuf",
    "Ace Combat Advance (Beta)",
    "Final Fantasy Prototype",
    "Metal Slug Demo",
  ]) assert.equal(isIndividualGame(title), false, title)
})

test("catálogo consolida ofertas, mantém URI fora do jogo e oculta cirílico", () => {
  const catalog = buildCatalog([{ source: { id: "classic", title: "Classics" }, payload: { downloads: [
    { title: "Chrono Trigger (USA)", platform: "snes", uris: ["magnet:?xt=urn:btih:ABC123"] },
    { title: "Chrono Trigger [ENG]", platform: "snes", uris: ["magnet:?xt=urn:btih:DEF456"] },
    { title: "Взломщик кодов", platform: "ps3", uris: ["magnet:?xt=urn:btih:FFFF"] },
  ] } }])
  assert.equal(catalog.games.length, 1)
  assert.equal(catalog.games[0].title, "Chrono Trigger")
  assert.equal(catalog.games[0].offerCount, 2)
  assert.equal("uris" in catalog.games[0], false)
  assert.equal(catalog.offers.length, 2)
  assert.equal(catalog.unmatched.length, 1)
})

test("catálogo usa identidade LaunchBox estável e consolida aliases", () => {
  const canonical = {
    provider: "launchbox",
    providerId: "2240",
    title: "Shadow of the Colossus",
    cover: "https://images.launchbox-app.com/cover.png",
  }
  const catalog = buildCatalog([{ source: { id: "classic", title: "Classics" }, payload: { downloads: [
    { title: "Shadow of the Colossus (USA)", platform: "ps2", uris: ["magnet:?xt=urn:btih:ABC123"] },
    { title: "Wanda and the Colossus [ENG]", platform: "ps2", uris: ["magnet:?xt=urn:btih:DEF456"] },
  ] } }], { canonicalBySystem: new Map([["sony-playstation-2", new Map([
    ["shadow of colossus", canonical],
    ["wanda and colossus", canonical],
  ])]]) })

  assert.equal(catalog.games.length, 1)
  assert.equal(catalog.games[0].id, "retro:sony-playstation-2:launchbox:2240")
  assert.equal(catalog.games[0].title, "Shadow of the Colossus")
  assert.equal(catalog.games[0].artwork.provider, "launchbox")
  assert.equal(catalog.games[0].offerCount, 2)
})

test("oferta com qualificador russo só entra quando resolve para título inglês canônico", () => {
  const canonical = {
    provider: "launchbox",
    providerId: "1387",
    title: "Grand Theft Auto: Vice City",
    cover: "https://images.launchbox-app.com/vice-city.png",
  }
  const catalog = buildCatalog([{ source: { id: "ru", title: "Rutracker" }, payload: { downloads: [
    { title: "Grand Theft Auto: Vice City (GTA VC) [RUS|NTSC] (Официальный перевод)", platform: "ps2", uris: ["magnet:?xt=urn:btih:ABC123"] },
    { title: "Неизвестная игра (Перевод)", platform: "ps2", uris: ["magnet:?xt=urn:btih:DEF456"] },
  ] } }], { canonicalBySystem: new Map([["sony-playstation-2", new Map([
    ["grand theft auto vice city", canonical],
  ])]]) })

  assert.equal(catalog.games.length, 1)
  assert.equal(catalog.games[0].title, "Grand Theft Auto: Vice City")
  assert.equal(catalog.games[0].id, "retro:sony-playstation-2:launchbox:1387")
  assert.equal(catalog.unmatched.length, 1)
  assert.equal(catalog.unmatched[0].reason, "english-title")
})

test("usa arte declarada pela source como fallback sem publicar a URI", () => {
  const catalog = buildCatalog([{ source: { id: "classic", title: "Classics" }, payload: { downloads: [
    {
      title: "Resident Evil 4 (USA)", platform: "ps2",
      uris: ["magnet:?xt=urn:btih:ART123"],
      artwork: {
        cover: "https://cdn.example.test/re4-cover.jpg",
        screenshots: ["https://cdn.example.test/re4-shot.jpg"],
        description: "A source-provided description.",
      },
    },
  ] } }])
  assert.equal(catalog.games.length, 1)
  assert.equal(catalog.games[0].artwork.cover, "https://cdn.example.test/re4-cover.jpg")
  assert.deepEqual(catalog.games[0].artwork.screenshots, ["https://cdn.example.test/re4-shot.jpg"])
  assert.equal(catalog.games[0].artwork.description, "A source-provided description.")
  assert.equal("uris" in catalog.games[0], false)
})
