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

// Set dos ids possuídos pela conta ativa. Guest não tem posse (a UI dele
// já vê tudo via filtrarPorPosse), então devolve conjunto vazio.
// Retorna NULL quando o arquivo está ausente ou corrompido — o chamador
// (push) DEVE tratar null como "posse desconhecida; não remova nada".
// Devolver Set() vazio aqui fazia o push marcar TODOS os jogos como
// removed — apagando a biblioteca do servidor e propagando o wipe.
function ownedSet() {
  if (!conta()) return new Set()
  const raw = readOwned()
  return raw === null ? null : new Set(raw)
}

// Grava atômico (tmp+rename): uma queda no meio não pode deixar o
// owned_games.json truncado, senão a conta perde acesso a jogos que já tinha.
// O nome tmp único (pid+ts) evita colisão entre processos concorrentes.
function gravarOwned(ids) {
  const alvo = caminhoConta(OWNED_GAMES)
  const tmp = `${alvo}.tmp-${process.pid}-${Date.now()}`
  fs.writeFileSync(tmp, JSON.stringify(ids))
  fs.renameSync(tmp, alvo)
}

// Registra o id como possuído da conta ativa. Guest não tem arquivo de posse,
// vê tudo sempre, então é no-op, não cria owned_games.json na raiz.
function ownedAdd(id) {
  if (!conta()) return
  const ids = readOwned() || []
  if (ids.includes(id)) return
  ids.push(id)
  gravarOwned(ids)
}

// Tira o id da posse da conta ativa. No-op se guest ou se o id não estava lá,
// nada pra gravar, evita criar owned_games.json à toa.
function ownedRemove(id) {
  if (!conta()) return
  const ids = readOwned()
  if (ids === null || !ids.includes(id)) return
  gravarOwned(ids.filter((x) => x !== id))
}

module.exports = {
  OWNED_GAMES,
  filtrarPorPosse,
  readOwned,
  materializarPosse,
  ownedSet,
  ownedAdd,
  ownedRemove,
}
