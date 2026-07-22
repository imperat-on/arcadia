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

function fetchRede(url, opts) {
  const f = obter()
  return f ? f(url, opts) : fetch(url, opts)
}

module.exports = { fetchRede }
