"use strict"

// Catraca contra chave DUPLICADA nos catálogos de i18n.
//
// Duplicata em JSON é silenciosa: `JSON.parse` fica com a última ocorrência e a
// primeira vira lixo invisível (editar a "de cima" não muda nada na tela). Os
// catálogos tinham `controller.titulo`, `controller.completo` e
// `controller.parcial` duplicados nos três idiomas.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.join(__dirname, "..", "src", "i18n")
const RE_CHAVE = /^\s*"([^"]+)":/gm

// Varre o TEXTO (não o objeto parseado, que já perdeu a duplicata).
function chavesDuplicadas(bruto) {
  const contagem = new Map()
  for (const m of bruto.matchAll(RE_CHAVE)) {
    contagem.set(m[1], (contagem.get(m[1]) || 0) + 1)
  }
  return [...contagem.entries()].filter(([, n]) => n > 1).map(([k, n]) => `${k} (${n}x)`)
}

for (const lang of ["pt-BR", "en-US", "es-ES"]) {
  test(`${lang}: nenhuma chave duplicada`, () => {
    const bruto = fs.readFileSync(path.join(root, `${lang}.json`), "utf8")
    const dup = chavesDuplicadas(bruto)
    assert.deepEqual(dup, [], `chaves duplicadas em ${lang}: ${dup.join(", ")}`)
  })
}

test("os três catálogos têm exatamente o mesmo conjunto de chaves", () => {
  const conjuntos = ["pt-BR", "en-US", "es-ES"].map((lang) => {
    const bruto = fs.readFileSync(path.join(root, `${lang}.json`), "utf8")
    const d = JSON.parse(bruto)
    return { lang, chaves: new Set(Object.keys(d)) }
  })
  const base = conjuntos[0]
  for (const outro of conjuntos.slice(1)) {
    const soLa = [...base.chaves].filter((k) => !outro.chaves.has(k))
    const soCa = [...outro.chaves].filter((k) => !base.chaves.has(k))
    assert.deepEqual(
      { soLa, soCa },
      { soLa: [], soCa: [] },
      `${outro.lang} divergente de ${base.lang}`,
    )
  }
})
