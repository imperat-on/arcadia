"use client"

import { useEffect, useRef, useState } from "react"
import type { Game } from "./types"
import type { Profile, ProfileStats, RecentAchievement } from "../../global"
import { BADGES, Badge, buildBadges } from "./badges"
import { useGamepadNav } from "./useGamepadNav"

// XP estilo Steam: cada conquista vale 10, rara (≤10%) +15, jogo 100% vale 100
// e cada hora jogada vale 2.
function calcularXP(s: ProfileStats): number {
  return s.ach_done * 10 + s.ach_raras * 15 + s.jogos_100 * 100 + s.playtime_hours * 2
}

// Curva da Steam: a cada 10 níveis o custo por nível sobe (100, 200, 300…).
function nivelDoXP(xp: number): { nivel: number; noNivel: number; custo: number } {
  let nivel = 0
  let resto = xp
  while (true) {
    const custo = (Math.floor(nivel / 10) + 1) * 100
    if (resto < custo) return { nivel, noNivel: resto, custo }
    resto -= custo
    nivel++
  }
}

interface ProfilePageProps {
  open: boolean
  navActive: boolean
  profile: Profile
  games: Game[]
  onClose: () => void
  onEdit: () => void
}

export function ProfilePage({
  open,
  navActive,
  profile,
  games,
  onClose,
  onEdit,
}: ProfilePageProps) {
  const rootRef = useRef<HTMLDivElement>(null)
  useGamepadNav(rootRef, open && navActive, onClose)

  // Estatísticas reais (conquistas/playtime) para nível e insígnias.
  const [stats, setStats] = useState<ProfileStats | null>(null)
  const [feed, setFeed] = useState<RecentAchievement[]>([])
  useEffect(() => {
    if (!open) return
    window.launcherAPI?.profileStats().then(setStats)
    window.launcherAPI?.achievementsRecent().then((r) => {
      if (Array.isArray(r)) setFeed(r.slice(0, 8))
    })
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose()
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [open, onClose])

  if (!open) return null

  const isOwner = profile.owner !== false
  const name = profile.name || "Jogador"
  const xp = stats ? calcularXP(stats) : games.length * 25
  const { nivel, noNivel, custo } = nivelDoXP(xp)
  const badgesDin = stats ? buildBadges(stats) : []
  const desbloqueadas = badgesDin.filter((b) => b.unlocked).length
  const badgesDono = BADGES.filter((b) => (b.rare ? isOwner : true))
  // Vitrine: usa os destaques escolhidos; senão, os primeiros com capa.
  const showcase =
    profile.showcase && profile.showcase.length
      ? profile.showcase
          .map((id) => games.find((g) => g.id === id))
          .filter((g): g is Game => Boolean(g && g.cover))
      : games.filter((g) => g.cover).slice(0, 8)
  const recent = games.slice(0, 3)
  const launchers = Array.from(new Set(games.map((g) => g.launcher)))

  return (
    <div ref={rootRef} className="gp-scope fixed inset-0 z-50 overflow-y-auto" style={{ background: "#000000" }}>
      {/* Plano de fundo do perfil (imagem/GIF/vídeo) — cobre a TELA INTEIRA */}
      {profile.background && (
        <>
          {/\.(webm|mp4|m4v|mov)$/i.test(profile.background.split("?")[0]) ? (
            <video
              className="fixed inset-0 w-full h-full object-cover pointer-events-none"
              src={profile.background}
              autoPlay
              loop
              muted
              playsInline
            />
          ) : (
            <div
              className="fixed inset-0 pointer-events-none"
              style={{
                backgroundImage: `url(${profile.background})`,
                backgroundSize: "cover",
                backgroundPosition: "center",
                backgroundRepeat: "no-repeat",
              }}
            />
          )}
          {/* Escurecimento p/ legibilidade (mais forte embaixo) */}
          <div
            className="fixed inset-0 pointer-events-none"
            style={{
              background:
                "linear-gradient(to bottom, rgba(0,0,0,0.45) 0%, rgba(0,0,0,0.7) 40%, rgba(0,0,0,0.92) 100%)",
            }}
          />
        </>
      )}

      {/* Brilho de topo estilo perfil (só quando não há fundo próprio) */}
      {!profile.background && (
        <div
          className="absolute top-0 inset-x-0 h-96 pointer-events-none"
          style={{
            background:
              "radial-gradient(ellipse 60% 100% at 50% 0%, rgba(75,40,120,0.35), transparent 70%)",
          }}
        />
      )}

      {/* Fechar */}
      <button
        onClick={onClose}
        className="fixed top-6 right-8 z-10 w-10 h-10 rounded-full flex items-center justify-center text-[#8a93a6] hover:bg-white/10 hover:text-white transition-colors"
        title="Fechar (Esc)"
      >
        <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
          <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z" />
        </svg>
      </button>

      <div className="relative max-w-6xl mx-auto px-10 pt-16 pb-16">
        {/* Cabeçalho */}
        <div className="flex items-start gap-6 mb-10">
          <div
            className="w-28 h-28 rounded-2xl overflow-hidden shrink-0 flex items-center justify-center text-4xl font-bold text-white"
            style={{
              background: "linear-gradient(135deg, #0072ce, #003791)",
              border: "2px solid rgba(255,255,255,0.15)",
              boxShadow: isOwner ? "0 0 30px rgba(255,196,0,0.25)" : "none",
            }}
          >
            {profile.avatar ? (
              <img src={profile.avatar} alt="" className="w-full h-full object-cover" />
            ) : (
              name[0].toUpperCase()
            )}
          </div>

          <div className="flex-1 pt-2">
            <div className="flex items-center gap-3">
              <h1 className="text-3xl font-bold text-white">{name}</h1>
              {isOwner && (
                <span
                  className="inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-semibold"
                  style={{ background: "rgba(255,196,0,0.12)", color: "#ffc400", border: "1px solid rgba(255,196,0,0.3)" }}
                >
                  👑 Dono
                </span>
              )}
            </div>
            <p className="text-sm text-[#8a93a6] mt-1">
              Membro do Arcadia · {launchers.length} plataforma(s) conectada(s)
            </p>
            <button
              onClick={onEdit}
              className="mt-4 px-4 py-2 rounded-lg text-sm font-medium text-[#c8d0e0] transition-colors hover:bg-white/5"
              style={{ border: "1px solid rgba(255,255,255,0.14)" }}
            >
              Editar perfil
            </button>
          </div>

          {/* Nível (estilo Steam: círculo + barra de XP) */}
          <div className="flex w-56 shrink-0 flex-col items-center pt-1">
            <span className="mb-1 text-xs uppercase tracking-wider text-[#8a93a6]">Nível</span>
            <div
              className="flex h-16 w-16 items-center justify-center rounded-full text-2xl font-bold text-white"
              style={{
                background: "radial-gradient(circle at 30% 30%, rgba(155,107,255,0.35), rgba(155,107,255,0.08))",
                border: "2px solid rgba(155,107,255,0.5)",
              }}
            >
              {nivel}
            </div>
            <div className="mt-3 h-1.5 w-full overflow-hidden rounded-full bg-white/10">
              <div
                className="h-full rounded-full"
                style={{ width: `${Math.round((noNivel / custo) * 100)}%`, background: "linear-gradient(90deg, #a06bff, var(--accent))" }}
              />
            </div>
            <span className="mt-1.5 text-[11px] tabular-nums text-[#6b7280]">
              {noNivel} / {custo} XP · total {xp} XP
            </span>
          </div>
        </div>

        <div className="grid grid-cols-3 gap-8">
          {/* Coluna principal */}
          <div className="col-span-2 space-y-8">
            {/* Feed de atividade: conquistas recentes */}
            {feed.length > 0 && (
              <section>
                <h2 className="mb-3 pb-2 text-sm font-semibold uppercase tracking-wider text-[#8a93a6]"
                  style={{ borderBottom: "1px solid rgba(255,255,255,0.08)" }}>
                  Conquistas recentes
                </h2>
                <div className="space-y-2">
                  {feed.map((a) => {
                    const pct = typeof a.percent === "number" ? a.percent : parseFloat(String(a.percent)) || 0
                    const rara = pct > 0 && pct <= 10
                    return (
                      <div key={`${a.appid}-${a.title}`} className="flex items-center gap-4 rounded-xl p-3"
                        style={{ background: "rgba(255,255,255,0.03)", border: "1px solid rgba(255,255,255,0.06)" }}>
                        <img src={a.icon} alt="" className="h-10 w-10 shrink-0 rounded-lg ring-1 ring-white/10" loading="lazy" />
                        <div className="min-w-0 flex-1">
                          <div className="truncate text-sm font-medium text-white">{a.title}</div>
                          <div className="truncate text-xs text-[#8a93a6]">{a.game}</div>
                        </div>
                        <div className="shrink-0 text-right">
                          <div className="text-[11px] tabular-nums text-white/50">
                            {new Date(a.unlock * 1000).toLocaleDateString("pt-BR")}
                          </div>
                          {pct > 0 && (
                            <div className={`text-[10px] ${rara ? "text-[#ffd23f]" : "text-white/30"}`}>
                              {pct.toFixed(1).replace(".", ",")}%{rara ? " · rara" : ""}
                            </div>
                          )}
                        </div>
                      </div>
                    )
                  })}
                </div>
              </section>
            )}

            {/* Vitrine de jogos */}
            <section>
              <h2 className="text-sm font-semibold text-[#8a93a6] uppercase tracking-wider mb-3 pb-2"
                style={{ borderBottom: "1px solid rgba(255,255,255,0.08)" }}>
                Vitrine de jogos
              </h2>
              <div className="grid grid-cols-4 gap-3">
                {showcase.map((g) => (
                  <div key={g.id} className="rounded-lg overflow-hidden" style={{ aspectRatio: "2/3" }}>
                    <img src={g.cover} alt={g.title} className="w-full h-full object-cover" />
                  </div>
                ))}
              </div>
            </section>

            {/* Atividade recente */}
            <section>
              <h2 className="text-sm font-semibold text-[#8a93a6] uppercase tracking-wider mb-3 pb-2"
                style={{ borderBottom: "1px solid rgba(255,255,255,0.08)" }}>
                Atividade recente
              </h2>
              <div className="space-y-3">
                {recent.map((g) => (
                  <div key={g.id} className="flex items-center gap-4 p-3 rounded-xl"
                    style={{ background: "rgba(255,255,255,0.03)", border: "1px solid rgba(255,255,255,0.06)" }}>
                    {g.cover && (
                      <img src={g.cover} alt="" className="w-12 h-16 rounded object-cover shrink-0" />
                    )}
                    <div className="min-w-0">
                      <div className="text-white font-medium truncate">{g.title}</div>
                      <div className="text-xs text-[#8a93a6]">
                        {g.genre || "Jogo"} {g.year ? `· ${g.year}` : ""}
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            </section>
          </div>

          {/* Coluna lateral */}
          <div className="space-y-8">
            {/* Insígnias (regras reais, estilo Steam) */}
            <section>
              <h2 className="text-sm font-semibold text-[#8a93a6] uppercase tracking-wider mb-3">
                Insígnias <span className="opacity-60">{desbloqueadas}/{badgesDin.length}</span>
              </h2>
              <div className="flex flex-wrap gap-3">
                {badgesDin.map((b) => (
                  <div key={b.def.id} title={`${b.def.name} — ${b.def.desc} (${b.progress})`} className="flex flex-col items-center gap-1">
                    <Badge badge={b.def} size={52} locked={!b.unlocked} />
                    <span className={`text-[10px] tabular-nums ${b.unlocked ? "text-white/60" : "text-white/30"}`}>
                      {b.progress}
                    </span>
                  </div>
                ))}
                {badgesDono.map((b) => (
                  <div key={b.id} title={`${b.name} — ${b.desc}`}>
                    <Badge badge={b} size={52} />
                  </div>
                ))}
              </div>
            </section>

            {/* Estatísticas */}
            <section>
              <h2 className="text-sm font-semibold text-[#8a93a6] uppercase tracking-wider mb-3">
                Estatísticas
              </h2>
              <div className="space-y-2">
                <StatRow label="Jogos" value={String(games.length)} />
                <StatRow label="Conquistas" value={stats ? `${stats.ach_done} / ${stats.ach_total}` : "—"} />
                <StatRow label="Raras" value={stats ? String(stats.ach_raras) : "—"} />
                <StatRow label="100% completos" value={stats ? String(stats.jogos_100) : "—"} />
                <StatRow label="Horas jogadas" value={stats ? `${stats.playtime_hours} h` : "—"} />
                <StatRow label="Plataformas" value={String(launchers.length)} />
              </div>
            </section>

            {/* Plataformas */}
            <section>
              <h2 className="text-sm font-semibold text-[#8a93a6] uppercase tracking-wider mb-3">
                Plataformas
              </h2>
              <div className="flex flex-wrap gap-2">
                {launchers.map((l) => (
                  <span key={l} className="px-3 py-1 rounded-full text-xs font-medium capitalize"
                    style={{ background: "rgba(255,255,255,0.06)", color: "#c8d0e0" }}>
                    {l}
                  </span>
                ))}
              </div>
            </section>
          </div>
        </div>
      </div>
    </div>
  )
}

function StatRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between px-4 py-2.5 rounded-lg"
      style={{ background: "rgba(255,255,255,0.03)" }}>
      <span className="text-sm text-[#c8d0e0]">{label}</span>
      <span className="text-sm font-bold text-white">{value}</span>
    </div>
  )
}
