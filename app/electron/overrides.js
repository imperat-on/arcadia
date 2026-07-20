const fs = require("fs")

// Edições do usuário (capa, descrição, oculto…) por id de jogo.
//
// Mora FORA do library.json de propósito: o index.py reescreve aquele arquivo
// inteiro a cada scan e levaria as edições junto. Aqui elas sobrevivem, e o
// readLibrary() as aplica por cima do que o indexador achou.

function readOverrides(file) {
  try {
    const o = JSON.parse(fs.readFileSync(file, "utf-8"))
    return o && typeof o === "object" && !Array.isArray(o) ? o : {}
  } catch {
    return {}
  }
}

// Aplica um patch no override de um jogo. Campo com null/undefined desfaz a
// edição (volta ao valor original do indexador); jogo sem nenhuma edição sai
// do arquivo, para não acumular lixo.
function setOverride(file, id, patch) {
  const all = readOverrides(file)
  const next = { ...(all[id] || {}), ...(patch || {}) }
  for (const k of Object.keys(next)) {
    if (next[k] === null || next[k] === undefined) delete next[k]
  }
  if (Object.keys(next).length) all[id] = next
  else delete all[id]
  // Atômico: as edições de arte/nome dos jogos são trabalho manual do usuário.
  // Uma queda no meio da gravação apagaria tudo de uma vez.
  const tmp = `${file}.tmp`
  fs.writeFileSync(tmp, JSON.stringify(all, null, 2), "utf-8")
  fs.renameSync(tmp, file)
  return all
}

// Aplica as edições sobre a lista vinda do library.json.
function applyOverrides(games, overrides) {
  for (const g of games) {
    const o = overrides[g.id]
    if (o) Object.assign(g, o)
  }
  return games
}

// Decide se a arte anterior de um jogo pode ser apagada ao trocar a imagem.
// Só apaga o que é NOSSO (vive em art/): as capas do indexador apontam para o
// cache da Steam/Heroic e apagá-las estragaria a instalação do usuário.
function artToDelete(anterior, artDir, sep = "/") {
  if (typeof anterior !== "string" || !anterior) return null
  const p = anterior.replace(/^file:\/\//, "")
  const raiz = artDir.endsWith(sep) ? artDir : artDir + sep
  if (!p.startsWith(raiz)) return null
  // Barra a fuga por ".." (art/../../coisa.png resolveria fora da pasta).
  if (p.includes("..")) return null
  return p
}

module.exports = { readOverrides, setOverride, applyOverrides, artToDelete }
