"use strict"

// Catraca das horas combinadas.
//
// A regra é: o total da Steam MANDA quando existe, porque o jogo do Arcadia abre
// pela Steam e o cliente já conta aquele tempo. O que o Arcadia mediu entra só
// quando a Steam não tem o jogo (crackeado/emulador). SOMAR os dois daria um
// número maior que o cliente mostra — contagem dupla — e é isso que este teste
// impede de voltar por engano.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const root = path.join(__dirname, "..")
const ler = (rel) => fs.readFileSync(path.join(root, rel), "utf8")

test("o componente escolhe Steam; Arcadia é o que sobra", () => {
  const fonte = ler("src/components/desktop/HorasNaSteam.tsx")
  assert.ok(fonte.includes("minutosSteam > 0"), "decide pelo valor da Steam")
  assert.ok(
    /const total = daSteam \? minutosSteam :/.test(fonte),
    "Steam manda; Arcadia só quando a Steam não tem",
  )
  assert.ok(
    !/minutosSteam\s*\+\s*(Number\()?minutosArcadia/.test(fonte),
    "somar as duas fontes é contagem dupla — não pode voltar",
  )
})

test("o id do jogo (steam:990080) casa com a chave numérica do arquivo da Steam", () => {
  // O bug que deixava as capas em 0min: a chave da Steam é "990080" e o jogo da
  // biblioteca carrega o número dentro do `id` ("steam:990080") — o campo `appid`
  // não existe fora das linhas da loja.
  const main = fs.readFileSync(path.join(root, "electron", "steam-account.js"), "utf8")
  assert.match(main, /function chaveAppid\(appid\)/, "existe o normalizador de chave")
  assert.ok(
    main.includes("module.exports") && main.includes("chaveAppid,"),
    "e ele é exportado para o main usar",
  )
  const mainJs = fs.readFileSync(path.join(root, "electron", "main.js"), "utf8")
  assert.ok(
    (mainJs.match(/chaveAppid\(/g) || []).length >= 2,
    "perfil e horasDoJogo normalizam a chave (não comparam a string crua)",
  )
  assert.ok(
    mainJs.includes("chaveAppid(g.id || g.appid"),
    "o agregado do perfil usa o id da biblioteca (não só o appid da loja)",
  )
  const renderer = ler("src/components/steamHoras.ts")
  assert.ok(renderer.includes("export function chaveAppid"), "o renderer faz o mesmo")
  assert.ok(
    renderer.includes("export function minutosDaSteam") && renderer.includes("chaveAppid(f)"),
    "e o lookup passa pelo normalizador",
  )

  // Quem usa precisa mandar o `id` do jogo, não só o appid (que falta na biblioteca).
  const perfil = ler("src/components/ps5-launcher/ProfilePage.tsx")
  assert.ok(
    (perfil.match(/\[game\.id, game\.appid\]|\[[ab]\.id, [ab]\.appid\]/g) || []).length >= 2,
    "as capas do perfil passam id e appid como candidatos",
  )
  assert.ok(
    ler("src/components/desktop/GamePage.tsx").includes("g.id || g.appid"),
    "a página da biblioteca também",
  )
  assert.ok(
    ler("src/components/desktop/GameDetailsDialog.tsx").includes("game.id || game.appid"),
    "e o diálogo de detalhes",
  )
})

test("as horas aparecem na loja, na biblioteca e no diálogo de detalhes", () => {
  assert.ok(ler("src/components/desktop/StoreGamePage.tsx").includes("<HorasNaSteam"))
  assert.ok(ler("src/components/desktop/GamePage.tsx").includes("<HorasNaSteam"))
  const dialog = ler("src/components/desktop/GameDetailsDialog.tsx")
  assert.ok(dialog.includes("steamHorasDoJogo"), "o diálogo lê as horas da Steam")
  assert.ok(dialog.includes("minutosSteam > 0"), "e usa a mesma regra")
})

test("o perfil e as capas usam as horas combinadas, não só as do Arcadia", () => {
  const main = fs.readFileSync(path.join(root, "electron", "main.js"), "utf8")
  // O agregado do perfil ("horas jogadas") passa a somar Steam (quando existe).
  const blocoStats = main.slice(main.indexOf('ipcMain.handle("profile:stats"'), main.indexOf("steam:horasTodas"))
  assert.match(blocoStats, /steam-account/, "o perfil lê as horas da Steam")
  assert.match(blocoStats, /daSteam > 0 \? daSteam/, "e aplica a regra: Steam manda")

  const perfil = ler("src/components/ps5-launcher/ProfilePage.tsx")
  assert.ok(perfil.includes("horasCombinadas"), "as capas do perfil usam a regra")
  assert.ok(ler("src/components/steamHoras.ts").includes("steamHorasTodas"), "uma leitura só serve para todos os tiles")
})

test("os textos existem nos três catálogos", () => {
  for (const lang of ["pt-BR", "en-US", "es-ES"]) {
    const d = JSON.parse(ler(`src/i18n/${lang}.json`))
    for (const k of ["steam_captura.horas_na_steam", "steam_captura.tempo_jogo", "steam_captura.na_steam"]) {
      assert.ok(typeof d[k] === "string" && d[k].length > 3, `${lang} sem ${k}`)
    }
  }
})
