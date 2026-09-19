"use strict"

const { describe, it } = require("node:test")
const assert = require("node:assert")
const {
  calculateMatchScore,
  findBestMatch,
  matchBatch,
  getMatchQuality,
  normalizeTitle,
  fuzzyMatch,
  extractAllSerials,
  CONFIDENCE_THRESHOLD_AUTO,
  CONFIDENCE_THRESHOLD_PROBABLE,
} = require("../electron/retro-matcher")

const PS1 = "sony-playstation"
const PS2 = "sony-playstation-2"

function makeGame(overrides = {}) {
  return {
    id: overrides.id || `game:${overrides.title}`,
    systemId: PS2,
    title: "Game",
    aliases: [],
    serials: [],
    hashes: {},
    regions: [],
    ...overrides,
  }
}

function makeOffer(overrides = {}) {
  return {
    id: overrides.id || `offer:${overrides.normalizedTitle}`,
    normalizedTitle: "Game",
    systemId: PS2,
    serials: [],
    hashes: {},
    region: null,
    ...overrides,
  }
}

describe("retro-matcher: falsos positivos (titulos diferentes com palavras comuns)", () => {
  const pairs = [
    ["Grand Theft Auto: San Andreas", "Grand Theft Auto: Vice City"],
    ["Spider-Man 2", "Spider-Man 3"],
    ["Final Fantasy X", "Final Fantasy X-2"],
    ["Ace Combat 5: The Unsung War", "Ace Combat Zero: The Belkan War"],
    ["Mortal Kombat: Armageddon", "Mortal Kombat: Deadly Alliance"],
    ["God of War II", "God of War"],
    ["Croc 2", "Croc"],
  ]

  for (const [offerTitle, gameTitle] of pairs) {
    it(`nao casa "${offerTitle}" com "${gameTitle}"`, () => {
      const result = calculateMatchScore(makeOffer({ normalizedTitle: offerTitle }), makeGame({ title: gameTitle }))
      assert.ok(
        result.score < CONFIDENCE_THRESHOLD_PROBABLE,
        `score ${result.score} deveria ficar abaixo do piso ${CONFIDENCE_THRESHOLD_PROBABLE}`,
      )
      assert.equal(findBestMatch(makeOffer({ normalizedTitle: offerTitle }), [makeGame({ title: gameTitle })]), null)
    })
  }

  it("numeros diferentes nunca sao o mesmo jogo", () => {
    assert.ok(fuzzyMatch("Spider-Man 2", "Spider-Man 3") < 65)
    assert.ok(fuzzyMatch("Final Fantasy X", "Final Fantasy X-2") < 65)
    assert.ok(fuzzyMatch("Tekken 4", "Tekken 5") < 65)
  })

  it("matchBatch nao casa oferta sem correspondencia", () => {
    const offers = [
      makeOffer({ id: "o1", normalizedTitle: "Mortal Kombat: Deadly Alliance" }),
      makeOffer({ id: "o2", normalizedTitle: "Spider-Man 3" }),
    ]
    const games = [
      makeGame({ id: "g1", title: "Mortal Kombat: Armageddon" }),
      makeGame({ id: "g2", title: "Spider-Man 2" }),
    ]
    const batch = matchBatch(offers, games)
    assert.equal(batch.matches.length, 0)
    assert.equal(batch.unmatched.length, 2)
  })

  it("escolhe o jogo certo quando o catalogo tem os parecidos", () => {
    const offer = makeOffer({ normalizedTitle: "Mortal Kombat: Armageddon" })
    const games = [
      makeGame({ id: "deadly", title: "Mortal Kombat: Deadly Alliance" }),
      makeGame({ id: "armageddon", title: "Mortal Kombat: Armageddon" }),
      makeGame({ id: "deception", title: "Mortal Kombat: Deception" }),
    ]
    const match = findBestMatch(offer, games)
    assert.ok(match)
    assert.equal(match.gameId, "armageddon")
    assert.equal(match.method, "exact-title")
    assert.equal(match.score, 85)
  })
})

