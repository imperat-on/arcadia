"use strict"

// Escopo por CONTA dos arquivos locais do launcher (biblioteca, conquistas,
// horas jogadas, jogos custom, fila de sync...).
//
// Antes: tudo em DATA_DIR raiz — ao trocar de conta, a conta nova via os
// mesmos dados da anterior (conquistas/horas/biblioteca vazavam).
// Agora: logado como <user> → arquivos em DATA_DIR/contas/<user>/;
// deslogado (guest) → raiz (como sempre foi).
//
// Migração: a PRIMEIRA conta que logar numa máquina herda os arquivos da raiz
// (os dados já existentes passam a ser dela). Contas seguintes começam vazias.

const fs = require("fs")
const os = require("os")
const path = require("path")

// Mesmo padrão do sync.js/main.js: honra ARCADIA_DATA_DIR se definido
const DATA_DIR =
  process.env.ARCADIA_DATA_DIR ||
  path.join(os.homedir(), ".local/share/arcadia")
const CONTAS_DIR = path.join(DATA_DIR, "contas")

// Arquivos que pertencem à conta (não são globais da máquina)
const ARQS_CONTA = [
  "library.json",
  "achievements.json",
  "overrides.json",
  "custom_games.json",
  "pending_games.json",
  "game_settings.json",
  "profile_cache.json",
  "sync_queue.json",
  "friends_cache.json",
]

// Marcador: quando existe, a raiz já foi herdada por alguma conta — as
// próximas começam vazias (cada conta tem os DADOS DELA, não da máquina).
const MARCADOR = path.join(CONTAS_DIR, ".migrado")

let contaAtiva = null // username logado; null = guest (raiz)

function conta() {
  return contaAtiva
}

/** Caminho escopado: raiz se guest, senão contas/<user>/<arquivo>. */
function caminhoConta(base) {
  if (!contaAtiva) return base
  const dir = path.join(CONTAS_DIR, contaAtiva)
  fs.mkdirSync(dir, { recursive: true })
  return path.join(dir, path.basename(base))
}

/**
 * Migra os dados da raiz pra conta — só a PRIMEIRA conta da máquina herda
 * os dados existentes (biblioteca/horas/conquistas atuais). Depois do
 * marcador, contas novas começam do zero (separação total).
 */
function migrarConta(username) {
  const dir = path.join(CONTAS_DIR, username)
  if (fs.existsSync(dir)) return
  fs.mkdirSync(dir, { recursive: true })
  const herdarRaiz = !fs.existsSync(MARCADOR)
  if (herdarRaiz) {
    for (const f of ARQS_CONTA) {
      const src = path.join(DATA_DIR, f)
      if (fs.existsSync(src)) {
        try {
          fs.copyFileSync(src, path.join(dir, f))
        } catch {
          /* best effort */
        }
      }
    }
    try {
      fs.writeFileSync(MARCADOR, new Date().toISOString())
    } catch {
      /* ignore */
    }
  }
}

/** Troca a conta ativa (null = deslogado/guest). */
function definirConta(username) {
  contaAtiva = username || null
  if (contaAtiva) migrarConta(contaAtiva)
}

module.exports = { conta, caminhoConta, definirConta, DATA_DIR, CONTAS_DIR, ARQS_CONTA, MARCADOR }
