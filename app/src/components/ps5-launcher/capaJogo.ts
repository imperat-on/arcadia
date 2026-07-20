import type { JogoLinha } from "./types"

const CDN = "https://cdn.akamai.steamstatic.com/steam/apps"

/**
 * Devolve uma cascata de URLs de imagem para um jogo da loja.
 * O `cover` do backend (quando existe) vem primeiro; depois tentamos as artes
 * oficiais da Steam em ordem de adequação ao formato. A deduplicação evita
 * duas requisições para a mesma URL quando o backend já devolve uma arte do CDN.
 */
export function urlsCapa(jogo: JogoLinha, formato: "retrato" | "paisagem" = "retrato"): string[] {
  const appid = jogo.appid
  const urls: string[] = []
  if (jogo.cover) urls.push(jogo.cover)
  if (formato === "retrato") {
    urls.push(`${CDN}/${appid}/library_600x900.jpg`)
    urls.push(`${CDN}/${appid}/header.jpg`)
    urls.push(`${CDN}/${appid}/library_hero.jpg`)
    urls.push(`${CDN}/${appid}/capsule_467x181.jpg`)
  } else {
    urls.push(`${CDN}/${appid}/header.jpg`)
    urls.push(`${CDN}/${appid}/library_hero.jpg`)
    urls.push(`${CDN}/${appid}/capsule_616x353.jpg`)
    urls.push(`${CDN}/${appid}/library_600x900.jpg`)
  }
  return [...new Set(urls)]
}
