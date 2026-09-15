const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const metadata = require('../electron/metadata')

// Provedores falsos: nenhum teste deste arquivo toca a rede.
function provedores(over = {}) {
  return {
    steamArt: async (_id, kind) =>
      kind === 'hero'
        ? [{ fonte: 'Steam', url: 'https://steam/hero.jpg', largura: 3840, altura: 1240 }]
        : [],
    sgdbPorSteam: async () => null,
    sgdbSearch: async () => [],
    sgdbArt: async () => [],
    xboxSearch: async () => [{ id: 'x1', titulo: 'Game' }],
    xboxProduto: async () => ({
      Images: [
        { ImagePurpose: 'Poster', Width: 600, Height: 900, Uri: '//cdn/poster' },
        { ImagePurpose: 'BoxArt', Width: 2160, Height: 2160, Uri: '//cdn/box' },
      ],
    }),
    igdbProxy: async () => [],
    ...over,
  }
}

test('sem chave: quadrado do Xbox vem antes da arte promocional da Steam', async () => {
  const r = await metadata.squareArt('steam:1', 'Game', '', provedores())
  assert.deepEqual(r.candidatos.map((c) => c.ajuste), ['quadrado', 'preencher'])
  assert.match(r.candidatos[0].url, /^https:\/\/cdn\/box/)
  assert.equal(r.candidatos[0].largura, 2160)
})

test('com chave: a ficha vem pelo ID da Steam, sem comparar nome', async () => {
  let buscouPorTitulo = 0
  const r = await metadata.squareArt('steam:1', 'Red Dead Redemption 2', 'k', provedores({
    sgdbPorSteam: async (appid) => { assert.equal(appid, '1'); return 7 },
    sgdbSearch: async () => { buscouPorTitulo++; return [] },
    sgdbArt: async (_id, kind, _key, opts) =>
      opts?.dimensions
        ? [{ fonte: 'SteamGridDB', url: 'https://sgdb/sq.jpg', largura: 1024, altura: 1024 }]
        : [{ fonte: 'SteamGridDB', url: 'https://sgdb/hero.jpg', largura: 1920, altura: 620 }],
    xboxSearch: async () => [],
  }))
  assert.equal(buscouPorTitulo, 0, 'ID da Steam resolve, nao precisa procurar por nome')
  assert.equal(r.candidatos[0].ajuste, 'quadrado')
  // Ordem de primeira aparição das classes: quadrado primeiro, resto depois.
  assert.deepEqual([...new Set(r.candidatos.map((c) => c.ajuste))], ['quadrado', 'preencher'])
  assert.equal(r.candidatos[0].url, 'https://sgdb/sq.jpg')
})

test('com chave: sem ficha por ID, cai na busca por titulo tolerante a edicao', async () => {
  let buscouPorTitulo = 0
  const r = await metadata.squareArt('steam:1', 'Hogwarts Legacy', 'k', provedores({
    sgdbSearch: async (titulo) => {
      buscouPorTitulo++
      assert.equal(titulo, 'Hogwarts Legacy')
      return [{ id: 9, titulo: 'Hogwarts Legacy Deluxe Bundle' }]
    },
    sgdbArt: async (_id, _kind, _key, opts) =>
      opts?.dimensions
        ? [{ fonte: 'SteamGridDB', url: 'https://sgdb/sq.jpg', largura: 512, altura: 512 }]
        : [],
  }))
  assert.equal(buscouPorTitulo, 1)
  assert.equal(r.candidatos[0].url, 'https://sgdb/sq.jpg')
})

test('capa retrato entra por ultimo, sem ser descartada por nao ser quadrada', async () => {
  const r = await metadata.squareArt('steam:1', 'Game', '', provedores({
    steamArt: async () => [
      { fonte: 'Steam', url: 'https://steam/600x900.jpg', largura: 600, altura: 900 },
    ],
    xboxSearch: async () => [],
  }))
  assert.deepEqual(r.candidatos.map((c) => c.ajuste), ['capa'])
})

test('IGDB so entra quando nem quadrado nem promocional apareceram', async () => {
  let chamadas = 0
  const arteIgdb = () => {
    chamadas++
    return [{ fonte: 'IGDB', url: 'https://igdb/art.jpg', largura: 3840, altura: 2160 }]
  }
  const semNadaLocal = provedores({
    steamArt: async () => [],
    xboxSearch: async () => [],
    igdbProxy: async () => [{ artworks: [{ image_id: 'a' }] }],
    igdbArtDe: arteIgdb,
  })
  const r1 = await metadata.squareArt('steam:1', 'Game', '', semNadaLocal)
  assert.equal(chamadas, 1)
  assert.deepEqual(r1.candidatos.map((c) => c.ajuste), ['preencher'])

  chamadas = 0
  const comArteLocal = provedores({ igdbArtDe: arteIgdb })
  await metadata.squareArt('steam:1', 'Game', '', comArteLocal)
  assert.equal(chamadas, 0, 'com arte local nao gasta chamada no proxy de terceiros')
})

