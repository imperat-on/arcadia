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
const path = require("path")
const { log } = require("./../debug")
const { getDataDir } = require("../runtime-paths")

// Todos os arquivos locais partem da mesma raiz, inclusive em testes.
const DATA_DIR = getDataDir()
const CONTAS_DIR = path.join(DATA_DIR, "contas")

// library.json NÃO entra: é um snapshot global legado na raiz.
const ARQS_CONTA = [
  "achievements.json",
  "overrides.json",
  "custom_games.json",
  "pending_games.json",
  "game_settings.json",
  "profile_cache.json",
  "sync_queue.json",
  "friends_cache.json",
  "owned_games.json",
]

// Marcador: quando existe, a raiz já foi herdada por alguma conta — as
// próximas começam vazias (cada conta tem os DADOS DELA, não da máquina).
const MARCADOR = path.join(CONTAS_DIR, ".migrado")

let contaAtiva = null // username logado, null = guest (raiz)

function conta() {
  return contaAtiva
}

// Dirs de conta já criados. O mkdir roda na primeira caminhoConta depois do
// login (migrarConta já cria a pasta) e fica memoizado, senão vira um syscall
// a cada chamada. E o watcher de conquistas chama isso a cada poll de 15s.
const criados = new Set()

/** Caminho escopado: raiz se guest, senão contas/<user>/<arquivo>. */
function caminhoConta(base) {
  if (!contaAtiva) return base
  const dir = path.join(CONTAS_DIR, contaAtiva)
  if (!criados.has(dir)) {
    fs.mkdirSync(dir, { recursive: true })
    criados.add(dir)
  }
  return path.join(dir, path.basename(base))
}

/** Caminho de um arquivo de dados da conta, em DATA_DIR/contas/<user>/<nome>. */
function caminhoArquivoConta(nome) {
  return caminhoConta(path.join(DATA_DIR, nome))
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
        } catch (e) {
          log("conta/migrar", e)
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

module.exports = { conta, caminhoConta, caminhoArquivoConta, definirConta, DATA_DIR, CONTAS_DIR, ARQS_CONTA, MARCADOR }
