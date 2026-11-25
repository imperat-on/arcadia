// Blindagem de rede do processo principal.
//
// O erro que isso resolve (visto em produção em 2026-09-19):
//
//   TypeError: Cannot convert argument to a ByteString because the character at
//   index 81 has a value of 65533 which is greater than 255 (undici / ByteString)
//       at _Headers.set (undici:8913)
//       at ClientRequest.<anonymous> (node:electron/js2c/browser_init)
//
// O undici (fetch do Node e do Electron) recusa QUALQUER header — nome ou valor —
// com code point > 255. Basta um dado externo corrompido (um título com ® vindo em
// Latin-1, um cookie, um header de resolvedor) para o request estourar, e como isso
// acontece no processo principal o app inteiro cai com um dialog de "JavaScript error".
//
// Aqui TODO fetch global e todo net.request do main passam por saneamento antes de
// sair: code points > 255 são descartados (mesma regra do httpfetch.saneOpts) e o
// NOME do header também é limpo. Nenhum header vindo de fora derruba o app.
let app = null
let net = null
try {
  ;({ app, net } = require("electron"))
} catch {
  /* fora do Electron (testes): so o saneador e importado */
}

function limparNome(nome) {
  return String(nome).replace(/[^\x20-\x7E]/g, "")
}

function limparValor(valor) {
  return String(valor).replace(/[^\x00-\xFF]/g, "")
}

function sanear(headers) {
  if (!headers) return headers
  // Headers do bus: preserva a semântica (append/set) reconstruindo no formato de objeto.
  let pares = null
  try {
    pares = typeof headers.entries === "function" ? [...headers.entries()] : null
  } catch {}
  if (!pares) {
    try {
      pares = Object.entries(headers).map(([k, v]) => [k, v])
    } catch {
      return headers
    }
  }
  const limpo = {}
  for (const [k, v] of pares) {
    if (v == null) continue
    limpo[limparNome(k)] = limparValor(v)
  }
  return limpo
}

let instalado = false
function instalar() {
  if (instalado) return { ok: true, ja: true }
  instalado = true
  const relatorio = { fetch: false, net: false }

  // 1. fetch global (o caminho do undici - o do stack de erro)
  try {
    const origFetch = globalThis.fetch
    if (typeof origFetch === "function") {
      globalThis.fetch = function (input, init) {
        if (init && init.headers) {
          try {
            init = { ...init, headers: sanear(init.headers) }
          } catch {}
        }
        return origFetch.call(this, input, init)
      }
      relatorio.fetch = true
    }
  } catch {}

  // 2. net.request (o caminho do SimpleURLLoader do Electron)
  try {
    const origRequest = net.request
    net.request = function (opts) {
      try {
        if (opts && typeof opts === "object" && opts.headers) {
          opts = { ...opts, headers: sanear(opts.headers) }
        }
      } catch {}
      return origRequest.call(net, opts)
    }
    relatorio.net = true
  } catch {}

  try {
    console.log("[net-guard] instalado:", JSON.stringify(relatorio))
  } catch {}
  return { ok: relatorio.fetch && relatorio.net, ...relatorio }
}

// Tenta agora (pode vir antes do app pronto) e de novo no ready, porque o Electron
// troca o fetch global durante o bootstrap.
if (net) {
  instalar()
  try {
    app.whenReady().then(() => instalar())
  } catch {}
}

module.exports = { instalar, sanear }
