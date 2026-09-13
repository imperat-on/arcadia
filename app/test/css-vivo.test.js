"use strict"

// Catraca do CSS vivo do renderer.
//
// Nada aqui é estético: o refactor daqui apagou regras "mortas" com um script
// que olhava só o texto. Duas coisas escaparam dele e só apareciam no CSS
// COMPILADO, onde ninguém olha:
//
//  1. as linhas de continuação de comentários multi-linha foram apagadas junto
//     com as regras vizinhas, deixando um `/*` aberto — e o comentário engoliu
//     o que vinha depois (o @supports do vidro e um @media inteiro);
//  2. uma regra com vários seletores (.glass-1, .glass-2, .glass-3) saiu
//     inteira porque só dois dos três nomes pareciam mortos.
//
// As três checagens abaixo pegam as duas coisas: comentário não pode engolir
// regra, tudo que o index.css define tem que chegar ao pacote, e classe de
// projeto usada no JSX tem que estar definida.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const APP = path.resolve(__dirname, "..")
const CSS = path.join(APP, "src", "index.css")

/** Remove comentários como o navegador faz e diz se algum ficou aberto. */
function semComentarios(css) {
  let saida = ""
  let i = 0
  while (i < css.length) {
    if (css.startsWith("/*", i)) {
      const fim = css.indexOf("*/", i + 2)
      if (fim < 0) return { limpo: saida, abertoEm: i }
      i = fim + 2
      continue
    }
    saida += css[i]
    i++
  }
  return { limpo: saida, abertoEm: -1 }
}

/** Classes que o index.css DEFINE como seletor. */
function classesDefinidas(css) {
  const { limpo } = semComentarios(css)
  const nomes = new Set()
  for (const bloco of limpo.matchAll(/([^{}]+)\{/g)) {
    const sel = bloco[1]
    for (const c of sel.matchAll(/\.([A-Za-z][A-Za-z0-9_-]*)/g)) nomes.add(c[1])
  }
  return nomes
}

/**
 * Classes definidas em blocos <style> dentro dos componentes (o AuthDialog faz
 * isso com as animações da marca). São CSS de verdade, injetado em runtime —
 * não passam pelo bundle nem pelo index.css.
 */
function classesEmStyleInline() {
  const nomes = new Set()
  const anda = (dir) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, e.name)
      if (e.isDirectory()) {
        if (e.name === "test" || e.name === "node_modules") continue
        anda(p)
      } else if (/\.tsx$/.test(e.name)) {
        const src = fs.readFileSync(p, "utf8")
        for (const m of src.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)) {
          for (const c of m[1].matchAll(/\.([A-Za-z][A-Za-z0-9_-]*)/g)) nomes.add(c[1])
        }
      }
    }
  }
  anda(path.join(APP, "src"))
  return nomes
}

/**
 * Classes usadas no JSX em listas estáticas. Só listas puras: template com
 * interpolação ou ternário no meio traria identificador de JS junto ("c.url"),
 * que não é classe.
 */
function classesUsadasNoJsx() {
  const usadas = new Set()
  const anda = (dir) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, e.name)
      if (e.isDirectory()) {
        if (e.name === "test" || e.name === "node_modules") continue
        anda(p)
      } else if (/\.tsx$/.test(e.name)) {
        const src = fs.readFileSync(p, "utf8")
        for (const m of src.matchAll(/className="([^"]*)"/g)) {
          for (const t of m[1].split(/\s+/)) {
            if (t && /^[a-z][a-z0-9._/-]*$/i.test(t)) usadas.add(t)
          }
        }
        for (const m of src.matchAll(/className=\{`([^`]*)`\}/g)) {
          if (m[1].includes("${")) continue // lista dinâmica: não dá para ler como texto
          for (const t of m[1].split(/\s+/)) {
            if (t && /^[a-z][a-z0-9._/-]*$/i.test(t)) usadas.add(t)
          }
        }
      }
    }
  }
  anda(path.join(APP, "src"))
  return usadas
}

test("index.css: nenhum comentário aberto engolindo regra", () => {
  const css = fs.readFileSync(CSS, "utf8")
  const { abertoEm } = semComentarios(css)
  assert.equal(
    abertoEm,
    -1,
    abertoEm >= 0
      ? `comentário /* sem fechar na linha ${css.slice(0, abertoEm).split("\n").length} — tudo depois dele vira comentário e some do pacote`
      : "",
  )
  // Um comentário não pode conter seletor: é sintoma de bloco engolido.
  for (const m of css.matchAll(/\/\*([\s\S]*?)\*\//g)) {
    const corpo = m[1]
    assert.ok(
      !/[.#@][A-Za-z][^\n{]*\{/.test(corpo),
      `comentário engolindo regra: ${corpo.trim().slice(0, 80)}`,
    )
  }
})

/** O dist/ precisa estar mais novo que o index.css para a comparação valer. */
function cssCompilado(t) {
  const dist = path.join(APP, "dist", "assets")
  const arquivos = fs.existsSync(dist) ? fs.readdirSync(dist).filter((f) => f.endsWith(".css")) : []
  if (!arquivos.length) {
    t.skip("sem dist/ — rode npm run build")
    return null
  }
  const alvo = arquivos.map((f) => path.join(dist, f))
  if (Math.min(...alvo.map((f) => fs.statSync(f).mtimeMs)) < fs.statSync(CSS).mtimeMs) {
    t.skip("dist/ mais velho que o index.css — rode npm run build")
    return null
  }
  // O Tailwind escapa `: / . [ ]` no que gera (.text-white\/85, .md\:grid-cols-3):
  // tirar as barras deixa a comparação por substring válida.
  return alvo.map((f) => fs.readFileSync(f, "utf8")).join("\n").replace(/\\/g, "")
}

test("toda regra do index.css chega ao CSS compilado", (t) => {
  const compilado = cssCompilado(t)
  if (!compilado) return
  const definidas = classesDefinidas(fs.readFileSync(CSS, "utf8"))
  const ausentes = [...definidas].filter((c) => !compilado.includes(c)).sort()
  assert.deepEqual(ausentes, [], `definidas no index.css e ausentes do pacote: ${ausentes.join(", ")}`)
})

// Ganchos sem estilo por natureza: ficam no JSX para marcar o elemento (e podem
// ser alvo do CSS que o usuário injeta via custom_css_path), mas nunca tiveram
// regra no index.css. Cada um foi conferido: não existiam nem antes do prune.
const GANCHOS_SEM_ESTILO = new Set([
  "detail-achievement-full-grid",
  "retro-rail-arrow-left",
  "retro-rail-arrow-right",
  "store-game-hero",
  "store-game-skeleton",
])

test("classe usada no JSX ou é gerada pelo Tailwind ou está definida no index.css", (t) => {
  const compilado = cssCompilado(t)
  if (!compilado) return
  const definidas = classesDefinidas(fs.readFileSync(CSS, "utf8"))
  const inline = classesEmStyleInline()
  const orfas = []
  for (const classe of classesUsadasNoJsx()) {
    if (GANCHOS_SEM_ESTILO.has(classe)) continue
    // Estilizada de algum modo: utilitário do Tailwind (está no pacote), regra
    // própria do index.css ou bloco <style> do componente.
    if (compilado.includes(classe) || definidas.has(classe) || inline.has(classe)) continue
    orfas.push(classe)
  }
  assert.deepEqual(
    [...new Set(orfas)].sort(),
    [],
    `classes sem nenhuma regra (Tailwind ou index.css): ${[...new Set(orfas)].sort().join(", ")}`,
  )
})
