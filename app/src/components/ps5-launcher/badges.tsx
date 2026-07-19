"use client"

import { useId } from "react"

// Insígnias estilo Steam: medalhões circulares com anel metálico, brilho e gloss.

export interface BadgeDef {
  id: string
  name: string
  desc: string
  rare: boolean
  tint: string
  icon: string // path d de um viewBox 24x24
}

const ICONS = {
  crown: "M5 16L3 6l5.5 4L12 4l3.5 6L19 6l-2 10H5zm-.5 3h15a1 1 0 010 2h-15a1 1 0 010-2z",
  diamond: "M6 2h12l3.5 6L12 22 2.5 8 6 2zm1.2 2L5 7.6 12 18l7-10.4L16.8 4H7.2z",
  flame: "M12 2c.5 3-2.5 4.2-2.5 7.2A2.5 2.5 0 0012 12c0-1.5 1-2 1-2 .3 1.7 2.5 2.4 2.5 4.8A4.5 4.5 0 116.5 12c1.2-1.6 2-2.6 2-4.8 1.2.6 1.7 1.8 1.7 2.8 1-1 1.8-2.4 1.8-4 0-2-.5-3-1.7-4z",
  books: "M4 3h6a2 2 0 012 2v14a3 3 0 00-3-3H4V3zm16 0h-6a2 2 0 00-2 2v14a3 3 0 013-3h5V3z",
  rocket: "M12 2c3.5 2 5 5.5 5 9 0 1.7-.4 3.2-1 4.5L14 18h-4l-2-2.5C7.4 14.2 7 12.7 7 11c0-3.5 1.5-7 5-9zm0 6a2 2 0 100 4 2 2 0 000-4zM8 19l-2 3 3.5-1.2M16 19l2 3-3.5-1.2",
  compass: "M12 2a10 10 0 100 20 10 10 0 000-20zm3.5 6.5l-2 5.5-5.5 2 2-5.5 5.5-2zM12 11a1 1 0 100 2 1 1 0 000-2z",
  trophy: "M8 21h8m-4-4v4M7 4h10v4a5 5 0 01-10 0V4zM7 6H4.5A1.5 1.5 0 003 7.5v0A2.5 2.5 0 005.5 10H7M17 6h2.5A1.5 1.5 0 0121 7.5v0a2.5 2.5 0 01-2.5 2.5H17",
  clock: "M12 21a9 9 0 100-18 9 9 0 000 18zm0-13v5l3 2",
}

export const BADGES: BadgeDef[] = [
  { id: "founder", name: "Fundador", desc: "Um dos criadores do Arcadia", rare: true, tint: "#ffd23f", icon: ICONS.crown },
  { id: "owner", name: "Dono Supremo", desc: "Detentor do sistema", rare: true, tint: "#a06bff", icon: ICONS.diamond },
  { id: "legend", name: "Lenda", desc: "Presença lendária", rare: true, tint: "#ff5d5d", icon: ICONS.flame },
  { id: "collector", name: "Colecionador", desc: "Grande biblioteca", rare: false, tint: "#4aa3ff", icon: ICONS.books },
  { id: "pioneer", name: "Pioneiro", desc: "Entrou cedo", rare: false, tint: "#4adf9a", icon: ICONS.rocket },
  { id: "explorer", name: "Explorador", desc: "Vários launchers", rare: false, tint: "#c8d0e0", icon: ICONS.compass },
]

/** Insígnia calculada a partir das estatísticas reais do jogador. */
export interface EarnedBadge {
  def: BadgeDef
  unlocked: boolean
  progress: string // ex.: "37/50" (texto de condição)
}

