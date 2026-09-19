"use strict"

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const { _matchTitulo, _buscarNoIndice } = require("../electron/sources")

const root = path.resolve(__dirname, "..")
const CRLF = String.fromCharCode(13, 10)
const read = (...parts) =>
  fs.readFileSync(path.join(root, ...parts), "utf8").split(CRLF).join(String.fromCharCode(10))

const casa = (alvo, candidato) => Boolean(_matchTitulo(alvo, candidato))
const rank = (alvo, candidatos) =>
  candidatos
    .flatMap((c) => {
      const match = _matchTitulo(alvo, c)
      return match ? [{ c, match }] : []
    })
    .sort((a, b) => b.match.score - a.match.score || a.match.extra - b.match.extra)
    .map((item) => item.c)

test("fronteira de palavra: token curto não casa dentro de outra palavra", () => {
  assert.equal(casa("ARK: Survival Evolved", "Boris and the Dark Survival Free Download (V1.13)"), false)
  assert.equal(casa("ARK: Survival Evolved", "Shark Siege: Together Survival [DODI Repack]"), false)
  assert.equal(casa("Raft", "Warcraft II Battlenet Edition v2.02-v5 [GOG]"), false)
  assert.equal(casa("Bone: Out From Boneville", "Albedo: Eyes from Outer Space [GOG]"), false)
  assert.equal(casa("ARK: Survival Evolved", "ARK: Survival Evolved Free Download"), true)
})

test("sequência: Far Cry 3 não casa Far Cry 2 nem Baldur's Gate 2", () => {
  assert.equal(casa("Far Cry 3", "Far Cry 2: Fortune's Edition [GOG]"), false)
  assert.equal(casa("Far Cry 3", "Far Cry 3 Free Download [SteamGG]"), true)
  assert.equal(casa("Baldur's Gate 3", "Baldur's Gate: Enhanced Edition v2.6.6.0-p [GOG]"), false)
  assert.equal(casa("Baldur's Gate 3", "Baldur's Gate 2: Enhanced Edition v2.6.6.0 [GOG]"), false)
  assert.equal(casa("Baldur's Gate 3", "Baldur's Gate 3 Free Download"), true)
  assert.equal(casa("Sid Meier's Civilization V", "Sid Meiers Civilization VII Free Download"), false)
})

test("alvo sem numeral não casa continuação numerada", () => {
  assert.equal(casa("Portal", "Portal 2 Free Download [Build-23973718+Co-Op] [SteamGG]"), false)
  assert.equal(casa("Portal", "Portal"), true)
  assert.equal(casa("Half-Life", "Half Life 2 (v16.11.2024) [Pre-Instalado] [Kazumi]"), false)
  assert.equal(casa("Half-Life", "Half-Life"), true)
  assert.equal(casa("Half-Life 2", "Half-Life 2 (v16.11.2024) [Pre-Instalado]"), true)
  assert.equal(casa("Left 4 Dead", "Left 4 Dead 2 Free Download [Build-23990068+Online]"), false)
  assert.equal(casa("Left 4 Dead", "Left 4 Dead 2 Build 04102021"), false)
})

test("DLC exige o sufixo; jogo base não entra", () => {
  const dlc = "Assassin's Creed Black Flag Resynced - Dragon Storm Character Pack"
  assert.equal(casa(dlc, "Assassins Creed Black Flag Resynced Free Download [v1.0.6+Bonus] [SteamGG]"), false)
  assert.equal(
    casa(dlc, "Assassin's Creed Black Flag Resynced - Dragon Storm Character Pack Free Download"),
    true,
  )
  assert.equal(casa("Half-Life 2: Episode One", "Half-Life 2: Episode Two Free Download"), false)
  assert.equal(casa("Half-Life 2: Episode One", "Fears to Fathom Episode 1 Home Alone Free Download"), false)
  assert.equal(casa("Half-Life 2: Episode One", "Half-Life 2: Episode One Free Download"), true)
})

test("versão não vira sequência: v3.20 e v1.2.0.59 não contam como numeral", () => {
  assert.equal(casa("Brawlhalla", "Brawlhalla v3.20 [Dauphong]"), true)
  assert.equal(casa("Hitman 2", "Hitman Blood Money Free Download (v1.2)"), false)
  assert.equal(casa("Mount & Blade II: Bannerlord", "Mount and Blade II Bannerlord v1 1 6-FLT"), true)
})

test("I solto é pronome, não numeral romano", () => {
  assert.equal(casa("Team Fortress 2", "I am Bread (Meets Team Fortress 2 Update) [Dauphong]"), true)
  assert.equal(casa("The Last of Us Part I", "The Last of Us Part I Free Download"), true)
  assert.equal(casa("The Last of Us Part I", "The Last of Us Part II Remastered"), false)
  assert.equal(casa("Mega Man X", "Mega Man X4"), false)
})

test("acento é dobrado, não removido: Ragnarok acha Ragnarök", () => {
  assert.equal(casa("God of War Ragnarök", "God of War Ragnarok Free Download [SteamGG]"), true)
  assert.equal(casa("God of War Ragnarok", "God of War Ragnarök Free Download"), true)
  assert.equal(casa("God of War Ragnarök", "God of War [GOG]"), false)
})

test("ranking: título exato > prefixo > frase > palavras", () => {
  const candidatos = [
    "Far Cry 2: Fortune's Edition [GOG]",
    "Far Cry 3: Blood Dragon Free Download",
    "Far Cry 3 Free Download [SteamGG]",
    "Far Cry 3",
  ]
  const ordenados = rank("Far Cry 3", candidatos)
  assert.equal(ordenados[0], "Far Cry 3")
  assert.equal(ordenados[1], "Far Cry 3 Free Download [SteamGG]")
  assert.ok(!ordenados.includes("Far Cry 2: Fortune's Edition [GOG]"))
})

test("busca re-rankeia antes do corte: release certa fora das 50 primeiras do índice", () => {
  const index = []
  for (let i = 0; i < 60; i++) index.push({ ref: `x:${i}`, title: `Far Cry 2 Fortune's Edition ${i}`, src: "x" })
  index.push({ ref: "x:ok", title: "Far Cry 3 Free Download [SteamGG]", src: "x" })
  const resultados = _buscarNoIndice(index, "Far Cry 3", 50)
  assert.equal(resultados[0].ref, "x:ok")
  assert.ok(resultados.length <= 50)
})

test("busca mantém o fallback parcial da aba Fontes", () => {
  const index = [
    { ref: "a:0", title: "Far Cry 3 Free Download [SteamGG]", src: "a" },
    { ref: "a:1", title: "Farcry Collection [GOG]", src: "a" },
  ]
  const resultados = _buscarNoIndice(index, "farc", 40)
  assert.ok(resultados.some((g) => g.ref === "a:1"))
})

test("loja PC usa o mesmo matcher no diálogo e no detalhe", () => {
  const actions = read("src", "components", "useStoreActions.ts")
  const detail = read("src", "components", "desktop", "StoreGamePage.tsx")
  assert.match(actions, /export function matchTituloLoja/)
  assert.match(actions, /matchTituloLoja\(tituloBusca, g\.title\)/)
  assert.doesNotMatch(actions, /tokensCoincidentes/)
  assert.match(detail, /import \{ matchTituloLoja \} from "\.\.\/useStoreActions"/)
  assert.match(detail, /matchTituloLoja\(tituloBusca, candidato\.title\)/)
  assert.doesNotMatch(detail, /tokensCoincidentes/)
})
