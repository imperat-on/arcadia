"use strict"

// Regressão da tira de mídia do hub do Big Picture (GameOverview).
//
// O bug (corrigido em eae3d94): o useMemo `screenshots` fazia
//   ...(meta?.screenshots || []).flatMap((shot) => [shot.full, shot.thumb])
// e CADA screenshot do metadata virava DOIS itens na tira — um nítido (`full`)
// e um borrado (`thumb`) — sempre em pares: "Imagem 1" e "Imagem 2" mostravam a
// mesma arte. O fix passou a lista para o shape `{ src, full }`: o card usa
// `src` (leve), o clique abre `full` (nítida), e o metadata entra UMA vez via
// `.map`.
//
// Isto é um guard de FORMA, não de render: a suíte é `node --test` puro, sem
// React DOM. Lemos o fonte do componente como texto e travamos a estrutura que
// o fix introduziu — se o flatMap dobrando voltar, o teste quebra.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const COMPONENTE = path.join(
  __dirname,
  "..",
  "src",
  "components",
  "ps5-launcher",
  "GameOverview.tsx",
)
const fonte = fs.readFileSync(COMPONENTE, "utf8")

/** Recorte do fonte entre dois marcadores (o final fica de fora). */
function trecho(inicio, fim) {
  const i = fonte.indexOf(inicio)
  assert.ok(i >= 0, `marcador ausente no GameOverview.tsx: ${inicio}`)
  const j = fonte.indexOf(fim, i)
  assert.ok(j > i, `marcador final ausente no GameOverview.tsx: ${fim}`)
  return fonte.slice(i, j)
}

const SCREENSHOTS = trecho("const screenshots = useMemo(", "const mediaItems = useMemo")
const MEDIA_ITEMS = trecho("const mediaItems = useMemo", "const relatedNews = useMemo")

test("screenshots: metadata entra UMA vez, sem flatMap dobrando [full, thumb]", () => {
  // O padrão exato do bug não pode reaparecer em lugar nenhum do componente.
  assert.doesNotMatch(
    fonte,
    /flatMap\(\s*\(shot\)\s*=>\s*\[\s*shot\.full\s*,\s*shot\.thumb\s*\]\s*\)/,
    "voltou o flatMap que transformava cada screenshot em um par nítido+borrado",
  )
  // E o bloco dos screenshots não usa flatMap de forma alguma: metadata e
  // imagens locais entram cada um UMA vez.
  assert.doesNotMatch(SCREENSHOTS, /\.flatMap\(/)
  // Metadata entra com `.map` direto (uma entrada por screenshot).
  assert.match(SCREENSHOTS, /\(meta\?\.screenshots \|\| \[\]\)\s*\.map\(/)
})

test("screenshots: shape { src, full } — card usa src, viewer usa full", () => {
  // O par vem do MESMO shot: thumb leve para o card, full nítido para o viewer.
  assert.match(
    SCREENSHOTS,
    /\.map\(\s*\(shot\)\s*=>\s*\(\{\s*src:\s*shot\.thumb\s*\|\|\s*shot\.full,\s*full:\s*shot\.full\s*\|\|\s*shot\.thumb\s*\}\)\s*\)/,
  )
  // As imagens locais (screenshots/titleScreens) também viram { src, full }.
  assert.match(
    SCREENSHOTS,
    /local\.map\(\s*\(image\)\s*=>\s*\(\{\s*src:\s*image,\s*full:\s*image\s*\}\)\s*\)/,
  )
  // O tipo exige os dois campos: não dá para "esquecer" o full e cair no thumb.
  assert.match(fonte, /type MediaItem = \{\s*src: string\s*full: string/)
  // O card renderiza o leve (`item.src`)...
  assert.match(fonte, /<img src=\{item\.src\}/)
  // ...e o clique abre o nítido (`item.full`).
  assert.match(fonte, /onClick=\{\(\) => setMedia\(item\.full\)\}/)
})

test("mediaItems: um item por screenshot, labels renumerados sem buracos", () => {
  // Sem flatMap: a lista de screenshots é percorrida uma vez.
  assert.doesNotMatch(MEDIA_ITEMS, /\.flatMap\(/)
  assert.match(MEDIA_ITEMS, /screenshots\.map\(\s*\(image,\s*index\)\s*=>\s*\(\{/)
  // `src`/`full` passam direto — nada de re-expandir o par no meio do caminho.
  assert.match(MEDIA_ITEMS, /src:\s*image\.src/)
  assert.match(MEDIA_ITEMS, /full:\s*image\.full/)
  // O label deriva do índice (1, 2, 3...), então não há buraco na numeração.
  assert.match(MEDIA_ITEMS, /label:\s*`Imagem \$\{index \+ 1\}`/)
  // A tira é renderizada uma única vez, chaveada pelo `full`.
  const ocorrencias = fonte.match(/mediaItems\.map\(/g) || []
  assert.equal(ocorrencias.length, 1, "a tira deve renderizar mediaItems uma única vez")
  assert.match(fonte, /key=\{item\.full\}/)
})
