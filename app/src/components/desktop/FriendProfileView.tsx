"use client"

// Perfil do amigo: header gigante (avatar, username, "amigos desde") +
// grid de conquistas recentes dele (RPC friend_achievements — só entre
// amigos). Enriquece com título/ícone quando o jogo existe localmente.
import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { corDeUsername, inicialDe, formatarData } from "../account/avatar"
import { ProfilePage } from "../ps5-launcher/ProfilePage"
import type { Game } from "../ps5-launcher/types"
import type { Profile } from "../../global"

type ProfilePublic = Profile & {
  username?: string | null
  avatar_url?: string | null
  display_name?: string | null
  background_url?: string | null
  banner_url?: string | null
}

interface Props {
  amigo: FriendProfile
  games: Game[]
  onVoltar: () => void
  onRemovido: () => void
}

type ConquistaEnriquecida = FriendAchievement & {
  title?: string
  icon?: string
  percent?: number | null
  unlockLocal?: number
}

export function FriendProfileView({ amigo, games, onVoltar, onRemovido }: Props) {
  const { t } = useI18n()
  const [conquistas, setConquistas] = useState<ConquistaEnriquecida[] | null>(null)
  const [erro, setErro] = useState<string | null>(null)
  const [confirmandoRemover, setConfirmandoRemover] = useState(false)
  const [removendo, setRemovendo] = useState(false)
  const [perfil, setPerfil] = useState<{
    profile: ProfilePublic
    games: { appid: string; title: string; minutes?: number }[]
    friends?: FriendProfile[]
    stats?: { jogos: number; playtime_hours: number }
  } | null>(null)

  useEffect(() => {
    let vivo = true
    ;(async () => {
      let r
      try {
        r = await window.launcherAPI?.friendsAchievements(amigo.id)
      } catch (e) {
        if (!vivo) return
        setErro(String(e?.message || e))
        setConquistas([])
        return
      }
      if (!vivo) return
      if (!r?.ok) {
        setErro(r?.error || t("amigos.erro_geral"))
        setConquistas([])
        return
      }
      // 1a pintura: os dados crus do servidor ja trazem title/icon sincronizados
      // do amigo — a grade aparece imediatamente, sem esperar enriquecimento.
      const lista = (r.achievements || []) as ConquistaEnriquecida[]
      if (vivo) setConquistas(lista)

      // Enriquecimento em PARALELO: titulo/icone melhores do achievements.json
      // local (se o jogo existir aqui). Cada achievementsGet pode disparar um
      // scrape da Steam p/ jogo sem items locais (ate 15s) — em serie isso
      // segurava a tela inteira; em paralelo a grid ja esta visivel e os dados
      // chegam quando prontos.
      const appids = [...new Set(lista.map((a) => a.appid))]
      const porApp = await Promise.all(
        appids.map(async (appid) => {
          try {
            const g = await window.launcherAPI?.achievementsGet(appid)
            return [appid, Array.isArray(g) ? g : null] as const
          } catch {
            return [appid, null] as const
          }
        }),
      )
      if (!vivo) return
      const mapa = new Map(porApp)
      const enriquecidas = lista.map((a) => {
        const item = mapa.get(a.appid)?.find((i) => i.apiname === a.apiname)
        return {
          ...a,
          title: item?.title || a.title || a.apiname,
          icon: item?.icon || a.icon,
          unlockLocal: item?.unlock ?? a.unlockLocal,
        }
      })
      setConquistas(enriquecidas)
    })()
    return () => {
      vivo = false
    }
  }, [amigo.id, t])

  useEffect(() => {
    let vivo = true
    window.launcherAPI?.friendsProfile(amigo.id).then((r) => {
      if (vivo && r?.ok && r.profile) setPerfil({ profile: r.profile, games: r.games || [], friends: r.friends || [], stats: r.stats })
      if (vivo && !r?.ok) setErro(r?.error || t("amigos.erro_geral"))
    }).catch((e) => {
      // Promise rejeitada (rede/rota ausente): mostra o erro em vez de ficar
      // em "Carregando perfil..." para sempre.
      if (vivo) setErro(String(e?.message || e))
    })
    return () => { vivo = false }
  }, [amigo.id, t])

  const remover = async () => {
    setRemovendo(true)
    const r = await window.launcherAPI?.friendsRemove(amigo.id)
    setRemovendo(false)
    if (r?.ok) onRemovido()
    else setErro(r?.error || t("amigos.erro_geral"))
  }

  const cor = corDeUsername(amigo.username)
  const corClara = `hsl(${cor.replace(/hsl\((\d+).*/, "$1")} 90% 60%)`
  const aceito = amigo.status === "accepted"

  if (!perfil && !erro) {
    return <div className="flex h-full items-center justify-center text-sm text-white/45">Carregando perfil...</div>
  }

  if (perfil) {
    const localByApp = new Map(games.map((g) => [String(g.id).replace(/^steam:/, ""), g]))
    const jogos = perfil.games.map((item) => {
      const local = localByApp.get(String(item.appid))
      return {
        ...(local || {}),
        id: local?.id || `steam:${item.appid}`,
        title: item.title || local?.title || item.appid,
        launcher: local?.launcher || "steam",
        installed: false,
        cover: local?.cover || `https://cdn.cloudflare.steamstatic.com/steam/apps/${item.appid}/library_600x900.jpg`,
        playtime_minutes: Number(item.minutes || 0),
      } as Game
    })
    const p = perfil.profile
    return (
      <div className="relative h-full">
        <button
          onClick={onVoltar}
          className="absolute left-8 top-6 z-10 flex items-center gap-2 rounded-full border border-white/10 bg-black/35 px-3.5 py-2 text-xs font-medium text-white/75 backdrop-blur transition-colors hover:bg-white/10 hover:text-white"
          title={t("amigos.voltar")}
        >
          <span aria-hidden="true">←</span>
          {t("amigos.voltar")}
        </button>
        <ProfilePage
          open
          embedded
          readOnly
          profile={{
            name: String(p.display_name || p.username || amigo.username),
            avatar: String(p.avatar_url || amigo.avatar_url || ""),
            background: String(p.background_url || ""),
            banner: String(p.banner_url || ""),
            summary: String(p.summary || ""),
            country: String(p.country || ""),
            city: String(p.city || ""),
          }}
          games={jogos}
          statsOverride={perfil.stats}
          friendsOverride={perfil.friends}
          onClose={onVoltar}
          onEdit={() => {}}
          onJogoClick={() => {}}
        />
        {confirmandoRemover ? (
          <div className="absolute right-8 top-6 z-10 flex items-center gap-2 rounded-xl border border-[#ff6b81]/30 bg-[#16161c]/95 p-2 shadow-xl backdrop-blur">
            <span className="px-1 text-xs text-white/70">{t("amigos.confirmar_remover")}</span>
            <button onClick={remover} disabled={removendo} className="rounded-lg bg-[#ff6b81] px-2.5 py-1.5 text-xs font-bold text-white disabled:opacity-40">
              {removendo ? "..." : t("amigos.remover")}
            </button>
            <button onClick={() => setConfirmandoRemover(false)} className="rounded-lg border border-white/10 px-2.5 py-1.5 text-xs text-white/60">
              {t("amigos.cancelar")}
            </button>
          </div>
        ) : (
          <button
            onClick={() => setConfirmandoRemover(true)}
            className="absolute right-8 top-6 z-10 rounded-lg border border-white/10 px-3 py-2 text-xs text-white/55 hover:border-[#ff6b81]/40 hover:text-[#ff6b81]"
          >
            {t("amigos.remover")}
          </button>
        )}
      </div>
    )
  }

  return (
    <div className="arc-fade-up flex h-full flex-col overflow-y-auto">
      {/* Header */}
      <div className="relative overflow-hidden rounded-3xl border border-white/10 bg-gradient-to-b from-white/[0.06] to-transparent p-8">
        <div
          className="pointer-events-none absolute -right-20 -top-20 h-64 w-64 rounded-full opacity-30 blur-[80px]"
          style={{ background: corClara }}
        />
        <button
          onClick={onVoltar}
          className="mb-6 flex items-center gap-1.5 text-sm text-white/50 transition-colors hover:text-white"
        >
          ← {t("amigos.voltar")}
        </button>
        <div className="flex items-center gap-5">
          {amigo.avatar_url ? (
            <img
              src={amigo.avatar_url}
              alt=""
              className="h-20 w-20 rounded-2xl border border-white/10 object-cover"
            />
          ) : (
            <div
              className="flex h-20 w-20 items-center justify-center rounded-2xl border border-white/10 text-3xl font-bold text-white shadow-lg"
              style={{ background: cor }}
            >
              {inicialDe(amigo.username)}
            </div>
          )}
          <div>
            <div className="text-2xl font-bold text-white">
              {amigo.display_name || amigo.username}
              {amigo.display_name && amigo.display_name !== amigo.username && (
                <span className="ml-2 text-sm font-normal text-white/30">@{amigo.username}</span>
              )}
            </div>
            <div className="mt-1 text-sm text-white/40">
              {aceito
                ? `${t("amigos.desde")} ${formatarData(amigo.since)}`
                : t("amigos.pendente_perfil")}
            </div>
          </div>
        </div>
      </div>

      {/* Conquistas — só entre amigos aceitos (privacidade) */}
      {aceito && (
        <div className="mt-6">
          <div className="mb-3 flex items-center justify-between">
            <h3 className="text-sm font-semibold uppercase tracking-wider text-white/50">
              {t("amigos.conquistas_recentes")}
              {conquistas ? ` (${conquistas.length})` : ""}
            </h3>
          </div>

        {erro && (
          <div className="rounded-xl border border-[#ff6b81]/25 bg-[#ff6b81]/[0.07] px-3.5 py-2.5 text-xs text-[#ff6b81]">
            {erro}
          </div>
        )}

        {!conquistas && !erro && (
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
            {Array.from({ length: 6 }).map((_, i) => (
              <div
                key={i}
                className="h-20 animate-pulse rounded-xl border border-white/5 bg-white/[0.03]"
              />
            ))}
          </div>
        )}

        {conquistas && conquistas.length === 0 && !erro && (
          <div className="rounded-2xl border border-dashed border-white/10 py-10 text-center text-sm text-white/30">
            {t("amigos.sem_conquistas")}
          </div>
        )}

        {conquistas && conquistas.length > 0 && (
          <>
            {/* Resumo estilo "meu perfil" */}
            <div className="mb-4 grid grid-cols-3 gap-3">
              <div className="rounded-xl border border-white/10 bg-white/[0.03] p-3 text-center">
                <div className="text-xl font-bold text-[#7fd0ff]">{conquistas.length}</div>
                <div className="text-[10px] uppercase tracking-wider text-white/35">
                  {t("amigos.total_conquistas")}
                </div>
              </div>
              <div className="rounded-xl border border-white/10 bg-white/[0.03] p-3 text-center">
                <div className="text-xl font-bold text-[#4adf9a]">
                  {conquistas.filter((c) => c.percent != null && c.percent >= 100).length > 0
                    ? `${Math.round(
                        (conquistas.filter((c) => c.percent != null && c.percent >= 100).length / conquistas.length) * 100,
                      )}%`
                    : "—"}
                </div>
                <div className="text-[10px] uppercase tracking-wider text-white/35">
                  {t("amigos.platinadas")}
                </div>
              </div>
              <div className="rounded-xl border border-white/10 bg-white/[0.03] p-3 text-center">
                <div className="text-xl font-bold text-[#f5a623]">
                  {new Set(conquistas.map((c) => c.appid)).size}
                </div>
                <div className="text-[10px] uppercase tracking-wider text-white/35">
                  {t("amigos.jogos")}
                </div>
              </div>
            </div>

            <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
              {conquistas.map((c) => (
                <div
                  key={c.appid + c.apiname}
                  className="group flex flex-col gap-2 rounded-xl border border-white/10 bg-white/[0.03] p-3 transition-all hover:border-[#00a8ff]/40 hover:bg-white/[0.05]"
                >
                  <div className="flex items-start gap-2.5">
                    {c.icon ? (
                      <img
                        src={c.icon}
                        alt=""
                        className="h-11 w-11 shrink-0 rounded-lg border border-white/10 object-cover"
                      />
                    ) : (
                      <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-lg bg-gradient-to-br from-[#0072ce]/30 to-[#00a8ff]/10 text-lg text-[#7fd0ff]">
                        🏆
                      </div>
                    )}
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-xs font-medium text-white">
                        {c.title || c.apiname}
                      </div>
                      <div className="mt-0.5 truncate text-[10px] text-white/35">{c.appid}</div>
                    </div>
                    {c.percent != null && (
                      <span className="shrink-0 rounded-full bg-[#4adf9a]/15 px-1.5 py-0.5 text-[10px] font-bold text-[#4adf9a]">
                        {Math.round(c.percent)}%
                      </span>
                    )}
                  </div>
                  <div className="flex items-center justify-between border-t border-white/5 pt-2">
                    <span className="text-[10px] text-white/35">
                      {formatarData(c.unlocked_at)}
                    </span>
                    <span className="text-[10px] font-semibold text-[#4adf9a]">✓</span>
                  </div>
                </div>
              ))}
            </div>
          </>
        )}
        </div>
      )}

      {/* Ações — só entre amigos aceitos */}
      {aceito && (
        <div className="mt-auto pt-8">
          {confirmandoRemover ? (
            <div className="flex items-center gap-2 rounded-xl border border-[#ff6b81]/25 bg-[#ff6b81]/[0.06] p-3">
              <span className="text-sm text-white/70">{t("amigos.confirmar_remover")}</span>
              <button
                onClick={remover}
                disabled={removendo}
                className="ml-auto rounded-lg bg-[#ff6b81] px-3 py-1.5 text-xs font-bold text-white transition-colors hover:brightness-110 disabled:opacity-40"
              >
                {removendo ? "…" : t("amigos.remover")}
              </button>
              <button
                onClick={() => setConfirmandoRemover(false)}
                className="rounded-lg border border-white/10 px-3 py-1.5 text-xs text-white/60 hover:text-white"
              >
                {t("amigos.cancelar")}
              </button>
            </div>
          ) : (
            <button
              onClick={() => setConfirmandoRemover(true)}
              className="rounded-lg border border-white/10 px-4 py-2 text-xs font-medium text-white/50 transition-colors hover:border-[#ff6b81]/40 hover:text-[#ff6b81]"
            >
              {t("amigos.remover")}
            </button>
          )}
        </div>
      )}
    </div>
  )
}
