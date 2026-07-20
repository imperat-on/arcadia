import type { JogoLinha } from "./types"

// Caminho ANTIGO do CDN, sem hash no meio. Continua valendo para o catálogo
// consolidado (~98% do que aparece em "Em alta"), e é o único que podemos
// montar só com o appid. Jogos publicados no esquema novo servem os assets em
// /store_item_assets/steam/apps/<appid>/<hash>/..., que não dá para adivinhar —
// para esses, a URL só chega pronta pela API, no campo `cover`.
const CDN = "https://cdn.akamai.steamstatic.com/steam/apps"

/**
 * Artes em formato RETRATO (3:4), na ordem de tentativa.
 *
 * A `library_600x900` vem antes do `cover` de propósito: o backend preenche
 * `cover` com um header 460x215 para quase toda lista, e colocá-lo primeiro
 * fazia TODO ladrilho exibir a arte horizontal esmagada numa moldura vertical,
 * mesmo quando o jogo tinha capa retrato. Só entra aqui o `cover` que já é
 * reconhecidamente vertical.
 */
export function urlsRetrato(jogo: JogoLinha): string[] {
  const urls = [`${CDN}/${jogo.appid}/library_600x900.jpg`]
  if (jogo.cover && /library_600x900|600x900|660x930/.test(jogo.cover)) urls.unshift(jogo.cover)
  return [...new Set(urls)]
}

/**
 * Artes em formato PAISAGEM, na ordem de tentativa. O `cover` vem primeiro
 * porque, quando a API o entrega, é uma URL completa (com hash) — a única que
 * funciona para lançamentos recentes.
 */
export function urlsPaisagem(jogo: JogoLinha): string[] {
  const urls: string[] = []
  if (jogo.cover) urls.push(jogo.cover)
  urls.push(`${CDN}/${jogo.appid}/header.jpg`)
  urls.push(`${CDN}/${jogo.appid}/library_hero.jpg`)
  urls.push(`${CDN}/${jogo.appid}/capsule_616x353.jpg`)
  return [...new Set(urls)]
}

/** Compatibilidade com as chamadas existentes. */
export function urlsCapa(jogo: JogoLinha, formato: "retrato" | "paisagem" = "retrato"): string[] {
  return formato === "retrato" ? urlsRetrato(jogo) : urlsPaisagem(jogo)
}