test('falha de um provedor nao derruba os outros nem vaza o erro bruto', async () => {
  const r = await metadata.squareArt('steam:1', 'Game', 'k', provedores({
    sgdbPorSteam: async () => { throw new Error('chave-secreta-invalida') },
    sgdbSearch: async () => { throw new Error('chave-secreta-invalida') },
  }))
  assert.ok(r.candidatos.length > 0, 'segue com Steam e Xbox')
  assert.doesNotMatch(JSON.stringify(r), /chave-secreta-invalida/)
})

test('jogo sem titulo devolve vazio sem tocar a rede', async () => {
  let chamou = false
  const r = await metadata.squareArt('steam:1', '   ', '', provedores({
    steamArt: async () => { chamou = true; return [] },
  }))
  assert.deepEqual(r.candidatos, [])
  assert.equal(chamou, false)
})

test('steam art informa as dimensoes reais de cada arquivo', async () => {
  const original = global.fetch
  global.fetch = async () => ({ ok: true, status: 200 })
  try {
    const hero = await metadata.steamArt('steam:990080', 'hero')
    assert.deepEqual(
      hero.map((a) => [a.url.split('/').pop(), a.largura, a.altura]),
      [
        ['library_hero.jpg', 1920, 620],
        ['library_hero_2x.jpg', 3840, 1240],
        ['page_bg_generated_v6b.jpg', 1438, 810],
      ],
    )
    const cover = await metadata.steamArt('steam:990080', 'cover')
    assert.deepEqual(
      cover.map((a) => [a.url.split('/').pop(), a.largura, a.altura]),
      [
        ['library_600x900.jpg', 600, 900],
        ['header.jpg', 460, 215],
      ],
    )
  } finally {
    global.fetch = original
  }
})

test('mesmaObra aceita sufixo de edicao e rejeita jogo diferente', () => {
  assert.equal(metadata.mesmaObra('Hogwarts Legacy Deluxe Bundle', 'Hogwarts Legacy'), true)
  assert.equal(
    metadata.mesmaObra('Grand Theft Auto: San Andreas - The Definitive Edition', 'Grand Theft Auto: San Andreas'),
    true,
  )
  assert.equal(metadata.mesmaObra('Eldrynn', 'Elden Ring'), false)
  assert.equal(metadata.mesmaObra('', 'Elden Ring'), false)
})

test('o quadrado tem canal proprio de IPC, sem dimensions magicas', () => {
  const main = fs.readFileSync(path.join(__dirname, '../electron/main.js'), 'utf8')
  const preload = fs.readFileSync(path.join(__dirname, '../electron/preload.js'), 'utf8')
  const tipos = fs.readFileSync(path.join(__dirname, '../src/global.d.ts'), 'utf8')
  assert.match(main, /ipcMain\.handle\("meta:squareArt"/)
  assert.doesNotMatch(main, /kind === "cover" && Array\.isArray\(dimensions\)/)
  assert.match(preload, /searchSquareArt: \(gameId, titulo\)/)
  // O canal do preload precisa ser EXATAMENTE o que o main registra.
  assert.match(preload, /invoke\("meta:squareArt", \{ gameId, titulo \}\)/)
  assert.match(tipos, /searchSquareArt:/)
})

test('o trilho aceita qualquer arte e nunca estica', () => {
  const fonte = fs.readFileSync(
    path.join(__dirname, '../src/components/ps5-launcher/GameRail.tsx'),
    'utf8',
  )
  assert.match(fonte, /searchSquareArt\(game\.id, game\.title\)/)
  assert.doesNotMatch(fonte, /naturalWidth === naturalHeight/, 'nao pode recusar capa retrato')
  assert.match(fonte, /naturalWidth > 0/)
  assert.match(fonte, /IntersectionObserver/)
  const css = fs.readFileSync(path.join(__dirname, '../src/index.css'), 'utf8')
  assert.doesNotMatch(
    css,
    /ps5-cover-art[^{]*\{[^}]*object-fit:\s*(fill|contain)/,
    'nem estica nem deixa barra',
  )
})
