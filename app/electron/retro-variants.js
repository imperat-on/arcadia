"use strict"

// Colapso de variantes regionais no catálogo Retro.
//
// O mesmo jogo entra no catálogo mais de uma vez porque a release name da ISO
// (`(Korea)`, `(Taikenban)`, `(v1.20)`, `(France)`) é tratada como parte do
// título. A grade mostra a ficha canônica (LaunchBox/metadados) e as variantes
// viram fontes de download no detalhe, em vez de cartões repetidos.
//
// A regra vive aqui — um módulo sem dependências — para que o catálogo local
// (V1, V2) e o catálogo servido pelo backend usem exatamente a mesma identidade.

// A chave usa o parser do projeto, não um fold caseiro: `007: Agent Under Fire`
// e `007 - Agent Under Fire (Korea)` só coincidem porque a release name da ISO
// (`(Korea)`, `(Taikenban)`, `(v1.20)`) é removida e a pontuação vira espaço.
const { normalizeRetroTitle } = require("./retro-title-parser")

const QUALITY_RANK = { exact: 3, strong: 2, probable: 1, unmatched: 0 }

function foldTitle(value) {
  return normalizeRetroTitle(String(value || ""))
    .toLowerCase()
    .trim()
}

/**
 * Identidade de exibição: sistema + título canônico.
 * @param {object} game - Jogo ou oferta com systemId/plateforma e title
 * @returns {string} - Chave canônica
 */
function displayKey(game) {
  const system = String(game?.systemId || game?.platform || "")
    .trim()
    .toLowerCase()
  return `${system}\u001f${foldTitle(game?.title)}`
}

function winnerRank(game) {
  return [
    // Ficha de catálogo antes de jogo promovido a partir de uma oferta.
    String(game?.id || "").includes(":slug:") ? 0 : 1,
    Number(game?.offerCount) || 0,
    game?.artwork?.cover || game?.cover ? 1 : 0,
    QUALITY_RANK[String(game?.matchQuality || "")] || 0,
  ]
}

function preferWinner(left, right) {
  const a = winnerRank(left)
  const b = winnerRank(right)
  for (let index = 0; index < a.length; index++) {
    if (a[index] !== b[index]) return b[index] - a[index]
  }
  // Desempate determinístico pelo título citado, não pelo hash do id.
  return String(left?.title || "").localeCompare(String(right?.title || ""))
}

/**
 * Agrupa jogos por identidade de exibição.
 * @param {object[]} games - Jogos canônicos
 * @returns {Map<string, object[]>} - Grupos na ordem de primeira aparição
 */
function groupVariants(games) {
  const groups = new Map()
  for (const game of Array.isArray(games) ? games : []) {
    if (!game || typeof game !== "object") continue
    const key = displayKey(game)
    if (!groups.has(key)) groups.set(key, [])
    groups.get(key).push(game)
  }
  return groups
}

/**
 * Colapsa variantes em um representante por jogo, somando as ofertas.
 *
 * `offerCount` do representante é a soma do grupo: antes, `Agent Under Fire`
 * mostrava 4 (LaunchBox) e o registro coreano 1, como jogos distintos. Os
 * títulos absorvidos entram em `aliases` para que a busca continue os achando.
 *
 * @param {object[]} games - Jogos canônicos
 * @returns {object[]} - Um jogo por identidade de exibição
 */
function collapseVariants(games) {
  const collapsed = []
  for (const members of groupVariants(games).values()) {
    if (members.length === 1) {
      collapsed.push(members[0])
      continue
    }

    const [winner, ...absorbed] = [...members].sort(preferWinner)
    const aliases = new Set(winner.aliases || [])
    for (const game of absorbed) {
      if (game.title) aliases.add(game.title)
      for (const alias of game.aliases || []) aliases.add(alias)
    }
    aliases.delete(winner.title)

    collapsed.push({
      ...winner,
      offerCount: members.reduce((total, game) => total + (Number(game.offerCount) || 0), 0),
      aliases: [...aliases],
    })
  }
  return collapsed
}

/**
 * IDs dos registros absorvidos por `gameId`.
 * @param {string} gameId - ID do jogo aberto
 * @param {object[]} games - Catálogo completo
 * @returns {string[]} - IDs das variantes do mesmo jogo
 */
function variantIds(gameId, games) {
  const all = Array.isArray(games) ? games : []
  const game = all.find((candidate) => candidate?.id === gameId)
  if (!game) return []
  const key = displayKey(game)
  return all
    .filter((candidate) => candidate?.id !== gameId && displayKey(candidate) === key)
    .map((candidate) => candidate.id)
}

module.exports = {
  displayKey,
  groupVariants,
  collapseVariants,
  variantIds,
}
