// `fetch` do processo principal, feito pela pilha de rede do Chromium.
//
// O `fetch` global do Node resolve nomes pelo getaddrinfo do sistema e não
// guarda nada: cada conexão nova refaz a consulta DNS. Numa máquina cujo
// resolvedor primário está lento ou morto — o DNS do roteador, tipicamente —
// isso custa segundos POR CONEXÃO, e como o keep-alive cai depois de poucos
// segundos ociosos, quase toda ação da loja pagava o preço de novo. Medido
// nesta máquina: 3,4s para a primeira busca, 3,4s de novo depois de 9s parado.
//
// O `net.fetch` do Electron usa o mesmo resolvedor e o mesmo pool de conexões
// do navegador, que cacheiam. Mesma medição: 3,4s na primeira vez e 213ms
// depois de 9s ocioso.
//
// A API é a mesma do fetch padrão (headers, method, signal, redirecionamentos),
// então dá para trocar sem mexer em quem chama. Fora do Electron — nos scripts
// avulsos de teste — cai no fetch global.
let netFetch = null

function obter() {
  if (netFetch) return netFetch
  try {
    const { net, app } = require("electron")
    // net.fetch só existe depois do app pronto; antes disso, fetch global.
    if (net?.fetch && app?.isReady?.()) netFetch = net.fetch.bind(net)
  } catch {}
  return netFetch
}

function saneOpts(opts = {}) {
  if (!opts.headers) return opts
  const headers = {}
  for (const [k, v] of Object.entries(opts.headers)) {
    if (v == null) continue
    headers[k] = String(v).replace(/[^\x00-\xFF]/g, "")
  }
  return { ...opts, headers }
}

// Endereço do backend: primário + reserva. Um DNS ruim ou uma queda do provedor
// não podem derrubar o app quando existe um segundo caminho. Só erro de REDE
// (o fetch lança: DNS, conexão, TLS) troca de endereço — resposta HTTP com erro
// (401, 404) é resposta legítima do servidor e não se repete.
const { urls } = require("./supabase/config")

let preferido = 0

/** O endereço da lista que esta URL está usando, ou null se for URL externa. */
function baseDaUrl(url) {
  if (typeof url !== "string") return null
  return urls.find((u) => url.startsWith(u)) || null
}

async function fetchRede(url, opts) {
  const f = obter()
  const o = saneOpts(opts)
  const chamar = (alvo) => (f ? f(alvo, o) : fetch(alvo, o))

  const base = baseDaUrl(url)
  // URL externa (loja, CDN, SteamSpy): nada a fazer, comportamento de sempre.
  if (!base) return chamar(url)

  // Tenta primeiro o endereço que respondeu por último e, se a rede falhar,
  // os outros. Requisição abortada pelo chamador e corpo em stream não são
  // falha de rede reenviável: propagam na hora.
  const ordem = [...new Set([urls[preferido], ...urls])].filter(Boolean)
  let ultimoErro
  for (const alvo of ordem) {
    const destino = alvo === base ? url : alvo + url.slice(base.length)
    try {
      const resposta = await chamar(destino)
      preferido = urls.indexOf(alvo)
      return resposta
    } catch (erro) {
      ultimoErro = erro
      if (o?.signal?.aborted) throw erro
      if (o?.body && typeof o.body?.pipe === "function") throw erro
    }
  }
  throw ultimoErro
}

// Fetch com CONTROLE MANUAL de redirects. O net.fetch do Chromium NÃO suporta
// `redirect: "manual"`: em QUALQUER resposta 3xx ele falha com
// "Redirect was cancelled" (comprovado 2026-09-11 com httpbin e datanodes).
// Onde precisamos inspecionar/validar cada salto (anti-SSRF no download),
// usamos o fetch global do Node (undici), que segue o padrão WHATWG.
function fetchManual(url, opts) {
  const o = saneOpts(opts)
  return fetch(url, o)
}

module.exports = { fetchRede, fetchManual }
