"use strict"

// Regressão: a Loja Retro mostrava o mesmo jogo várias vezes só porque a ISO
// mudava de região (`007: Agent Under Fire` + `007 - Agent Under Fire (Korea)`)
// ou de release (`(Taikenban)`, `(v1.20)`).
//
// Os títulos abaixo vêm do catálogo real servido pelo backend (espelho em
// %LOCALAPPDATA%\arcadia\catalog_espelho). Sem o colapso, a PS2 da amostra tinha
// 96 registros para ~76 jogos, e os cartões repetidos eram o sintoma reportado.

const test = require("node:test")
const assert = require("node:assert/strict")

const { collapseVariants, groupVariants, displayKey, variantIds } = require("../electron/retro-variants")

const PS2 = "sony-playstation-2"

const catalogo = (games) => games.map((game) => ({ systemId: PS2, ...game }))

// Extraído do espelho: um registro LaunchBox com as ofertas agregadas e o
// registro criado a partir da release coreana da mesma ISO.
const amostra = catalogo([
  {
    id: `retro:${PS2}:launchbox:4759`,
    title: "007: Agent Under Fire",
    offerCount: 4,
    matchQuality: "strong",
    artwork: { cover: "https://example.test/eua.jpg" },
    aliases: ["007 - Agent Under Fire (USA)", "007 Agent Under Fire", "Agent Under Fire"],
  },
  {
    id: `retro:${PS2}:slug:44ad4a28b0dd`,
    title: "007 - Agent Under Fire (Korea)",
    offerCount: 1,
    matchQuality: "strong",
    artwork: { cover: "https://example.test/korea.jpg" },
    aliases: [],
  },
  {
    id: `retro:${PS2}:launchbox:4757`,
    title: "007: Nightfire",
    offerCount: 3,
    matchQuality: "strong",
    artwork: { cover: "https://example.test/nightfire.jpg" },
    aliases: [],
  },
  {
    id: `retro:${PS2}:slug:15111c924f8c`,
    title: "007 - Nightfire (Korea)",
    offerCount: 1,
    matchQuality: "strong",
    artwork: { cover: "https://example.test/nightfire-kr.jpg" },
    aliases: [],
  },
])

test("colapsa as variantes regionais e soma as ofertas", () => {
  const games = collapseVariants(amostra)

  assert.equal(games.length, 2, "uma ficha por jogo, não por ISO")

  const agent = games.find((game) => game.title === "007: Agent Under Fire")
  assert.equal(agent.offerCount, 5, "4 do LaunchBox + 1 da ISO coreana")
  assert.equal(
    agent.id,
    `retro:${PS2}:launchbox:4759`,
    "a ficha canônica vence o jogo promovido de oferta",
  )
  assert.equal(agent.artwork.cover, "https://example.test/eua.jpg")

  const nightfire = games.find((game) => game.title === "007: Nightfire")
  assert.equal(nightfire.offerCount, 4)
})

test("preserva o título da variante como alias, para a busca continuar achando", () => {
  const [agent] = collapseVariants(amostra)
  assert.ok(
    agent.aliases.includes("007 - Agent Under Fire (Korea)"),
    "buscar por 'Korea' precisa continuar encontrando o jogo",
  )
  assert.ok(agent.aliases.includes("Agent Under Fire"), "aliases originais sobrevivem")
  assert.ok(!agent.aliases.includes("007: Agent Under Fire"), "o título do vencedor não vira alias")
})

test("sem variantes, o jogo passa intacto e na mesma ordem", () => {
  const unicos = catalogo([
    { id: `retro:${PS2}:launchbox:1`, title: "Ace Combat 04: Shattered Skies", offerCount: 2 },
    { id: `retro:${PS2}:launchbox:2`, title: "Ace Combat 5: The Unsung War", offerCount: 1 },
  ])
  const colapsado = collapseVariants(unicos)
  assert.deepEqual(colapsado, unicos, "ordem e conteúdo preservados quando não há duplicata")
})

test("nome parecido não é o mesmo jogo", () => {
  const games = collapseVariants(
    catalogo([
      { id: `retro:${PS2}:launchbox:4759`, title: "007: Agent Under Fire", offerCount: 4 },
      { id: `retro:${PS2}:launchbox:4761`, title: "007: Agent Under Fire 2", offerCount: 1 },
      { id: `retro:${PS2}:launchbox:4758`, title: "007: From Russia with Love", offerCount: 5 },
    ]),
  )
  assert.equal(games.length, 3)
})

test("mesmo título em sistemas diferentes continua separado", () => {
  const games = collapseVariants([
    { id: "retro:sony-playstation-2:launchbox:1", title: "Bully", systemId: PS2, offerCount: 3 },
    { id: "retro:sony-playstation-3:launchbox:1", title: "Bully", systemId: "sony-playstation-3", offerCount: 2 },
  ])
  assert.equal(games.length, 2)
})

