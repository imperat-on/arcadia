"use strict"

// popularItens: pré-popula o cache local de items (store_items_cache.json)
// com os appids dos stubs pending que o pull de biblioteca vai criar.
// Sem isto, o itensDaLoja do pull tinha que ir à Steam na hora — o stub
// nascia com capa cinza e a sidebar só ganhava o ícone após a cura.
// Teste com fetchItems injetado (o closure interno não é patchável).

const test = require("node:test")
const assert = require("node:assert")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const DATA = path.join(os.homedir(), ".local", "share", "arcadia")
const ITENS_CACHE = path.join(DATA, "store_items_cache.json")
const ORIGINAL = fs.readFileSync(ITENS_CACHE, "utf8")

function carregarMod() {
  const caminho = require.resolve("../../app/electron/steamstore.js")
  delete require.cache[caminho]
  return require(caminho)
}

function comCacheLimpo() {
  const cache = JSON.parse(fs.readFileSync(ITENS_CACHE, "utf8"))
  delete cache["xyz"]
  delete cache["12345"]
  delete cache["99999"]
  fs.writeFileSync(ITENS_CACHE, JSON.stringify(cache))
}

function restaurar() {
  fs.writeFileSync(ITENS_CACHE, ORIGINAL)
}

test("popularItens: appid novo dispara fetch", async () => {
  comCacheLimpo()
  const mod = carregarMod()
  let chamou = null
  await mod.popularItens(["99999", "99999", "12345"], async (ids) => {
    chamou = ids
    return { mapa: new Map(), respondidos: new Set() }
  })
  assert.ok(chamou, "chamou fetchItems")
  assert.deepStrictEqual([...chamou].sort(), ["12345", "99999"], "só os que faltam, sem duplicados")
  restaurar()
})

test("popularItens: appids ja no cache (fresco) nao disparam fetch", async () => {
  const cache = JSON.parse(fs.readFileSync(ITENS_CACHE, "utf8"))
  cache["abc"] = { tipo: 0, capa: "c", heroi: "h", icon: "i", at: Date.now() }
  fs.writeFileSync(ITENS_CACHE, JSON.stringify(cache))
  const mod = carregarMod()
  let chamou = null
  await mod.popularItens(["abc"], async (ids) => {
    chamou = ids
    return { mapa: new Map(), respondidos: new Set() }
  })
  assert.strictEqual(chamou, null, "nao chamou fetchItems para appid em cache")
  restaurar()
})

test("popularItens: lista vazia nao chama fetch", async () => {
  const mod = carregarMod()
  let chamou = null
  await mod.popularItens([], async (ids) => {
    chamou = ids
    return { mapa: new Map(), respondidos: new Set() }
  })
  assert.strictEqual(chamou, null, "sem appids nao chama")
  restaurar()
})
