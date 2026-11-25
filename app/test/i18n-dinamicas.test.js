"use strict"

// Catraca das chaves i18n montadas DINAMICAMENTE.
//
// O prune de i18n daqui media uso por texto: se a chave `downloads.status.done`
// não aparece literalmente em lugar nenhum, ela era removida. Só que o card de
// download monta a chave na hora (`downloads.status.${status}`), então três
// chaves saíram e o card passaria a mostrar "downloads.status.done" cru na tela.
//
// Aqui os domínios são lidos do PRÓPRIO código (unions de tipo, objetos de
// rótulo) em vez de listas escritas à mão, para que um status novo sem chave
// também falhe.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const APP = path.resolve(__dirname, "..")
const ler = (...p) => fs.readFileSync(path.join(APP, ...p), "utf8")
const catalogo = (nome) => JSON.parse(ler("src", "i18n", `${nome}.json`))

/** Membros de um union de strings: `export type X = "a" | "b"`. */
function membrosDoUnion(fonte, nome) {
  // Vai até a próxima declaração (linha em branco ou outro `export`) — sem
  // isso, um union colado no seguinte contamina a lista.
  const m = new RegExp(`export type ${nome}\\s*=([\\s\\S]*?)(?:\\n\\s*\\nexport|\\n\\n|\\nexport)`).exec(fonte)
  assert.ok(m, `union ${nome} não encontrado`)
  return [...m[1].matchAll(/"([^"]+)"/g)].map((x) => x[1])
}

/** Chaves literais de um objeto `{ a: "chave.x", ... }`. */
function valoresDoObjeto(fonte, nome) {
  const m = new RegExp(`const ${nome}\\s*:[^=]*=\\s*\\{([\\s\\S]*?)\\n\\}`).exec(fonte)
  assert.ok(m, `objeto ${nome} não encontrado`)
  return [...m[1].matchAll(/:\s*"([^"]+)"/g)].map((x) => x[1])
}

test("as chaves dinâmicas dos downloads (status/origem/etapas) existem nos 3 catálogos", () => {
  const norm = ler("src", "components", "downloads", "normalize.ts")
  const kinds = membrosDoUnion(norm, "DownloadKind")
  const status = membrosDoUnion(norm, "DownloadStatus")

  // Mesmas regras de kindKey()/statusKey() — se elas mudarem, este teste cai.
  const chaves = []
  for (const k of kinds) chaves.push(`downloads.origem.${k === "legendary" ? "epic" : k}`)
  for (const s of status) {
    if (s === "caching") chaves.push("torrent.status.cacheando")
    else chaves.push(`downloads.status.${s === "active" ? "baixando" : s}`)
  }
  chaves.push(...valoresDoObjeto(ler("src", "components", "downloads", "DownloadCard.tsx"), "ROTULO"))

  assert.ok(chaves.length >= 10, `esperava várias chaves, achei ${chaves.length}`)
  for (const nome of ["pt-BR", "en-US", "es-ES"]) {
    const cat = catalogo(nome)
    for (const k of chaves) assert.ok(k in cat, `${nome}: falta a chave dinâmica ${k}`)
  }
})

test("os motivos de update bloqueado (updater.js -> GeneralSection) têm chave", () => {
  const js = ler("electron", "updater.js")
  const motivos = [...js.matchAll(/motivo:\s*"([^"]+)"/g)].map((m) => m[1])
  assert.ok(motivos.length > 0, "nenhum motivo encontrado no updater")
  for (const nome of ["pt-BR", "en-US", "es-ES"]) {
    const cat = catalogo(nome)
    for (const motivo of new Set(motivos)) {
      assert.ok(`update.bloqueado.${motivo}` in cat, `${nome}: falta update.bloqueado.${motivo}`)
    }
  }
})

test("as famílias de controle e os graus do protondb têm chave", () => {
  const tipos = membrosDoUnion(ler("src", "components", "useGamepadConnection.ts"), "TipoControle")
  // Domínios que vêm da API, não do repositório: os valores conhecidos.
  const decks = ["verified", "playable", "unsupported", "unknown"]
  const tiers = ["platinum", "gold", "silver", "bronze", "borked"]

  const chaves = [
    "controller.tipo",
    ...tipos.map((t) => `controller.tipo_${t}`),
    ...decks.map((d) => `protondb.deck.${d}`),
    ...tiers.map((t) => `protondb.tier.${t}`),
  ]
  for (const nome of ["pt-BR", "en-US", "es-ES"]) {
    const cat = catalogo(nome)
    for (const k of chaves) assert.ok(k in cat, `${nome}: falta a chave dinâmica ${k}`)
  }
})
