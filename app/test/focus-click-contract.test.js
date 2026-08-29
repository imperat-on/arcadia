"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.resolve(__dirname, "..")
const read = (...parts) => fs.readFileSync(path.join(root, ...parts), "utf8")

function effectBlock(source, marker) {
  const start = source.indexOf(marker)
  assert.ok(start >= 0, `marcador ausente: ${marker}`)
  const end = source.indexOf("return () =>", start)
  assert.ok(end > start, `cleanup ausente depois de: ${marker}`)
  return source.slice(start, end)
}

test("PS5 usa o foco IPC do main como autoridade contra blur/focus DOM", () => {
  const source = read("src", "components", "ps5-launcher", "PS5Launcher.tsx")
  const block = effectBlock(source, "const focoNativoDisponivel = typeof api?.onAppFocus === \"function\"")

  assert.match(block, /const focoNativoDisponivel = typeof api\?\.onAppFocus === "function"/)
  assert.match(block, /const aoFocar = \(\) => \{[\s\S]*?if \(focoNativoDisponivel\) return/)
  assert.match(block, /const aoDesfocar = \(\) => \{[\s\S]*?if \(focoNativoDisponivel\) return/)
  assert.match(
    block,
    /if \(!focoNativoDisponivel\) \{[\s\S]*?window\.addEventListener\("focus", aoFocar\)[\s\S]*?window\.addEventListener\("blur", aoDesfocar\)[\s\S]*?\}/,
  )
  assert.match(block, /const off = focoNativoDisponivel \? api\?\.onAppFocus\?\.\(aoFocoDoMain\) : undefined/)
})

test("replay assíncrono de getAppFocus não sobrescreve um evento IPC mais novo", () => {
  for (const file of [
    "src/components/ps5-launcher/PS5Launcher.tsx",
    "src/components/desktop/DesktopLauncher.tsx",
  ]) {
    const source = read(...file.split("/"))
    const block = effectBlock(source, "const sequenciaDaConsulta = focoSequenciaRef.current")
    const sequence = block.indexOf("const sequenciaDaConsulta = focoSequenciaRef.current")
    const query = block.indexOf("getAppFocus")
    const promise = block.indexOf("void ", query)
    const guard = block.indexOf("focoSequenciaRef.current !== sequenciaDaConsulta")

    assert.ok(sequence >= 0, `${file}: captura a geração antes da consulta`)
    assert.ok(query > sequence, `${file}: captura a geração antes de chamar getAppFocus`)
    assert.ok(promise > query, `${file}: consulta usa a geração capturada`)
    assert.ok(guard > promise, `${file}: resposta antiga é descartada antes de aplicar foco`)
    assert.match(block, /if \(!efeitoAtivo \|\| focoSequenciaRef\.current !== sequenciaDaConsulta\) return/)
  }
})

test("mudanças de foco IPC avançam a geração que invalida respostas pendentes", () => {
  const ps5 = read("src", "components", "ps5-launcher", "PS5Launcher.tsx")
  const desktop = read("src", "components", "desktop", "DesktopLauncher.tsx")
  assert.match(ps5, /const aoFocoDoMain = \(f: boolean\) => \{[\s\S]*?const sequencia = \+\+focoSequenciaRef\.current/)
  assert.match(desktop, /const aoFoco = \(focused: boolean\) => \{[\s\S]*?focoSequenciaRef\.current \+= 1/)
})
