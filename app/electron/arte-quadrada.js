// Classificação pura da arte para o quadrado do Big Picture.
//
// O trilho do modo console mostra UM formato: quadrado 1:1 preenchido de ponta
// a ponta. Cada provedor devolve uma geometria diferente, e a única coisa que
// decide COMO pintar é a proporção da imagem — nunca a fonte nem o "kind" com
// que ela foi pedida:
//
//   quadrado   largura === altura       -> pinta como está (nada é cortado)
//   preencher  qualquer outra proporção -> recorte central (arte promocional,
//                                          em regra sem título embutido)
//   capa       retrato (altura>largura) -> último recurso: recorte central
//
// Um quadrado nativo nunca é cortado; um retrato nunca é esticado.

const MINIMO = 256
const PESO = { quadrado: 0, preencher: 1, capa: 2 }

function classificar(arte) {
  const url = String(arte?.url || "")
  const largura = Number(arte?.largura) || 0
  const altura = Number(arte?.altura) || 0
  if (!/^https:\/\//.test(url)) return null
  if (arte?.animado) return null
  if (largura < MINIMO || altura < MINIMO) return null
  if (largura === altura) return "quadrado"
  return altura > largura ? "capa" : "preencher"
}

function ordenar(candidatos) {
  const vistos = new Set()
  const saida = []
  for (const arte of candidatos || []) {
    const ajuste = classificar(arte)
    if (!ajuste || vistos.has(arte.url)) continue
    vistos.add(arte.url)
    saida.push({ ...arte, ajuste })
  }
  return saida.sort(
    (a, b) => PESO[a.ajuste] - PESO[b.ajuste] || b.largura * b.altura - a.largura * a.altura,
  )
}

module.exports = { classificar, ordenar, MINIMO }
