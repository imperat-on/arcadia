"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const { Readable } = require("node:stream")
const {
  deserializeIndex,
  englishTitle,
  imagePriority,
  imageUrl,
  parseLaunchboxMetadata,
  serializeIndex,
} = require("../src/launchbox-catalog")
const { matchKey } = require("../src/retro-service")

const FIXTURE = `<?xml version="1.0"?>
<LaunchBox>
  <Game>
    <Name>Shadow of the Colossus</Name>
    <ReleaseYear>2005</ReleaseYear>
    <ReleaseType>Released</ReleaseType>
    <DatabaseID>2240</DatabaseID>
    <Platform>Sony Playstation 2</Platform>
  </Game>
  <Game>
    <Name>Образ диска</Name>
    <DatabaseID>9999</DatabaseID>
    <Platform>Sony Playstation 2</Platform>
  </Game>
  <GameAlternateName>
    <AlternateName>Wanda and the Colossus</AlternateName>
    <DatabaseID>2240</DatabaseID>
    <Region>Japan</Region>
  </GameAlternateName>
  <GameImage>
    <DatabaseID>2240</DatabaseID>
    <FileName>back.jpg</FileName>
    <Type>Box - Back</Type>
    <Region>World</Region>
  </GameImage>
  <GameImage>
    <DatabaseID>2240</DatabaseID>
    <FileName>europe.png</FileName>
    <Type>Box - Front</Type>
    <Region>Europe</Region>
  </GameImage>
  <GameImage>
    <DatabaseID>2240</DatabaseID>
    <FileName>world.png</FileName>
    <Type>Box - Front</Type>
    <Region>World</Region>
  </GameImage>
</LaunchBox>`

test("parser LaunchBox produz índice canônico inglês com alias e melhor capa", async () => {
  const parsed = await parseLaunchboxMetadata(Readable.from([FIXTURE]), { matchKey })
  assert.equal(parsed.stats.games, 1)
  assert.equal(parsed.stats.artwork, 1)
  const index = parsed.gamesBySystem.get("sony-playstation-2")
  const canonical = index.get(matchKey("Shadow of the Colossus"))
  const alias = index.get(matchKey("Wanda and the Colossus"))
  assert.equal(canonical.providerId, "2240")
  assert.equal(canonical.title, "Shadow of the Colossus")
  assert.equal(canonical.cover, "https://images.launchbox-app.com/world.png")
  assert.equal(alias, canonical)
  assert.equal(index.has(matchKey("Образ диска")), false)
})

test("índice LaunchBox serializado preserva candidatos sem repetir objetos", async () => {
  const parsed = await parseLaunchboxMetadata(Readable.from([FIXTURE]), { matchKey })
  const payload = serializeIndex(parsed)
  const system = payload.systems["sony-playstation-2"]
  assert.equal(system.games.length, 1)
  assert.equal(system.entries.length, 2)
  const restored = deserializeIndex(payload).gamesBySystem.get("sony-playstation-2")
  assert.equal(restored.get("shadow of colossus").providerId, "2240")
  assert.equal(restored.get("wanda and colossus").providerId, "2240")
})

test("valida títulos, prioridade e filenames de imagem", () => {
  assert.equal(englishTitle("Final Fantasy VII"), true)
  assert.equal(englishTitle("Последняя фантазия"), false)
  assert.ok(imagePriority("Box - Front", "World") > imagePriority("Box - Front", "Europe"))
  assert.equal(imagePriority("Box - Back", "World"), -1)
  assert.equal(imageUrl("cover.png"), "https://images.launchbox-app.com/cover.png")
  assert.equal(imageUrl("../secret.png"), "")
})
