"use client"

import { useEffect, useRef, useState } from "react"
import type { Game } from "./types"
import type { Profile, ProfileStats } from "../../global"
import { useGamepadNav } from "./useGamepadNav"
import { useI18n } from "../../i18n/I18nContext"
import { useFriends } from "../account/FriendsContext"

interface ProfilePageProps {
  open: boolean
  profile: Profile
  games: Game[]
  onClose: () => void
  onEdit: () => void
  onJogoClick?: (g: Game) => void
  embedded?: boolean
  navActive?: boolean
}

export function ProfilePage({
  open,
  navActive = true,
  profile,
  games,
  onClose,
  onEdit,
  onJogoClick,
  embedded = false,
}: ProfilePageProps) {
  const rootRef = useRef<HTMLDivElement>(null)
  useGamepadNav(rootRef, open && navActive, onClose)
  const { t } = useI18n()
  // Amigos vêm do FriendsContext (cache + atualização automática) — a 1ª
  // pintura do perfil é instantânea, sem fetch próprio aqui.
  const { data: amigosData } = useFriends()
  const friends = amigosData?.friends ?? []

  // Estatísticas reais (jogos/playtime).
  const [stats, setStats] = useState<ProfileStats | null>(null)

  useEffect(() => {
    if (!open) return
    window.launcherAPI?.profileStats().then(setStats)
    if (embedded) return
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose()
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [open, onClose, embedded])

  if (!open) return null

  const name = profile.name || t("profile.jogador")

  // Todos os jogos da biblioteca (com capa), mais jogados primeiro — o perfil
  // mostra a biblioteca INTEIRA com as horas em cima de cada capa.
  const todosJogos = games
    .filter((g) => g.cover && !g.hidden)
    .sort((a, b) => (b.playtime_minutes || 0) - (a.playtime_minutes || 0))

  return (
    <div
      ref={rootRef}
      className={
        embedded ? "gp-scope h-full overflow-y-auto" : "gp-scope fixed inset-0 z-50 overflow-y-auto"
      }
      style={embedded ? undefined : { background: "#000000" }}
    >
      {/* Fundo do PROJETO (imagem/GIF/vídeo) — cobre a TELA INTEIRA como
          atmosfera. Com blur + escurecimento forte: o fundo fica de ambiente,
          e o banner (faixa no topo) é quem traz o destaque. O blur pode ser
          desligado (profile.background_blur === false) para o fundo nítido.
          Roda também em embedded (desktop): o fundo do projeto vale nos dois
          modos — antes só o Big Picture (não-embedded) mostrava. */}
      {profile.background && (
        <>
          {/\.(webm|mp4|m4v|mov)$/i.test(profile.background.split("?")[0]) ? (
            <video
              className={`fixed inset-0 w-full h-full object-cover pointer-events-none ${profile.background_blur === false ? "" : "blur-md scale-105"}`}
              src={profile.background}
              autoPlay
              loop
              muted
              playsInline
            />
          ) : (
            <div
              className={`fixed inset-0 pointer-events-none ${profile.background_blur === false ? "" : "blur-md scale-105"}`}
              style={{
                backgroundImage: `url(${profile.background})`,
                backgroundSize: "cover",
                backgroundPosition: "center",
                backgroundRepeat: "no-repeat",
              }}
            />
          )}
          {/* Escurecimento forte: mantém o visual OLED do Big Picture */}
          <div
            className="fixed inset-0 pointer-events-none"
            style={{
              background:
                "linear-gradient(to bottom, rgba(0,0,0,0.72) 0%, rgba(0,0,0,0.82) 40%, rgba(0,0,0,0.94) 100%)",
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

      <div className="relative min-h-full pb-12">
        {/* ── Header estilo Steam: faixa de capa + avatar sobreposto + nome ── */}
        <div className="mx-auto max-w-6xl px-6 pt-6">
          {/* Faixa de ações (voltar + editar) no topo */}
          <div className="mb-4 flex items-center justify-between">
            {embedded ? (
              <span />
            ) : (
              <button
                onClick={onClose}
                className="flex items-center gap-2 rounded-full border border-white/10 bg-black/25 px-4 py-2 text-sm text-white/75 backdrop-blur transition-colors hover:bg-white/10 hover:text-white"
                title={t("profile.fechar")}
              >
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path d="M19 12H5M12 19l-7-7 7-7" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
                {t("profile.voltar")}
              </button>
            )}
            <button
              onClick={onEdit}
              className="flex items-center gap-2 rounded-full border border-white/15 bg-black/30 px-4 py-2 text-sm font-medium text-white/90 backdrop-blur transition-colors hover:border-white/30 hover:bg-white/10"
            >
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M12 20h9M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
              {t("profile.editar_perfil")}
            </button>
          </div>

          {/* Banner do perfil (faixa larga estilo Steam) */}
          <div className="relative h-48 overflow-hidden rounded-2xl border border-white/10">
            {profile.banner ? (
              <img
                src={profile.banner}
                alt=""
                className="h-full w-full object-cover"
              />
            ) : (
              <div
                className="h-full w-full"
                style={{
                  background:
                    "linear-gradient(135deg, color-mix(in oklab, var(--accent) 30%, transparent) 0%, transparent 60%), linear-gradient(180deg, rgba(0,0,0,0.4), rgba(0,0,0,0.85))",
                }}
              />
            )}
            <div className="absolute inset-0 bg-gradient-to-t from-black/70 via-black/20 to-transparent" />
          </div>

          {/* Avatar sobreposto à capa + nome */}
          <div className="-mt-14 flex items-end gap-5 px-4">
            <div className="relative shrink-0">
              <div
                className="absolute -inset-1 rounded-2xl opacity-40 blur-lg"
                style={{ background: "var(--accent)" }}
              />
              <div
                className="relative flex h-24 w-24 items-center justify-center overflow-hidden rounded-2xl text-3xl font-bold text-white shadow-2xl"
                style={{
                  background: "linear-gradient(135deg, color-mix(in oklab, var(--accent) 55%, #003791), #003791)",
                  boxShadow: "0 12px 40px rgba(0,0,0,0.55)",
                  border: "3px solid rgba(0,0,0,0.6)",
                }}
              >
                {profile.avatar ? (
                  <img src={profile.avatar} alt="" className="h-full w-full object-cover" />
                ) : (
                  name[0].toUpperCase()
                )}
              </div>
            </div>
            <div className="min-w-0 pb-1">
              <h1
                className="game-name truncate text-3xl font-bold text-white"
                style={{ textShadow: "0 4px 24px rgba(0,0,0,0.6)" }}
              >
                {name}
              </h1>
            </div>
          </div>
        </div>

        {/* ── Corpo: biblioteca (principal) + sidebar (stats/amigos) ── */}
        <div className="mx-auto mt-8 grid max-w-6xl grid-cols-[minmax(0,1fr)_340px] gap-6 px-6">
          <main className="min-w-0">
            <section>
              <div className="mb-4 flex items-end justify-between gap-3">
                <h2 className="game-name text-xl font-bold text-white">
                  {t("profile.todos_jogos")}
                </h2>
                <span className="rounded-full border border-white/10 bg-white/[0.04] px-3 py-1 text-xs font-semibold text-white/55">
                  {t("profile.todos_jogos_contagem", { count: String(todosJogos.length) })}
                </span>
              </div>
              {todosJogos.length === 0 ? (
                <div className="flex flex-col items-center justify-center rounded-2xl border border-white/[0.06] bg-white/[0.02] py-14 px-8 text-center">
                  <p className="text-sm text-[#8a93a6]">{t("profile.nenhum_jogado")}</p>
                </div>
              ) : (
                <div className="grid grid-cols-4 gap-3">
                  {todosJogos.map((game) => (
                    <JogoTile
                      key={game.id}
                      game={game}
                      onClick={onJogoClick ? () => onJogoClick(game) : undefined}
                    />
                  ))}
                </div>
              )}
            </section>
          </main>

          <aside className="space-y-4">
            <ProfileCard title={t("profile.estatisticas")}>
              <div className="space-y-2">
                <StatRow label={t("profile.estatisticas.jogos")} value={String(games.length)} />
                <StatRow
                  label={t("profile.estatisticas.horas")}
                  value={
                    stats
                      ? t("profile.estatisticas.horas_display", { h: String(stats.playtime_hours) })
                      : t("profile.estatisticas.fallback")
                  }
                />
                <StatRow label={t("amigos.titulo")} value={String(friends.length)} />
              </div>
            </ProfileCard>

            <ProfileCard title={t("amigos.titulo")}>
              {friends.length === 0 ? (
                <p className="py-2 text-center text-xs text-[#8a93a6]">
                  {t("profile.vitrine_vazia", { max: "0" }).split(".")[0]}
                </p>
              ) : (
                <div className="grid grid-cols-4 gap-2">
                  {friends.slice(0, 8).map((friend) => (
                    <FriendAvatar key={friend.id} friend={friend} />
                  ))}
                </div>
              )}
            </ProfileCard>
          </aside>
        </div>
      </div>
    </div>
  )
}

function ProfileCard({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="glass-1 overflow-hidden rounded-2xl">
      <h2 className="border-b border-white/[0.06] bg-white/[0.03] px-5 py-3.5 text-sm font-bold text-white">
        {title}
      </h2>
      <div className="p-5">{children}</div>
    </section>
  )
}

function StatRow({ label, value }: { label: string; value: string }) {
  return (
    <div
      className="flex items-center justify-between rounded-lg px-3.5 py-2.5"
      style={{ background: "rgba(255,255,255,0.03)" }}
    >
      <span className="text-sm text-[#c8d0e0]">{label}</span>
      <span className="text-sm font-bold text-white tabular-nums">{value}</span>
    </div>
  )
}

// Formata minutos em "3h" (>= 1h) ou "45min".
function formatarHoras(min?: number): string {
  const m = Math.max(0, Math.round(min || 0))
  return m >= 60 ? `${Math.round(m / 60)}h` : `${m}min`
}

function JogoTile({ game, onClick }: { game: Game; onClick?: () => void }) {
  const [broken, setBroken] = useState(false)
  const horas = formatarHoras(game.playtime_minutes)
  const cls =
    "relative flex items-center justify-center rounded-lg bg-gradient-to-br from-[#1e2536] to-[#0a0e1a] text-3xl font-bold text-white/50 ring-1 ring-white/10"
  if (!game.cover || broken) {
    return (
      <button
        className={cls}
        style={{ aspectRatio: "2/3" }}
        title={game.title}
        onClick={onClick}
        disabled={!onClick}
      >
        {game.title[0]?.toUpperCase()}
        <HorasBadge horas={horas} />
      </button>
    )
  }
  return (
    <button
      className="group relative overflow-hidden rounded-lg ring-1 ring-white/10 transition-transform hover:scale-[1.03] hover:ring-white/25"
      style={{ aspectRatio: "2/3" }}
      title={game.title}
      onClick={onClick}
      disabled={!onClick}
    >
      <img
        src={game.cover}
        alt={game.title}
        className="h-full w-full object-cover"
        loading="lazy"
        decoding="async"
        onError={() => setBroken(true)}
      />
      {/* Horas em cima da capa */}
      <HorasBadge horas={horas} />
      {/* Nome aparece no hover */}
      <div className="pointer-events-none absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/80 to-transparent p-2 pt-6 opacity-0 transition-opacity group-hover:opacity-100">
        <span className="block truncate text-[11px] font-semibold text-white">{game.title}</span>
      </div>
    </button>
  )
}

function HorasBadge({ horas }: { horas: string }) {
  return (
    <span className="absolute left-1.5 top-1.5 flex items-center gap-1 rounded-md bg-black/65 px-2 py-1 text-[11px] font-bold text-white backdrop-blur-sm">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" className="h-3 w-3">
        <circle cx="12" cy="12" r="9" />
        <path d="M12 7v5l3 2" />
      </svg>
      {horas}
    </span>
  )
}

function FriendAvatar({ friend }: { friend: FriendProfile }) {
  const initial = (friend.display_name || friend.username)?.[0]?.toUpperCase() || "?"
  return (
    <div
      className="flex aspect-square items-center justify-center overflow-hidden rounded-lg bg-gradient-to-br from-[#0072ce] to-[#003791] text-xl font-bold text-white ring-1 ring-white/15"
      title={friend.display_name || friend.username}
    >
      {friend.avatar_url ? (
        <img src={friend.avatar_url} alt="" className="h-full w-full object-cover" />
      ) : (
        initial
      )}
    </div>
  )
}
