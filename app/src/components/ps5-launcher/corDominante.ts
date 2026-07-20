// Cor dominante de uma capa, para a loja tingir a interface conforme o jogo
// focado. O CDN da Steam responde com Access-Control-Allow-Origin: *, então o
// canvas não é contaminado e dá para ler os pixels.

const cache = new Map<string, string>()
const emVoo = new Map<string, Promise<string>>()

// Reduzimos a capa para 16×16 antes de ler: são 256 pixels em vez de 540 mil,
// e a média de cor não muda de forma perceptível. Ler a imagem inteira a cada
// movimento do foco travaria a navegação.
const LADO = 16

function media(dados: Uint8ClampedArray): string {
  let r = 0
  let g = 0
  let b = 0
  let n = 0
  for (let i = 0; i < dados.length; i += 4) {
    const [pr, pg, pb, pa] = [dados[i], dados[i + 1], dados[i + 2], dados[i + 3]]
    if (pa < 128) continue
    const luz = (pr + pg + pb) / 3
    // Quase preto e quase branco dominam capas com bordas escuras ou logo
    // claro, e puxariam a média para um cinza sem identidade nenhuma.
    if (luz < 24 || luz > 232) continue
    r += pr
    g += pg
    b += pb
    n++
  }
  if (!n) return ""
  // Satura: a média de uma arte inteira tende ao cinza, e o objetivo é dar cor
  // ao ambiente, não reproduzir a capa. O clamp nos DOIS lados é obrigatório —
  // sem o piso, a saturação empurrava canais para negativo e saía
  // "rgb(249,115,-34)", que é CSS inválido e apaga a variável inteira.
  const mr = r / n
  const mg = g / n
  const mb = b / n
  const luz = (mr + mg + mb) / 3
  const limite = (c: number) => Math.max(0, Math.min(255, Math.round(c)))
  let cr = limite(luz + (mr - luz) * 1.5)
  let cg = limite(luz + (mg - luz) * 1.5)
  let cb = limite(luz + (mb - luz) * 1.5)

  // Capa em tons de cinza (Hollow Knight, por exemplo) daria um brilho sujo
  // que não diz nada. Melhor devolver vazio e deixar cair no accent do tema.
  if (Math.max(cr, cg, cb) - Math.min(cr, cg, cb) < 25) return ""

  // A média costuma vir escura demais para servir de brilho. Clareamos até uma
  // luminância mínima, preservando a proporção entre os canais (a matiz).
  const claro = Math.max(cr, cg, cb)
  if (claro < 150) {
    const k = 150 / (claro || 1)
    cr = limite(cr * k)
    cg = limite(cg * k)
    cb = limite(cb * k)
  }
  return `rgb(${cr}, ${cg}, ${cb})`
}

export function corDominante(url: string): Promise<string> {
  if (!url) return Promise.resolve("")
  const pronta = cache.get(url)
  if (pronta !== undefined) return Promise.resolve(pronta)
  const jaVoando = emVoo.get(url)
  if (jaVoando) return jaVoando

  const p = new Promise<string>((resolve) => {
    const img = new Image()
    img.crossOrigin = "anonymous" // sem isto o canvas fica contaminado
    img.onload = () => {
      try {
        const cv = document.createElement("canvas")
        cv.width = LADO
        cv.height = LADO
        const ctx = cv.getContext("2d", { willReadFrequently: true })
        if (!ctx) return resolve("")
        ctx.drawImage(img, 0, 0, LADO, LADO)
        resolve(media(ctx.getImageData(0, 0, LADO, LADO).data))
      } catch {
        resolve("") // quem chama cai no accent
      }
    }
    img.onerror = () => resolve("")
    img.src = url
  }).then((cor) => {
    cache.set(url, cor)
    emVoo.delete(url)
    return cor
  })

  emVoo.set(url, p)
  return p
}
