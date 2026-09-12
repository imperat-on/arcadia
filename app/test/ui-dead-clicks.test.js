"use strict"

// Clique que não faz nada é bug: um botão com handler vazio promete uma ação e
// não entrega nada — foi o caso do "Pendente" em Amigos e dos tiles do perfil
// de um amigo.
//
// Esta catraca procura handlers vazios passados como prop de clique
// (`onAlgo={() => {}}`) nos componentes do renderer. Ficam de fora do padrão as
// exceções legítimas, que não são props de JSX: `.catch(() => {})`,
// `useRef<() => void>(() => {})` e ternários (`x ? () => {} : onOutro`).

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const src = path.resolve(__dirname, "..", "src")
const CRLF = String.fromCharCode(13, 10)
const read = (...parts) =>
  fs.readFileSync(path.join(src, ...parts), "utf8").split(CRLF).join(String.fromCharCode(10))

function listarTsx(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entrada) => {
    const completo = path.join(dir, entrada.name)
    if (entrada.isDirectory()) return listarTsx(completo)
    return entrada.name.endsWith(".tsx") ? [completo] : []
  })
}

test("nenhum componente passa handler vazio em prop de clique", () => {
  const vazio = /\bon[A-Z][A-Za-z]*=\{\s*\(\s*\)\s*=>\s*\{\s*\}\s*\}/
  const achados = []
  for (const arquivo of listarTsx(src)) {
    const linhas = read(path.relative(src, arquivo)).split("\n")
    linhas.forEach((linha, i) => {
      if (vazio.test(linha)) achados.push(`${path.relative(src, arquivo)}:${i + 1}`)
    })
  }
  assert.deepEqual(achados, [], "prop de clique com handler vazio (clique mudo)")
})

test("pedido de amizade pendente é estado, não botão", () => {
  const amigos = read("components", "desktop", "FriendsView.tsx")
  assert.match(amigos, /seloPendente\(t\("amigos\.pendente"\)\)/)
  assert.doesNotMatch(amigos, /botaoAcao\(t\("amigos\.pendente"\)/)
})

test("jogos do perfil de um amigo não ficam clicáveis e mudos", () => {
  const amigo = read("components", "desktop", "FriendProfileView.tsx")
  assert.doesNotMatch(amigo, /onJogoClick=\{\(\) => \{\}\}/)
  assert.doesNotMatch(amigo, /onEdit=\{\(\) => \{\}\}/)
  // readOnly esconde o "Editar perfil"; sem onJogoClick o tile fica disabled.
  assert.match(amigo, /readOnly/)
})

test("página de jogo só pede 'adicionar' quando o jogo não está na biblioteca", () => {
  const pagina = read("components", "desktop", "StoreGamePage.tsx")
  assert.match(pagina, /onAdicionar\?: \(\) => void/)
  const launcher = read("components", "desktop", "DesktopLauncher.tsx")
  const library = read("components", "desktop", "LibraryView.tsx")
  assert.doesNotMatch(launcher, /onAdicionar=\{\(\) => \{\}\}/)
  assert.doesNotMatch(library, /onAdicionar=\{\(\) => \{\}\}/)
})

test("todo elemento clicável fora de <button> escapa da região de arraste", () => {
  // O shell do desktop inteiro é `-webkit-app-region: drag`. O CSS libera o
  // clique com no-drag para button/a/input/select/textarea/video/img,
  // [role="button"], [role="dialog"], [data-no-drag] e [class*="overflow-"].
  // Um <div onClick> fora dessa lista vira ÁREA DE ARRASTE: o Electron consome o
  // clique e move a janela — era o caso dos backdrops dos diálogos montados à
  // mão, dos avisos flutuantes e do card de amigo.
  const ABRE = /<(div|span|li|article|section|main|aside|header)\b/g
  const achados = []
  for (const arquivo of listarTsx(src)) {
    const texto = read(path.relative(src, arquivo))
    for (const achado of texto.matchAll(ABRE)) {
      let i = achado.index + achado[0].length
      let profundidade = 0
      let tag = ""
      for (; i < texto.length; i++) {
        const c = texto[i]
        tag += c
        if (c === "{") profundidade++
        else if (c === "}") profundidade--
        else if (c === ">" && profundidade === 0) break
      }
      if (!/\bonClick=/.test(tag)) continue
      if (/data-no-drag|role="button"|role="dialog"|overflow-/.test(tag)) continue
      const linha = texto.slice(0, achado.index).split("\n").length
      achados.push(`${path.relative(src, arquivo)}:${linha}`)
    }
  }
  assert.deepEqual(achados, [], "elemento clicável sem no-drag (o clique viraria arraste)")
})

test("o CSS mantém as saídas de no-drag que os componentes usam", () => {
  const css = read("index.css")
  assert.match(css, /\.app-drag \[data-no-drag\],/)
  assert.match(css, /\.app-drag \[role="dialog"\],/)
  assert.match(css, /\.app-drag \[class\*="overflow-"\]/)
})