test("falta de acento e pontuação não impede o colapso", () => {
  const games = collapseVariants(
    catalogo([
      { id: `retro:${PS2}:launchbox:9`, title: "Pokémon XD - Gale of Darkness", offerCount: 2 },
      { id: `retro:${PS2}:slug:aa`, title: "Pokemon XD Gale of Darkness (USA)", offerCount: 1 },
    ]),
  )
  assert.equal(games.length, 1)
  assert.equal(games[0].offerCount, 3)
})

test("variantIds devolve as variantes absorvidas do jogo aberto", () => {
  const ids = variantIds(`retro:${PS2}:launchbox:4759`, amostra)
  assert.deepEqual(ids, [`retro:${PS2}:slug:44ad4a28b0dd`])
  assert.deepEqual(variantIds(`retro:${PS2}:launchbox:4757`, amostra), [`retro:${PS2}:slug:15111c924f8c`])
})

test("groupVariants agrupa por identidade de exibição", () => {
  const grupos = [...groupVariants(amostra).values()]
  assert.equal(grupos.length, 2)
  assert.equal(grupos[0].length, 2)
  assert.equal(displayKey(amostra[0]), displayKey(amostra[1]))
  assert.notEqual(displayKey(amostra[0]), displayKey(amostra[2]))
})

// ─── Integração com o catálogo do servidor ──────────────────────────────────
// Os três casos abaixo falhavam antes das correções: o grupo partido sumia da
// grade, o cabeçalho contava registros em vez de jogos, e o detalhe abria com
// menos ofertas do que o número exibido na lista.

const { createRetroServerCatalog } = require("../electron/retro-server-catalog")

const jogo = (title, n) => ({ id: `retro:ps2:launchbox:${n}`, systemId: "ps2", title, offerCount: 1 })

function catalogCom(games, total = games.length, extra = {}) {
  const catalogGet = async () => ({
    ok: true,
    data: { ok: true, games, total, totalGames: total, ...extra },
  })
  return createRetroServerCatalog({
    catalogGet,
    dataDir: require("node:os").tmpdir(),
    sources: { search: null },
  })
}

test("grupo maior que a página continua aparecendo na grade", async () => {
  // Um jogo com 8 releases regionais e limit=5: o grupo não cabe na página.
  // Antes o `break` descartava tudo e o cliente recebia uma página VAZIA —
  // a grade mostrava um buraco no lugar do jogo.
  const variantes = ["USA", "Korea", "Japan", "France", "Germany", "Italy", "Spain", "Australia"]
  const games = variantes.map((regiao, i) => jogo(`Delta (${regiao})`, i + 1))

  const pagina = await catalogCom(games, 8).list({ offset: 0, limit: 5 })

  assert.equal(pagina.games.length, 1, "o jogo aparece, não uma página vazia")
  assert.equal(pagina.games[0].offerCount, 8, "todas as 8 releases contam como download")
  assert.equal(pagina.nextOffset, 8, "os 8 registros foram consumidos")
})

test("página não corta um grupo ao meio nem descarta o que sobrou", async () => {
  // Alpha, Beta e depois um grupo de 3. Com limit=5 cabem Alpha+Beta e sobram
  // 3 slots — mas o grupo precisa entrar inteiro ou não entrar.
  const games = [
    jogo("Alpha", 1), jogo("Beta", 2),
    jogo("Delta (USA)", 3), jogo("Delta (Korea)", 4), jogo("Delta (Japan)", 5),
  ]
  const pagina = await catalogCom(games, 5).list({ offset: 0, limit: 5 })

  assert.equal(pagina.games.length, 3, "Alpha + Beta + Delta inteiro")
  assert.equal(pagina.games[2].offerCount, 3, "as 3 variantes somadas na ficha")
  assert.equal(pagina.nextOffset, 5, "consumiu exatamente os 5 registros")
})

test("totalGames conta jogos, não registros do servidor", async () => {
  // 2 grupos de 2 + 1 solto = 3 fichas para 5 registros.
  const games = [
    jogo("Ace Combat Zero", 1), jogo("Ace Combat Zero (Korea)", 2),
    jogo("Ace Combat 04", 3), jogo("Ace Combat 04 (Taikenban)", 4),
    jogo("Bulletproof", 5),
  ]
  const pagina = await catalogCom(games, 5).list({ offset: 0, limit: 10 })

  assert.equal(pagina.games.length, 3)
  assert.equal(pagina.totalGames, 3, "o cabeçalho da Loja mostra fichas")
  assert.equal(pagina.registros, 5, "o total do servidor continua disponível")
})

test("o total não muda ao paginar", async () => {
  const games = Array.from({ length: 60 }, (_, i) =>
    i % 2 === 0
      ? jogo(`Jogo ${i / 2}`, i)
      : jogo(`Jogo ${(i - 1) / 2} (Korea)`, i),
  )
  const catalogo = catalogCom(games, 60)
  const primeira = await catalogo.list({ offset: 0, limit: 10 })
  const segunda = await catalogo.list({ offset: primeira.nextOffset, limit: 10 })

  assert.equal(primeira.totalGames, segunda.totalGames, "total estável entre páginas")
  assert.equal(primeira.totalGames, 30, "30 jogos em 60 registros pareados")
})