describe("retro-matcher: falsos negativos (mesmo jogo com variacoes)", () => {
  const variations = [
    ["James Bond 007: From Russia With Love", "007: From Russia with Love", CONFIDENCE_THRESHOLD_AUTO],
    ["7 Blades / Seven Blades", "7 Blades", CONFIDENCE_THRESHOLD_AUTO],
    ["7 Blades (Seven Blades)", "7 Blades", CONFIDENCE_THRESHOLD_AUTO],
    ["100 Bullets (unreleased)", "100 Bullets", CONFIDENCE_THRESHOLD_AUTO],
    ["Legend of Zelda, The", "The Legend of Zelda", CONFIDENCE_THRESHOLD_AUTO],
    ["AC/DC Live: Rock Band", "AC-DC Live - Rock Band", CONFIDENCE_THRESHOLD_AUTO],
  ]

  for (const [offerTitle, gameTitle, floor] of variations) {
    it(`casa "${offerTitle}" com "${gameTitle}"`, () => {
      const match = findBestMatch(makeOffer({ normalizedTitle: offerTitle }), [makeGame({ title: gameTitle })])
      assert.ok(match, `deveria casar "${offerTitle}" com "${gameTitle}"`)
      assert.ok(match.score >= floor, `score ${match.score} abaixo do piso ${floor}`)
      assert.equal(match.quality === "exact" || match.quality === "strong", true)
    })
  }

  it("titulo curto contido na oferta casa como probable (auditoria), nunca como exact", () => {
    const match = findBestMatch(
      makeOffer({ normalizedTitle: "Bully (Canis Canem Edit)" }),
      [makeGame({ title: "Bully" })],
      { allowProbable: true },
    )
    assert.ok(match)
    assert.equal(match.quality, "probable")
    assert.ok(match.score >= CONFIDENCE_THRESHOLD_PROBABLE && match.score < CONFIDENCE_THRESHOLD_AUTO)
  })

  it("matchBatch consolida variacoes da mesma fonte no mesmo jogo", () => {
    const offers = [
      makeOffer({ id: "a", normalizedTitle: "James Bond 007: Agent Under Fire" }),
      makeOffer({ id: "b", normalizedTitle: "007: Agent Under Fire" }),
      makeOffer({ id: "c", normalizedTitle: "Sky Odyssey" }),
    ]
    const games = [
      makeGame({ id: "agent", title: "007: Agent Under Fire" }),
      makeGame({ id: "sky", title: "Sky Odyssey" }),
    ]
    const batch = matchBatch(offers, games)
    assert.equal(batch.matches.length, 3)
    const byOffer = new Map(batch.matches.map((match) => [match.offerId, match]))
    assert.equal(byOffer.get("a").gameId, "agent")
    assert.equal(byOffer.get("b").gameId, "agent")
    assert.equal(byOffer.get("c").gameId, "sky")
    assert.equal(batch.stats.byMethod.fuzzy, 1)
    assert.equal(batch.stats.byMethod["exact-title"], 2)
  })
})

describe("retro-matcher: serial PlayStation manda quando presente", () => {
  it("serial exato vence titulo completamente diferente", () => {
    const offer = makeOffer({
      normalizedTitle: "Completely Different Release Name",
      serials: ["SLUS-20312"],
    })
    const game = makeGame({ title: "Example Adventure", serials: ["SLUS20312"] })
    const result = calculateMatchScore(offer, game)
    assert.equal(result.score, 100)
    assert.equal(result.method, "serial")
  })

  it("serial normaliza caixa, traco e underscore", () => {
    const offer = makeOffer({ serials: ["slus_20312"] })
    const game = makeGame({ serials: ["SLUS-20312"] })
    assert.equal(calculateMatchScore(offer, game).score, 100)
  })

  it("serial nao cruza sistema", () => {
    const offer = makeOffer({ systemId: PS2, serials: ["SLUS-20312"] })
    const game = makeGame({ systemId: PS1, serials: ["SLUS20312"] })
    const result = calculateMatchScore(offer, game)
    assert.equal(result.score, 0)
    assert.equal(result.method, "none")
  })

  it("matchBatch acha jogo so por serial mesmo com titulo irreconhecivel", () => {
    const offers = [makeOffer({ id: "serial-only", normalizedTitle: "ZZZ Unknown Dump", serials: ["SCUS-94194"] })]
    const games = [makeGame({ id: "gt", title: "Gran Turismo", serials: ["SCUS94194"] })]
    const batch = matchBatch(offers, games)
    assert.equal(batch.matches.length, 1)
    assert.equal(batch.matches[0].gameId, "gt")
    assert.equal(batch.matches[0].score, 100)
    assert.equal(batch.matches[0].quality, "exact")
  })

  it("extractAllSerials encontra serial em caixa baixa e deduplica", () => {
    const serials = extractAllSerials("Tekken 5 slus-20312 / SLUS_20312 (USA)", PS2)
    assert.deepEqual(serials, ["SLUS20312"])
  })
})

