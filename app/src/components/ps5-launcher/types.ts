import type { ArcadiaGame } from "../../../../contracts"

/** Contrato compartilhado da biblioteca; campos específicos continuam abertos. */
export type Game = ArcadiaGame

/** Um jogo como a loja o recebe do backend — bem mais magro que `Game`. */
export interface JogoLinha {
  appid: string
  title: string
  cover?: string
  /** Capa retrato oficial (library_capsule), quando a Steam publica uma. */
  capa?: string
  /** Arte larga (library_hero_2x, 3840x1240) para o herói da vitrine. */
  heroi?: string
  manifest?: boolean
  fontes?: string[]
  /** Só as seções da vitrine oficial trazem preço por item; o resto fica vazio. */
  preco?: string
  precoOriginal?: string
  desconto?: number
}

/** Dados do appdetails, buscados só para o jogo em foco. */
export interface FichaJogo {
  descricao?: string
  generos?: string[]
  preco?: string
  precoOriginal?: string
  desconto?: number
  metacritic?: number
  lancamento?: string
  fundo?: string
}
