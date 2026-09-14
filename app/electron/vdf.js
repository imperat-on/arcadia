"use strict"

// Parser mínimo de KeyValues (VDF) da Valve, em módulo puro e testável.
//
// Existe porque regex não dá conta: `loginusers.vdf` e `localconfig.vdf` são
// estruturas ANINHADAS ("apps" { "990080" { "Playtime" "4907" } }), e um regex
// sobre `{...}` para no primeiro fecha-chave interno — foi assim que um teste
// rápido não achou o `MostRecent` de cada conta.
//
// O formato é simples:
//   "chave" "valor"          -> par
//   "chave" { ... }          -> objeto
//   // comentário            -> ignorado
// Chaves repetidas viram lista, como na leitura da Valve.

function parseVDF(texto) {
  const src = String(texto || "")
  let i = 0

  function erro(msg) {
    throw new Error(`VDF inválido na posição ${i}: ${msg}`)
  }

  function pularEspaco() {
    while (i < src.length) {
      const c = src[i]
      if (c === " " || c === "\t" || c === "\r" || c === "\n") {
        i++
        continue
      }
      if (c === "/" && src[i + 1] === "/") {
        while (i < src.length && src[i] !== "\n") i++
        continue
      }
      break
    }
  }

  function lerString() {
    // Sempre entre aspas no arquivo da Valve.
    if (src[i] !== '"') erro("esperava uma string entre aspas")
    i++
    let out = ""
    while (i < src.length && src[i] !== '"') {
      if (src[i] === "\\" && i + 1 < src.length) {
        const n = src[i + 1]
        out += n === "n" ? "\n" : n === "t" ? "\t" : n
        i += 2
        continue
      }
      out += src[i]
      i++
    }
    i++ // fecha aspas
    return out
  }

  function porValor(obj, chave, valor) {
    if (!(chave in obj)) {
      obj[chave] = valor
      return
    }
    if (Array.isArray(obj[chave])) obj[chave].push(valor)
    else obj[chave] = [obj[chave], valor]
  }

  function bloco() {
    const obj = {}
    for (;;) {
      pularEspaco()
      if (i >= src.length) return obj
      if (src[i] === "}") {
        i++
        return obj
      }
      if (src[i] !== '"') erro("esperava uma chave")
      const chave = lerString()
      pularEspaco()
      if (src[i] === "{") {
        i++
        porValor(obj, chave, bloco())
      } else if (src[i] === '"') {
        porValor(obj, chave, lerString())
      } else {
        erro(`chave "${chave}" sem valor nem bloco`)
      }
    }
  }

  return bloco()
}

/** Contas do loginusers.vdf: steamid64 -> { persona, maisRecente } */
function contasDoLoginUsers(texto) {
  const raiz = parseVDF(texto)
  const users = raiz && raiz.users ? raiz.users : {}
  const contas = []
  for (const [steamid, bloco] of Object.entries(users)) {
    if (!/^\d{17}$/.test(steamid) || !bloco || typeof bloco !== "object") continue
    contas.push({
      steamid,
      persona: String(bloco.PersonaName || ""),
      maisRecente: String(bloco.MostRecent || "") === "1",
    })
  }
  return contas
}

/** A conta que a Steam considera ativa agora (MostRecent=1), se houver. */
function contaAtivaDoLoginUsers(texto) {
  const contas = contasDoLoginUsers(texto)
  return contas.find((c) => c.maisRecente) || null
}

/** Minutos por appid a partir do localconfig.vdf de uma conta. */
function horasDoLocalConfig(texto) {
  const raiz = parseVDF(texto)
  const apps = (raiz && raiz.UserLocalConfigStore && raiz.UserLocalConfigStore.apps) || {}
  const horas = {}
  for (const [appid, dado] of Object.entries(apps)) {
    // appid 0 é entrada da própria Steam ("LastPlayed"/"Playtime" da loja), não jogo.
    if (!/^\d+$/.test(appid) || Number(appid) <= 0 || !dado || typeof dado !== "object") continue
    const min = Number(dado.Playtime)
    if (Number.isFinite(min) && min > 0) horas[appid] = min
  }
  return horas
}

/** steamid64 <-> id de conta (nome da pasta em userdata/). */
const BASE_STEAMID = 76561197960265728n
function steamidParaContaId(steamid) {
  try {
    return String(BigInt(steamid) - BASE_STEAMID)
  } catch {
    return ""
  }
}
function contaIdParaSteamid(contaId) {
  try {
    return String(BASE_STEAMID + BigInt(contaId))
  } catch {
    return ""
  }
}

module.exports = {
  parseVDF,
  contasDoLoginUsers,
  contaAtivaDoLoginUsers,
  horasDoLocalConfig,
  steamidParaContaId,
  contaIdParaSteamid,
}
