// Pontes entre ESPAÇOS DE CHAVE diferentes para a MESMA conquista.
//
// O apiname não é estável entre as fontes que o Arcadia lê. O bin da Steam
// entrega o apiname real (ex. "ACH01"); a página pública da Steam parou de
// expor o apiname no HTML, então o fallback de scrape gerava "ach_01"
// (posicional, e numa ordem que NÃO é a da API pública). Resultado: a mesma
// conquista ficava com chaves diferentes em cada máquina — o desbloqueio
// empurrado pela máquina A não casava com o schema local da máquina B, o item
// criado pelo pull ficava sem dono e o reload do schema o descartava. A
// conquista sumia, silenciosamente, a cada boot.
//
// Aqui ficam as três pontes, na ordem de confiança:
//   1. apiname exato;
//   2. apelido (aliases[]) — apinames já conhecidos para o mesmo item;
//   3. título normalizado — só quando o título é ÚNICO dentro do appid.
// E a memória dos apelidos, para o casamento ser exato nas próximas vezes (e
// sobreviver a diferenças de tradução).

// Título comparável: sem acento, sem caixa, sem espaço duplicado. "Pé de
// valsa" e "Pe de  valsa" são a mesma conquista.
function normalizeTitle(value) {
  return String(value ?? "")
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/\s+/g, " ")
    .trim()
}

// apiname canônico do item + todos os apelidos conhecidos, sem repetição.
function aliasesOf(item) {
  if (!item) return []
  const out = []
  const add = (value) => {
    const nome = String(value ?? "").trim()
    if (nome && !out.includes(nome)) out.push(nome)
  }
  add(item.apiname)
  for (const alias of Array.isArray(item.aliases) ? item.aliases : []) add(alias)
  return out
}

// Guarda um apiname extra no item. Devolve true quando o item mudou (o caller
// usa isso para saber se precisa regravar). Nunca guarda o apiname do próprio
// item nem duplicata.
function rememberAlias(item, apiname) {
  if (!item) return false
  const nome = String(apiname ?? "").trim()
  if (!nome || nome === String(item.apiname ?? "").trim()) return false
  const list = Array.isArray(item.aliases) ? item.aliases : (item.aliases = [])
  if (list.includes(nome)) return false
  list.push(nome)
  return true
}

/**
 * Índice de busca de um appid: apinames/apelidos (case-insensitive) e títulos.
 * Título repetido dentro do appid vira `null` — ambíguo, nunca casa por título
 * (marcar a conquista errada é pior do que não marcar).
 */
function buildIndex(items) {
  const byApiname = new Map()
  const byTitle = new Map()
  for (const item of Array.isArray(items) ? items : []) {
    if (!item) continue
    for (const nome of aliasesOf(item)) {
      const chave = nome.toLowerCase()
      if (!byApiname.has(chave)) byApiname.set(chave, item)
    }
    const titulo = normalizeTitle(item.title)
    if (!titulo) continue
    byTitle.set(titulo, byTitle.has(titulo) ? null : item)
  }
  return { byApiname, byTitle }
}

/** Acha o item do appid por apiname exato → apelido → título único. */
function findItem(index, { apiname, title } = {}) {
  if (!index) return null
  const chave = String(apiname ?? "")
    .trim()
    .toLowerCase()
  if (chave) {
    const achado = index.byApiname.get(chave)
    if (achado) return achado
  }
  const titulo = normalizeTitle(title)
  if (titulo && index.byTitle.has(titulo)) {
    const achado = index.byTitle.get(titulo)
    if (achado) return achado
  }
  return null
}

module.exports = { normalizeTitle, aliasesOf, rememberAlias, buildIndex, findItem }
