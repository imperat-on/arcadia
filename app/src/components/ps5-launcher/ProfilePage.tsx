"use client"

import { useEffect, useRef, useState } from "react"
import type { Game } from "./types"
import type { Profile, ProfileStats, RecentAchievement } from "../../global"
import { Badge, buildBadges } from "./badges"
import { useGamepadNav } from "./useGamepadNav"
import { userLocale } from "../../i18n/locale"
import { useI18n } from "../../i18n/I18nContext"

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
  embedded?: boolean
}

export function ProfilePage({
  open,
  navActive,
  profile,
  games,
  onClose,
  onEdit,
  embedded = false,
}: ProfilePageProps) {
  const rootRef = useRef<HTMLDivElement>(null)
  useGamepadNav(rootRef, open && navActive, onClose)
  const { t } = useI18n()

  // Estatísticas reais (conquistas/playtime) para nível e insígnias.
  const [stats, setStats] = useState<ProfileStats | null>(null)
  const [feed, setFeed] = useState<RecentAchievement[]>([])
  useEffect(() => {
    if (!open) return
    window.launcherAPI?.profileStats().then(setStats)
    window.launcherAPI?.achievementsRecent().then((r) => {
      if (Array.isArray(r)) setFeed(r.slice(0, 8))
    })
    if (embedded) return
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose()
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [open, onClose, embedded])

  if (!open) return null

  const isOwner = profile.owner !== false
  const name = profile.name || t("profile.jogador")
  const xp = stats ? calcularXP(stats) : games.length * 25
  const { nivel, noNivel, custo } = nivelDoXP(xp)
  // Só entram no perfil as insígnias realmente conquistadas. Antes exibíamos
  // todas (as bloqueadas em cinza) MAIS uma lista fixa que era dada de graça
  // pelo simples fato de ser o dono — o perfil ficava cheio de insígnias que
  // ninguém tinha ganhado. Insígnia só aparece quando a condição é cumprida.
  const badgesDin = stats ? buildBadges(stats, t) : []
  const total = badgesDin.length
  const conquistadas = badgesDin.filter((b) => b.unlocked)
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
    <div ref={rootRef} className={embedded ? "gp-scope h-full overflow-y-auto" : "gp-scope fixed inset-0 z-50 overflow-y-auto"} style={embedded ? undefined : { background: "#000000" }}>
      {/* Plano de fundo do perfil (imagem/GIF/vídeo) — cobre a TELA INTEIRA */}
      {!embedded && profile.background && (
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
      {!embedded && !profile.background && (
        <div
          className="absolute top-0 inset-x-0 h-96 pointer-events-none"
          style={{
            background:
              "radial-gradient(ellipse 60% 100% at 50% 0%, rgba(75,40,120,0.35), transparent 70%)",
          }}
        />
      )}

      {!embedded && (
        <button
          onClick={onClose}
          className="fixed top-6 right-8 z-10 w-10 h-10 rounded-full flex items-center justify-center text-[#8a93a6] hover:bg-white/10 hover:text-white transition-colors"
          title={t("profile.fechar")}
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
            <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z" />
          </svg>
        </button>
      )}

      <div className="relative min-h-full pb-10">
        <section className="relative h-72 border-b border-white/[0.08] bg-[#17171a]">
          {profile.background && <img src={profile.background} alt="" className="absolute inset-0 h-full w-full object-cover opacity-70" />}
          <div className="absolute inset-0 bg-gradient-to-b from-black/25 via-black/20 to-black/65" />
          <button
            onClick={onEdit}
            className="absolute right-8 top-6 rounded-lg border border-white/20 bg-black/35 px-4 py-2 text-sm font-medium text-white/90 backdrop-blur transition-colors hover:bg-white/10"
          >
            {t("profile.editar_perfil")}
          </button>
          <div className="absolute bottom-10 left-8 flex items-center gap-5">
            <div className="flex h-24 w-24 items-center justify-center overflow-hidden rounded-xl bg-gradient-to-br from-[#0072ce] to-[#003791] text-4xl font-bold text-white shadow-2xl ring-1 ring-white/15">
              {profile.avatar ? <img src={profile.avatar} alt="" className="h-full w-full object-cover" /> : name[0].toUpperCase()}
            </div>
            <div>
              <div className="flex items-center gap-3">
                <h1 className="text-2xl font-bold text-white drop-shadow">{name}</h1>
                {isOwner && <span className="rounded-md bg-white/10 px-2 py-1 text-xs text-white/60">{t("profile.dono")}</span>}
              </div>
              <p className="mt-1 text-sm text-white/55">{t("profile.plataformas_conectadas", { count: String(launchers.length) })}</p>
            </div>
          </div>
        </section>

        <div className="grid grid-cols-[minmax(0,1fr)_400px] gap-6 px-8 py-7">
          <main className="min-w-0">
            <div className="mb-5 flex items-center gap-3">
              <span className="rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2 text-xs text-white/55">{t("library.todas")}</span>
              <span className="rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2 text-xs text-white/55">{t("profile.estatisticas.horas_display", { h: String(stats?.playtime_hours || 0) })}</span>
            </div>
            <h2 className="mb-4 text-2xl font-bold text-white">
              {t("sidebar.biblioteca")} <span className="ml-2 rounded-md bg-white/10 px-2 py-1 text-xs text-white/70">{games.length}</span>
            </h2>
            <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-4">
              {games.filter((g) => g.cover).map((g) => (
                <div key={g.id} className="group relative overflow-hidden rounded-xl border border-white/[0.06] bg-white/[0.03] shadow-lg shadow-black/25">
                  <div className="aspect-[2/3] overflow-hidden">
                    <img src={g.cover} alt={g.title} className="h-full w-full object-cover transition-transform duration-500 group-hover:scale-105" loading="lazy" />
                  </div>
                  <div className="absolute left-2 top-2 rounded-md bg-black/55 px-2 py-1 text-[11px] text-white/85 backdrop-blur">
                    {t("profile.estatisticas.horas_display", { h: String(Math.max(1, Math.round((g.playtime_minutes || 0) / 60))) })}
                  </div>
                </div>
              ))}
            </div>
          </main>

          <aside className="space-y-4">
            <ProfileCard title={t("profile.estatisticas")}>
              <StatRow label={t("profile.estatisticas.jogos")} value={String(games.length)} />
              <StatRow label={t("profile.estatisticas.conquistas")} value={stats ? `${stats.ach_done} / ${stats.ach_total}` : t("profile.estatisticas.fallback")} />
              <StatRow label={t("profile.estatisticas.raras")} value={stats ? String(stats.ach_raras) : t("profile.estatisticas.fallback")} />
              <StatRow label={t("profile.estatisticas.completos")} value={stats ? String(stats.jogos_100) : t("profile.estatisticas.fallback")} />
              <StatRow label={t("profile.estatisticas.horas")} value={stats ? t("profile.estatisticas.horas_display", { h: String(stats.playtime_hours) }) : t("profile.estatisticas.fallback")} />
            </ProfileCard>

            <ProfileCard title={t("profile.insignias", { count: String(conquistadas.length), total: String(total) })}>
              {conquistadas.length === 0 ? (
                <p className="text-xs text-[#8a93a6]">{t("profile.sem_insignias")}</p>
              ) : (
                <div className="space-y-2">
                  {conquistadas.slice(0, 4).map((b) => (
                    <div key={b.def.id} className="flex items-center gap-3 rounded-xl bg-white/[0.04] p-3">
                      <Badge badge={b.def} size={36} />
                      <div className="min-w-0">
                        <div className="truncate text-sm font-semibold text-white">{b.def.name}</div>
                        <div className="truncate text-xs text-white/45">{b.def.desc}</div>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </ProfileCard>

            <ProfileCard title={t("profile.atividade_recente")}>
              <div className="space-y-3">
                {(feed.length ? feed : recent.map((g) => ({ appid: g.id, title: g.title, game: g.title, icon: g.icon || g.cover || "", unlock: 0, percent: 0 }))).slice(0, 6).map((a) => (
                  <div key={`${a.appid}-${a.title}`} className="flex items-center gap-3">
                    {a.icon && <img src={a.icon} alt="" className="h-9 w-9 rounded-lg object-cover ring-1 ring-white/10" loading="lazy" />}
                    <div className="min-w-0">
                      <div className="truncate text-sm font-semibold text-white">{a.game}</div>
                      <div className="truncate text-xs text-white/45">{a.title}</div>
                    </div>
                  </div>
                ))}
              </div>
            </ProfileCard>
          </aside>
        </div>
      </div>
    </div>
  )
}

function ProfileCard({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="overflow-hidden rounded-2xl border border-white/[0.08] bg-white/[0.035] shadow-xl shadow-black/25">
      <h2 className="border-b border-white/[0.05] bg-white/[0.025] px-6 py-4 text-sm font-bold text-white">{title}</h2>
      <div className="space-y-3 p-6">{children}</div>
    </section>
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
