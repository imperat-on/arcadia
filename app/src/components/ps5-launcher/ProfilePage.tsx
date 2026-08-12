"use client"

import { useEffect, useRef, useState } from "react"
import type { Game } from "./types"
import type { Profile, ProfileStats } from "../../global"
import { useGamepadNav } from "./useGamepadNav"
import { useI18n } from "../../i18n/I18nContext"
import { useAccountOptional, OWNER_USERNAME } from "../account/AccountContext"
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
  const conta = useAccountOptional()
  const ehDono = conta?.perfil?.username === OWNER_USERNAME
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

  const isOwner = ehDono
  const name = profile.name || t("profile.jogador")
  
  // Todos os jogos da biblioteca (com capa), mais jogados primeiro — o perfil
  // mostra a biblioteca INTEIRA com as horas em cima de cada capa.
  const todosJogos = games
    .filter((g) => g.cover && !g.hidden)
    .sort((a, b) => (b.playtime_minutes || 0) - (a.playtime_minutes || 0))
  const launchers = Array.from(new Set(games.filter((g) => !g.hidden).map((g) => g.launcher)))

  return (
    <div
      ref={rootRef}
      className={
        embedded ? "gp-scope h-full overflow-y-auto" : "gp-scope fixed inset-0 z-50 overflow-y-auto"
      }
      style={embedded ? undefined : { background: "#000000" }}
    >
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
          {profile.background && (
            <img
              src={profile.background}
              alt=""
              className="absolute inset-0 h-full w-full object-cover opacity-70"
            />
          )}
          <div className="absolute inset-0 bg-gradient-to-b from-black/25 via-black/20 to-black/65" />
          <button
            onClick={onEdit}
            className="absolute right-20 top-6 rounded-lg border border-white/20 bg-black/35 px-4 py-2 text-sm font-medium text-white/90 backdrop-blur transition-colors hover:bg-white/10"
          >
            {t("profile.editar_perfil")}
          </button>
          <div className="absolute bottom-10 left-8 flex items-center gap-5">
            <div className="flex h-24 w-24 items-center justify-center overflow-hidden rounded-xl bg-gradient-to-br from-[#0072ce] to-[#003791] text-4xl font-bold text-white shadow-2xl ring-1 ring-white/15">
              {profile.avatar ? (
                <img src={profile.avatar} alt="" className="h-full w-full object-cover" />
              ) : (
                name[0].toUpperCase()
              )}
            </div>
            <div>
              <div className="flex items-center gap-3">
                <h1 className="text-2xl font-bold text-white drop-shadow">{name}</h1>
                {isOwner && (
                  <span className="rounded-md bg-white/10 px-2 py-1 text-xs text-white/60">
                    {t("profile.dono")}
                  </span>
                )}
              </div>
              <p className="mt-1 text-sm text-white/55">
                {t("profile.plataformas_conectadas", { count: String(launchers.length) })}
              </p>
            </div>
          </div>
        </section>

        <div className="grid grid-cols-[minmax(0,1fr)_400px] gap-6 px-8 py-7">
          <main className="min-w-0">
            {/* Biblioteca inteira com horas em cima de cada capa */}
            <section className="mb-8">
              <div className="mb-4 flex items-center justify-between gap-3">
                <h2 className="text-2xl font-bold text-white">{t("profile.todos_jogos")}</h2>
                <span className="text-sm text-white/55">
                  {t("profile.todos_jogos_contagem", { count: String(todosJogos.length) })}
                </span>
              </div>
              {todosJogos.length === 0 ? (
                <div className="flex flex-col items-center justify-center rounded-xl border border-white/[0.06] bg-white/[0.03] py-12 px-8 text-center">
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
              <StatRow label={t("profile.estatisticas.jogos")} value={String(games.length)} />
              <StatRow
                label={t("profile.estatisticas.horas")}
                value={
                  stats
                    ? t("profile.estatisticas.horas_display", { h: String(stats.playtime_hours) })
                    : t("profile.estatisticas.fallback")
                }
              />
            </ProfileCard>

            <ProfileCard title={t("amigos.titulo")}>
              <div className="text-sm text-[#c8d0e0] mb-3">
                {t("profile.amigos_contagem", { count: String(friends.length) })}
              </div>
              {friends.length === 0 ? (
                <p className="text-xs text-[#8a93a6] text-center py-2">
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

function GameCapsule({ src, title }: { src: string; title: string }) {
  const [broken, setBroken] = useState(false)
  if (!src || broken) {
    return (
      <div className="flex h-[90px] w-[240px] flex-none items-center justify-center rounded-lg bg-gradient-to-br from-[#1e2536] to-[#0a0e1a] text-2xl font-bold text-white/50 ring-1 ring-white/10">
        {title[0]?.toUpperCase()}
      </div>
    )
  }
  return (
    <img
      src={src}
      alt={title}
      className="h-[90px] w-[240px] flex-none rounded-lg object-cover ring-1 ring-white/10"
      loading="lazy"
      onError={() => setBroken(true)}
    />
  )
}

function ProfileCard({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="overflow-hidden rounded-2xl border border-white/[0.08] bg-white/[0.035] shadow-xl shadow-black/25">
      <h2 className="border-b border-white/[0.05] bg-white/[0.025] px-6 py-4 text-sm font-bold text-white">
        {title}
      </h2>
      <div className="space-y-3 p-6">{children}</div>
    </section>
  )
}

function StatRow({ label, value }: { label: string; value: string }) {
  return (
    <div
      className="flex items-center justify-between px-4 py-2.5 rounded-lg"
      style={{ background: "rgba(255,255,255,0.03)" }}
    >
      <span className="text-sm text-[#c8d0e0]">{label}</span>
      <span className="text-sm font-bold text-white">{value}</span>
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
