"use strict"

// Navegação do shell desktop: TODO clique de aba tem de entregar uma visão
// limpa.
//
// Regressão do "clico na Loja, depois clico no meu perfil e não vai":
// - as páginas do shell (jogo Steam/Epic) não checavam a aba, e o clique em
//   "Meu perfil" não limpava a página aberta — a página continuava na tela e o
//   perfil nunca aparecia;
// - reabrir a aba em que já se estava não remontava o conteúdo (a `key` era só
//   a aba), então um detalhe aberto DENTRO da aba — loja, início, biblioteca —
//   engolia o clique seguinte.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.resolve(__dirname, "..")
// Os fontes sao CRLF no checkout Windows; os contratos multi-linha buscam com
// \n. Normalizar na leitura preserva o offset dos indexOf/regex.
const CRLF = String.fromCharCode(13, 10)
const read = (...parts) =>
  fs.readFileSync(path.join(root, ...parts), "utf8").split(CRLF).join(String.fromCharCode(10))

const launcher = () => read("src", "components", "desktop", "DesktopLauncher.tsx")

test("toda troca de aba passa pelo mesmo ponto (irPara)", () => {
  const src = launcher()

  assert.match(src, /const irPara = useCallback\(\(destino: DesktopView\) => \{/)
  // Limpa as duas páginas do shell e fecha o editor de perfil sobreposto.
  assert.match(
    src,
    /setJogoPagina\(null\)\n\s*setRetroPaginaJogo\(null\)\n\s*setShowEditProfile\(false\)/,
  )
  // Reabrir a aba em que já se está remonta o conteúdo.
  assert.match(src, /if \(viewRef\.current === destino\) setNavegacao\(\(n\) => n \+ 1\)/)

  // Nenhum ponto de entrada pode voltar a trocar de aba por conta própria.
  assert.equal(
    src.match(/setView\(/g).length,
    1,
    "só o irPara pode chamar setView",
  )
  assert.doesNotMatch(src, /onView=\{\(v\) => \{/)
  assert.doesNotMatch(src, /onProfile=\{\(\) => setView/)
  assert.doesNotMatch(src, /onOpenDownloads=\{\(\) => setView/)
  assert.doesNotMatch(src, /onClose=\{\(\) => setView\("inicio"\)\}/)

  // Os atalhos que navegam continuam ligados no irPara.
  for (const ponto of [
    /onView=\{irPara\}/,
    /onProfile=\{\(\) => irPara\("perfil"\)\}/,
    /onOpenDownloads=\{\(\) => irPara\("downloads"\)\}/,
    /onClose=\{\(\) => irPara\("inicio"\)\}/,
    /irPara\("biblioteca"\)/,
  ]) {
    assert.match(src, ponto)
  }
})

test("páginas do shell só existem na Biblioteca", () => {
  const src = launcher()
  // Sem a guarda de aba, abrir um jogo e depois pedir "Meu perfil" deixava a
  // página do jogo na tela (era o bug relatado).
  assert.match(
    src,
    /\{view === "biblioteca" && jogoPagina && String\(jogoPagina\.id\)\.startsWith\("steam:"\) && \(/,
  )
  assert.match(
    src,
    /\{view === "biblioteca" && jogoPagina && !String\(jogoPagina\.id\)\.startsWith\("steam:"\)/,
  )
})

test("a key do conteúdo inclui a navegação", () => {
  const src = launcher()
  assert.match(src, /key=\{`\$\{view\}:\$\{navegacao\}`\}/)
})

test("o menu do perfil na sidebar continua chamando onProfile", () => {
  const sidebar = read("src", "components", "desktop", "Sidebar.tsx")
  assert.match(sidebar, /onProfile\(\)/)
  assert.match(sidebar, /profile\.meu_perfil/)
})
