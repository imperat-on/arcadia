"use strict"

// O catch-up de desbloqueios antigos (syncAllLocal) subia o apiname LOCAL. Numa
// máquina cujo schema ficou sintético do scrape ("ach_15", posicional) isso cria
// uma SEGUNDA linha no servidor para a mesma conquista — a outra máquina conhece
// essa conquista pelo nome real ("ACH19"), e o servidor chaveia por
// (user_id, appid, apiname). Resultado: contagem inflada, que é a classe de bug
// que já apareceu como "47 itens/44 desbloqueadas para 45 conquistas".
//
// A regra é a documentada: `item.remoteApiname || item.apiname` — a chave do
// servidor manda. Os outros dois caminhos de push (enqueue no main.js e o loader
// de schema) já seguiam essa regra; este era o que faltava.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")

const DIR = fs.mkdtempSync(path.join(os.tmpdir(), "arcadia-sync-catchup-"))
process.env.ARCADIA_DATA_DIR = DIR
const conta = require("../electron/supabase/conta.js")
const sync = require("../electron/supabase/sync.js")
const { getClient } = require("../electron/supabase/client.js")

test.after(() => {
  try {
    fs.rmSync(DIR, { recursive: true, force: true })
  } catch {}
})

test("catch-up sobe a chave do SERVIDOR (remoteApiname), nunca a local sintética", async () => {
  conta.definirConta("alice")
  const arq = path.join(DIR, "contas", "alice", "achievements.json")
  fs.mkdirSync(path.dirname(arq), { recursive: true })
  fs.writeFileSync(
    arq,
    JSON.stringify({
      "1547000": {
        items: [
          // item ach_15 do caso real: chave sintética + nome real já conhecido
          { apiname: "ach_15", title: "Tira meu gatinho da árvore?", achieved: true, unlock: 1789535204, remoteApiname: "ACH19", block: 14, bit: 0 },
          // sem remoteApiname (nunca veio do servidor): vale a chave local
          { apiname: "ach_20", title: "Faz-tudo", achieved: true, unlock: 1789538322, aliases: ["ACH12"], block: 19, bit: 0 },
          // não desbloqueada: não pode subir
          { apiname: "ach_17", title: "Vou querer dois números 9", achieved: false, unlock: 0, block: 16, bit: 0 },
        ],
      },
    }),
  )

  const chamadas = []
  const client = getClient()
  client.auth.getUser = async () => ({ data: { user: { id: "alice-id" } }, error: null })
  client.rpc = async (nome, args) => {
    chamadas.push({ nome, args })
    return { data: [], error: null }
  }

  const r = await sync.syncAllLocal()
  assert.equal(r.ok, true, JSON.stringify(r))

  const envio = chamadas.find((c) => c.nome === "sync_achievements")
  assert.ok(envio, "o catch-up chamou sync_achievements")
  const nomes = envio.args.p_items.map((i) => i.apiname)
  assert.deepEqual(nomes.sort(), ["ACH19", "ach_20"], "remoteApiname quando existe; a chave local só quando não há nome do servidor")
  assert.ok(!nomes.includes("ach_15"), "a chave sintética não pode subir quando o servidor já conhece o nome real")
})
