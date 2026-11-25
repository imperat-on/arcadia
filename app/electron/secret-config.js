"use strict"

// Regra de gravacao das chaves secretas que chegam do renderer (auditoria A-06).
//
// O renderer NUNCA ve a chave real: `config:get` devolve a mascara (ex.: "abc•••yz").
// Entao o que volta no formulario so pode significar:
//
//   null / string vazia -> apagar a chave
//   a mascara igual a atual -> nao mexeu, mantem o valor real no disco
//   qualquer outro texto -> chave nova, grava (com trim)
//
// E um quarto caso, que e o que fazia a chave "continuar ativa": uma mascara que
// NAO corresponde a nada. Uma mascara nunca e uma chave, entao ela e descartada
// em vez de gravada. Se ela ja estiver gravada no disco (de uma versao anterior),
// e lixo: sai na proxima gravacao.
//
// A remocao viaja numa lista a parte porque `writeConfig` faz merge raso — chave
// apenas ausente do patch voltaria do disco.
function pareceMascara(v) {
  return typeof v === "string" && v.includes("•")
}

function resolverSegredos(cfg, atual, redigir, segredos) {
  const entrada = cfg && typeof cfg === "object" && !Array.isArray(cfg) ? cfg : {}
  const saida = { ...entrada }
  const remover = []
  const marcar = (k) => {
    if (!remover.includes(k)) remover.push(k)
  }

  // Estado do disco com a mascara tratada como AUSENTE (mascara gravada = lixo).
  const disco = {}
  const sujo = {}
  for (const k of segredos || []) {
    sujo[k] = pareceMascara(atual ? atual[k] : undefined)
    const v = atual ? atual[k] : ""
    disco[k] = sujo[k] || typeof v !== "string" ? "" : v
  }
  const mascarado = typeof redigir === "function" ? redigir(disco) || {} : {}

  for (const k of segredos || []) {
    if (!Object.prototype.hasOwnProperty.call(saida, k)) {
      if (sujo[k]) marcar(k) // limpa mascara gravada como chave
      continue
    }

    const novo = saida[k]
    const vazia = novo === null || novo === undefined || (typeof novo === "string" && novo.trim() === "")
    if (vazia) {
      delete saida[k]
      marcar(k)
      continue
    }
    if (typeof novo !== "string") continue

    const texto = novo.trim()
    if (pareceMascara(texto)) {
      if (disco[k] && texto === String(mascarado[k] || "").trim()) {
        saida[k] = disco[k] // formulario reenviado sem alteracao: mantem o real
      } else {
        // Mascara orfa: nunca gravar como chave.
        delete saida[k]
        if (sujo[k]) marcar(k)
      }
      continue
    }

    saida[k] = texto
  }

  return { cfg: saida, remover }
}

module.exports = { resolverSegredos, pareceMascara }
