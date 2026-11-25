"use strict"

// Funde itens que são A MESMA conquista, sem descartar informação.
//
// Por que existe: no rebuild do loader, o casamento é por `apiname`. Quando um
// jogo foi lido primeiro SEM o bin da Steam (repack/crackeado), o app cria o
// schema sintético (`ach_01`, `ach_02`...). Depois, com o bin presente, chega o
// schema REAL (`PFA_1`, ...). Os apinames não casam, e a regra correta de
// "nunca descartar o que não reconhece" preserva os sintéticos como ÓRFÃOS — o
// casamento por título grava o vínculo em `aliases`/`remoteApiname`, mas os dois
// itens continuam na lista. Resultado no Hogwarts Legacy: 47 itens para 45
// conquistas e 44 desbloqueadas em vez de 42 (cada par contava nos dois lados).
//
// Hora de fundir. Um item sobrevive e herda do outro; nada é descartado:
//
//   - só funde quando é seguro: MESMO título normalizado E vínculo explícito de
//     apiname entre eles (aliases/remoteApiname). Título igual sozinho não basta
//     (dois achievements diferentes podem se chamar "Complete the game");
//   - sobrevive o item do schema REAL (apiname em `reais`); empate no primeiro;
//   - `achieved` é OR e `unlock` o valor não-zero: desbloqueio em qualquer espaço
//     de chaves vale;
//   - `aliases` e `remoteApiname` dos dois são unidos, para o casamento (e o
//     push) continuar exato nas próximas sessões.

function normalizar(titulo) {
  return String(titulo || "")
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/\s+/g, " ")
    .trim()
}

function apinamesDe(item) {
  const nomes = new Set()
  if (item && item.apiname) nomes.add(String(item.apiname))
  if (item && item.remoteApiname) nomes.add(String(item.remoteApiname))
  for (const a of (item && item.aliases) || []) if (a) nomes.add(String(a))
  return nomes
}

/** O item tem vínculo explícito com o outro (um apiname em comum)? */
function ligados(a, b) {
  const na = apinamesDe(a)
  for (const n of apinamesDe(b)) if (na.has(n)) return true
  return false
}

function fundir(survivor, outro) {
  const saida = Object.assign({}, survivor)
  saida.achieved = Boolean(survivor.achieved) || Boolean(outro.achieved)
  saida.unlock = Number(survivor.unlock) || Number(outro.unlock) || 0
  saida.percent = Number(survivor.percent) || Number(outro.percent) || 0
  saida.icon = survivor.icon || outro.icon || ""
  saida.icongray = survivor.icongray || outro.icongray || ""
  saida.desc = survivor.desc || outro.desc || ""
  if (!saida.remoteApiname && outro.remoteApiname) saida.remoteApiname = outro.remoteApiname
  const aliases = new Set()
  for (const a of (survivor.aliases || [])) if (a) aliases.add(String(a))
  for (const a of (outro.aliases || [])) if (a) aliases.add(String(a))
  // O apiname do item que sai vira apelido do que fica: é o que garante que a
  // máquina que só conhece o outro espaço de chaves continue casando.
  if (outro.apiname && outro.apiname !== saida.apiname) aliases.add(String(outro.apiname))
  if (saida.apiname) aliases.delete(String(saida.apiname))
  if (saida.remoteApiname) aliases.delete(String(saida.remoteApiname))
  if (aliases.size) saida.aliases = [...aliases]
  else delete saida.aliases
  return saida
}

/**
 * @param {Array} itens lista já montada pelo loader (schema + órfãos preservados)
 * @param {Set<string>|Array<string>} reais apinames que vieram do schema de verdade
 * @returns {Array} a mesma lista, sem duplicatas de conquista, na mesma ordem
 */
function consolidarItens(itens, reais) {
  const lista = Array.isArray(itens) ? itens : []
  const doSchema = reais instanceof Set ? reais : new Set(reais || [])
  const saida = []
  let fundidos = 0

  for (const item of lista) {
    const titulo = normalizar(item && item.title)
    // Procura um item já aceito que seja a MESMA conquista.
    const alvo = titulo
      ? saida.find((c) => normalizar(c.title) === titulo && ligados(c, item))
      : null

    if (!alvo) {
      saida.push(item)
      continue
    }

    // Quem sobrevive: o do schema real; se os dois forem (ou nenhum), o que já
    // está na lista.
    const itemEhReal = doSchema.has(String(item.apiname))
    const alvoEhReal = doSchema.has(String(alvo.apiname))
    if (itemEhReal && !alvoEhReal) {
      const i = saida.indexOf(alvo)
      saida[i] = fundir(item, alvo)
    } else {
      const i = saida.indexOf(alvo)
      saida[i] = fundir(alvo, item)
    }
    fundidos++
  }

  if (fundidos && typeof process !== "undefined" && process.env && process.env.ARCADIA_DEBUG) {
    console.log(`[achievements] consolidados ${fundidos} item(ns) duplicado(s)`)
  }
  return saida
}

module.exports = { consolidarItens, normalizar, ligados }
