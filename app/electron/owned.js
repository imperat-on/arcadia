"use strict"

const fs = require("fs")
const path = require("path")
const { conta, caminhoConta, DATA_DIR } = require("./supabase/conta")

// Posse de jogos por conta: filtra o library.json (global) pelos jogos que a
// conta ativa registrou como dela. Guest sempre vê tudo (raiz = como sempre
// foi, zero regressão). owned_games.json ausente = conta ainda não migrou
// pra posse = "possui tudo" (materializa na primeira leitura, senão a conta
// perderia acesso ao que já tinha antes desta feature existir).
const OWNED_GAMES = path.join(DATA_DIR, "owned_games.json")

function readOwned() {
  try {
    const raw = JSON.parse(fs.readFileSync(caminhoConta(OWNED_GAMES), "utf-8"))
    return Array.isArray(raw) ? raw : null
  } catch {
    return null
  }
}

// Best-effort: grava a posse inicial (ids atuais do library.json) na primeira
// leitura da conta. Falha aqui não pode derrubar a biblioteca, materializar é
// conveniência, não requisito de leitura.
function materializarPosse(globais) {
  try {
    fs.writeFileSync(caminhoConta(OWNED_GAMES), JSON.stringify(globais.map((g) => g.id)))
  } catch {
    /* best-effort */
  }
}

function filtrarPorPosse(globais) {
  if (!conta()) return globais
  const rawOwned = readOwned()
  if (rawOwned === null) {
    materializarPosse(globais)
    return globais
  }
  const owned = new Set(rawOwned)
  return globais.filter((g) => owned.has(g.id))
}

module.exports = { OWNED_GAMES, filtrarPorPosse, readOwned, materializarPosse }
