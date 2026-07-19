export interface Game {
  id: string
  title: string
  launcher: "steam" | "heroic" | "lutris" | string
  launch_cmd: string[]
  cover?: string
  hero?: string
  logo?: string
  installed?: boolean
  description?: string
  genre?: string
  year?: string | number
  rating?: number // 0–5 (estrelas)
  hidden?: boolean // ocultado pelo usuário (fica fora das listas)
  favorite?: boolean // marcado como favorito (menu de contexto)
  categories?: string[] // categorias atribuídas pelo usuário
  last_played?: number // timestamp da última vez que abriu o jogo
  exe?: string // caminho do executável (jogos custom)
  platform?: string // "windows" | "linux" (jogos custom)
  developer?: string
  publisher?: string
  metacritic?: number // 0–100
  achievements_total?: number
  achievements_done?: number // conquistas desbloqueadas pelo jogador
  playtime_minutes?: number // tempo de jogo na Steam
  players?: string // "1 jogador · co-op · multiplayer"
  size?: number // tamanho do download em MiB (jogos Epic)
}
