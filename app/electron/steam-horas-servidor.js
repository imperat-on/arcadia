"use strict"

/**
 * Horas da Steam no servidor.
 *
 * O jogo pode ter tempo jogado em contas Steam DIFERENTES, e tempos de contas
 * diferentes são tempos diferentes de verdade (só uma conta fica logada por vez).
 * Então a soma entre contas é legítima — mas a mesma conta pode aparecer em duas
 * fontes ao mesmo tempo:
 *
 *   - local: o arquivo da conta VINCULADA nesta máquina (`localconfig.vdf`);
 *   - servidor: o que já subiu de qualquer máquina/conta.
 *
 * Somar as duas fontes contaria a mesma hora duas vezes. Por isso a regra, igual à
 * do servidor: por (jogo, conta) vale o MAIOR valor visto, e só então as contas
 * somam. O tempo medido pelo Arcadia não entra aqui em nenhum momento: o arquivo da
 * Steam já inclui o tempo das sessões que o app lançou.
 */

/** Chave canônica de um jogo: o número do appid dentro do id (`steam:990080`). */
function chaveAppid(valor) {
  if (valor === null || valor === undefined) return ""
  const texto = String(valor)
  const achado = texto.match(/\d{2,}/)
  return achado ? achado[0] : ""
}

/**
 * @param {Record<string, number>} locais minutos por appid lidos nesta máquina
 * @param {string} steamid conta vinculada nesta máquina
 * @returns {Record<string, Record<string, number>>} { appid: { steamid: minutos } }
 */
function locaisPorConta(locais, steamid) {
  const conta = String(steamid || "")
  const saida = {}
  if (!conta) return saida
  for (const [appid, minutos] of Object.entries(locais || {})) {
    const chave = chaveAppid(appid)
    const valor = Number(minutos) || 0
    if (!chave || valor <= 0) continue
    if (!saida[chave]) saida[chave] = {}
    saida[chave][conta] = valor
  }
  return saida
}

/**
 * Combina as linhas do servidor (`[{appid, steamid, minutes}]`) com as locais.
 * Por (jogo, conta) vence o maior — reenviar não infla, e o que veio de outra
 * máquina não some por um total menor lido agora.
 *
 * @param {Array<{appid:string, steamid:string, minutes:number|string}>} doServidor
 * @param {Record<string, Record<string, number>>} locais
 */
function mesclarHoras(doServidor, locais) {
  const saida = {}
  const guardar = (appidBruto, steamidBruto, minutosBruto) => {
    const appid = chaveAppid(appidBruto)
    const steamid = String(steamidBruto || "")
    const minutos = Number(minutosBruto) || 0
    if (!appid || !steamid || minutos <= 0) return
    if (!saida[appid]) saida[appid] = {}
    saida[appid][steamid] = Math.max(saida[appid][steamid] || 0, minutos)
  }
  for (const linha of Array.isArray(doServidor) ? doServidor : []) {
    guardar(linha?.appid, linha?.steamid, linha?.minutes)
  }
  for (const [appid, contas] of Object.entries(locais || {})) {
    for (const [steamid, minutos] of Object.entries(contas || {})) guardar(appid, steamid, minutos)
  }
  return saida
}

/** Soma as contas de cada jogo: { appid: minutos } — o que a tela consome. */
function somarPorAppid(mapa) {
  const saida = {}
  for (const [appid, contas] of Object.entries(mapa || {})) {
    let total = 0
    for (const minutos of Object.values(contas || {})) total += Number(minutos) || 0
    if (total > 0) saida[appid] = total
  }
  return saida
}

/**
 * Junta as leituras de VÁRIAS contas locais num só mapa `appid -> { steamid: min }`.
 * Cada conta entra com o seu próprio steamid, então elas somam — e a mesma conta
 * vinda do servidor depois é resolvida pelo "maior vence" em mesclarHoras.
 *
 * @param {Array<{horas: Record<string, number>, steamid: string}>} leituras
 */
function locaisDeContas(leituras) {
  const saida = {}
  for (const leitura of Array.isArray(leituras) ? leituras : []) {
    const porConta = locaisPorConta(leitura?.horas, leitura?.steamid)
    for (const [appid, contas] of Object.entries(porConta)) {
      if (!saida[appid]) saida[appid] = {}
      for (const [steamid, minutos] of Object.entries(contas)) {
        saida[appid][steamid] = Math.max(saida[appid][steamid] || 0, minutos)
      }
    }
  }
  return saida
}

/**
 * Itens para enviar ao servidor: o TOTAL de cada jogo nesta conta (absoluto, não
 * delta) — o servidor guarda o maior por (jogo, conta).
 */
function itensParaEnviar(locais, steamid, persona = "") {
  const conta = String(steamid || "")
  if (!conta) return []
  const itens = []
  for (const [appid, minutos] of Object.entries(locais || {})) {
    const chave = chaveAppid(appid)
    const valor = Number(minutos) || 0
    if (!chave || valor <= 0) continue
    itens.push({ appid: chave, steamid: conta, minutes: valor, persona: String(persona || "") })
  }
  return itens
}

module.exports = {
  chaveAppid,
  locaisPorConta,
  locaisDeContas,
  mesclarHoras,
  somarPorAppid,
  itensParaEnviar,
}