describe("retro-matcher: sistema desempata", () => {
  it("mesmo titulo em dois sistemas escolhe o sistema da oferta", () => {
    const offer = makeOffer({ normalizedTitle: "Example Adventure", systemId: PS2 })
    const games = [
      makeGame({ id: "ps1", systemId: PS1, title: "Example Adventure" }),
      makeGame({ id: "ps2", systemId: PS2, title: "Example Adventure" }),
    ]
    const match = findBestMatch(offer, games)
    assert.ok(match)
    assert.equal(match.gameId, "ps2")
  })

  it("nunca cruza sistemas mesmo com titulo identico", () => {
    const offer = makeOffer({ normalizedTitle: "Example Adventure", systemId: PS2 })
    const game = makeGame({ systemId: PS1, title: "Example Adventure" })
    assert.equal(calculateMatchScore(offer, game).score, 0)
  })
})

describe("retro-matcher: empate nao vira melhor silenciosamente", () => {
  const tiedGames = () => [
    makeGame({ id: "zzz", title: "Twin Game" }),
    makeGame({ id: "aaa", title: "Twin Game" }),
  ]

  it("empate rebaixa para probable e fica marcado", () => {
    const offer = makeOffer({ normalizedTitle: "Twin Game" })
    const match = findBestMatch(offer, tiedGames(), { allowProbable: true })
    assert.ok(match)
    assert.equal(match.tied, true)
    assert.equal(match.quality, "probable")
    assert.match(match.evidence, /tie:2/)
  })

  it("empate nao associa automaticamente sem allowProbable", () => {
    const offer = makeOffer({ normalizedTitle: "Twin Game" })
    assert.equal(findBestMatch(offer, tiedGames()), null)
  })

  it("resultado independe da ordem do catalogo", () => {
    const offer = makeOffer({ normalizedTitle: "Twin Game" })
    const forward = findBestMatch(offer, tiedGames(), { allowProbable: true })
    const backward = findBestMatch(offer, [...tiedGames()].reverse(), { allowProbable: true })
    assert.equal(forward.gameId, backward.gameId)
    assert.equal(forward.gameId, "aaa")
  })

  it("matchBatch estavel com catalogo invertido", () => {
    const offers = [makeOffer({ id: "o1", normalizedTitle: "Twin Game" })]
    const forward = matchBatch(offers, tiedGames(), { allowProbable: true })
    const backward = matchBatch(offers, [...tiedGames()].reverse(), { allowProbable: true })
    assert.deepEqual(forward.matches, backward.matches)
    assert.equal(forward.stats.tied, 1)
  })
})

describe("retro-matcher: qualidade e confianca para a UI", () => {
  it("matchBatch devolve score, confidence, quality e tied", () => {
    const offers = [makeOffer({ id: "o1", normalizedTitle: "James Bond 007: Agent Under Fire" })]
    const games = [makeGame({ id: "agent", title: "007: Agent Under Fire" })]
    const batch = matchBatch(offers, games)
    const match = batch.matches[0]
    assert.equal(match.confidence, match.score)
    assert.equal(match.quality, "strong")
    assert.equal(match.tied, false)
    assert.equal(match.method, "fuzzy")
  })

  it("getMatchQuality classifica as faixas", () => {
    assert.equal(getMatchQuality(100), "exact")
    assert.equal(getMatchQuality(95), "exact")
    assert.equal(getMatchQuality(94), "strong")
    assert.equal(getMatchQuality(80), "strong")
    assert.equal(getMatchQuality(79), "probable")
    assert.equal(getMatchQuality(65), "probable")
    assert.equal(getMatchQuality(64), "unmatched")
  })

  it("piso padrao nao aceita probable sem allowProbable", () => {
    const offer = makeOffer({ normalizedTitle: "Bully (Canis Canem Edit)" })
    const games = [makeGame({ title: "Bully" })]
    assert.equal(findBestMatch(offer, games), null)
    const allowed = findBestMatch(offer, games, { allowProbable: true })
    assert.ok(allowed)
    assert.equal(allowed.quality, "probable")
  })

  it("minConfidence respeita o piso pedido", () => {
    const offer = makeOffer({ normalizedTitle: "007: Agent Under Fire" })
    const games = [makeGame({ title: "007: Agent Under Fire" })]
    assert.equal(findBestMatch(offer, games, { minConfidence: 90 }), null)
    assert.ok(findBestMatch(offer, games, { minConfidence: 85 }))
  })
})

describe("retro-matcher: normalizacao", () => {
  it("normalizeTitle remove acento, pontuacao e caixa", () => {
    assert.equal(normalizeTitle("Pokémon: Edição Vermelha!"), "pokemon edicao vermelha")
  })

  it("fuzzyMatch e simetrico e identidade vale 100", () => {
    assert.equal(fuzzyMatch("Final Fantasy VII", "Final Fantasy VII"), 100)
    assert.equal(
      fuzzyMatch("James Bond 007: Agent Under Fire", "007: Agent Under Fire"),
      fuzzyMatch("007: Agent Under Fire", "James Bond 007: Agent Under Fire"),
    )
  })
})
