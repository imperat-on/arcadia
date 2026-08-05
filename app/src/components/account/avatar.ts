"use client"

// Helper de avatar: cor estável derivada do username (hash → HSL) e inicial.
// Sem imagem de avatar ainda no servidor — letra colorida estilo Discord.

export function corDeUsername(nome: string): string {
  let h = 0
  for (const c of String(nome || "")) h = (h * 31 + c.charCodeAt(0)) % 360
  return `hsl(${h} 55% 24%)`
}

export function inicialDe(nome: string): string {
  return (String(nome || "")[0] || "?").toUpperCase()
}

/** Data ISO → "DD/MM/AAAA" (pt-BR) sem lib. */
export function formatarData(iso?: string | null): string {
  if (!iso) return "—"
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return "—"
  return d.toLocaleDateString("pt-BR", { day: "2-digit", month: "2-digit", year: "numeric" })
}

/** Timestamp epoch (s ou ms) → "DD/MM/AAAA". */
export function formatarTimestampEpoch(ts?: number | null): string {
  if (!ts) return "—"
  const n = Number(ts)
  const s = n > 1e12 ? Math.floor(n / 1000) : Math.floor(n)
  return formatarData(new Date(s * 1000).toISOString())
}