// Regras estilo Steam: desbloqueia ao atingir a condição; bloqueadas ficam cinza.
export function buildBadges(s: {
  jogos: number
  playtime_hours: number
  ach_done: number
  ach_raras: number
  jogos_100: number
}): EarnedBadge[] {
  const regra = (
    id: string, name: string, desc: string, tint: string, icon: string,
    atual: number, alvo: number, rare = false,
  ): EarnedBadge => ({
    def: { id, name, desc, rare, tint, icon },
    unlocked: atual >= alvo,
    progress: `${Math.min(atual, alvo)}/${alvo}`,
  })
  return [
    regra("first", "Primeira Conquista", "Desbloqueie 1 conquista", "#4adf9a", ICONS.trophy, s.ach_done, 1),
    regra("hunter50", "Caçador", "Desbloqueie 50 conquistas", "#4aa3ff", ICONS.trophy, s.ach_done, 50),
    regra("master200", "Mestre das Conquistas", "Desbloqueie 200 conquistas", "#a06bff", ICONS.crown, s.ach_done, 200, true),
    regra("rare1", "Caçador de Raras", "Desbloqueie 1 conquista rara (≤10%)", "#ff5d5d", ICONS.flame, s.ach_raras, 1),
    regra("rare10", "Lenda Rara", "Desbloqueie 10 conquistas raras", "#ffd23f", ICONS.flame, s.ach_raras, 10, true),
    regra("perfect1", "Completista", "Complete 100% de 1 jogo", "#ffd23f", ICONS.diamond, s.jogos_100, 1),
    regra("perfect5", "Perfeccionista", "Complete 100% de 5 jogos", "#ffd23f", ICONS.diamond, s.jogos_100, 5, true),
    regra("hours100", "Maratonista", "Acumule 100 horas de jogo", "#22d3ee", ICONS.clock, s.playtime_hours, 100),
    regra("hours500", "Viciado", "Acumule 500 horas de jogo", "#ff9f1c", ICONS.clock, s.playtime_hours, 500, true),
    regra("collector50", "Colecionador", "Tenha 50 jogos na biblioteca", "#4aa3ff", ICONS.books, s.jogos, 50),
    regra("pioneer", "Pioneiro", "Membro do Arcadia", "#4adf9a", ICONS.rocket, 1, 1),
  ]
}

export function Badge({
  badge,
  size = 56,
  locked = false,
}: {
  badge: BadgeDef
  size?: number
  locked?: boolean
}) {
  const uid = useId().replace(/:/g, "")
  const t = locked ? "#5a5a62" : badge.tint
  const ring = `ring-${uid}`
  const disc = `disc-${uid}`
  const gloss = `gloss-${uid}`

  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 100 100"
      style={{ filter: !locked && badge.rare ? `drop-shadow(0 0 6px ${t}88)` : "none" }}
    >
      <defs>
        {/* Anel metálico (foil) */}
        <linearGradient id={ring} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#ffffff" stopOpacity="0.95" />
          <stop offset="22%" stopColor={t} />
          <stop offset="50%" stopColor="#1c1c24" />
          <stop offset="72%" stopColor={t} />
          <stop offset="100%" stopColor="#ffffff" stopOpacity="0.7" />
        </linearGradient>
        {/* Disco interno */}
        <radialGradient id={disc} cx="38%" cy="30%" r="80%">
          <stop offset="0%" stopColor={t} stopOpacity="0.6" />
          <stop offset="62%" stopColor="#0d0d12" />
          <stop offset="100%" stopColor="#050507" />
        </radialGradient>
        {/* Gloss (reflexo) */}
        <radialGradient id={gloss} cx="34%" cy="20%" r="55%">
          <stop offset="0%" stopColor="#ffffff" stopOpacity="0.5" />
          <stop offset="45%" stopColor="#ffffff" stopOpacity="0" />
        </radialGradient>
      </defs>

      {/* brilho externo (raras) */}
      {!locked && badge.rare && <circle cx="50" cy="50" r="49" fill={t} opacity="0.14" />}

      {/* anel metálico */}
      <circle cx="50" cy="50" r="46" fill={`url(#${ring})`} />
      <circle cx="50" cy="50" r="41" fill="#08080b" />
      {/* disco */}
      <circle cx="50" cy="50" r="39" fill={`url(#${disc})`} />

      {/* emblema */}
      <g
        transform="translate(50 49) scale(1.55) translate(-12 -12)"
        fill="none"
        stroke={locked ? "#7a7a82" : "#ffffff"}
        strokeWidth={1.5}
        strokeLinejoin="round"
        strokeLinecap="round"
      >
        <path d={badge.icon} />
      </g>

      {/* gloss por cima */}
      <circle cx="50" cy="50" r="39" fill={`url(#${gloss})`} />

      {/* selo de raridade */}
      {!locked && badge.rare && (
        <g transform="translate(73 73)">
          <circle r="12" fill={t} stroke="#0a0a0e" strokeWidth="2" />
          <path
            d="M0 -6l1.8 3.9 4.2.4-3.2 2.8.95 4.1L0 7l-3.7 2.2.95-4.1-3.2-2.8 4.2-.4z"
            fill="#1a1200"
          />
        </g>
      )}
    </svg>
  )
}
