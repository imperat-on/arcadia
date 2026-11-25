"use client"

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

export function FriendProfileView({ amigo, games, onVoltar, onRemovido }: Props) {
  const { t } = useI18n()
  const [erro, setErro] = useState<string | null>(null)
  const [confirmandoRemover, setConfirmandoRemover] = useState(false)
  const [removendo, setRemovendo] = useState(false)
  const [perfil, setPerfil] = useState<{
    profile: ProfilePublic
    games: { appid: string; title: string; minutes?: number }[]
    friends?: FriendProfile[]
    stats?: { jogos: number; playtime_hours: number }
  } | null>()
  const aceito = amigo.status === "accepted"

  useEffect(() => {
    let vivo = true
    window.launcherAPI?.friendsProfile(amigo.id).then((r) => {
      if (!vivo) return
      if (r?.ok && r.profile) setPerfil({ profile: r.profile, games: r.games || [], friends: r.friends || [], stats: r.stats })
      else {
        setPerfil(null)
        setErro(r?.error || t("amigos.erro_geral"))
      }
    }).catch((e) => {
      // Promise rejeitada (rede/rota ausente): mostra o erro em vez de ficar
      // em "Carregando perfil..." para sempre.
      if (vivo) {
        setPerfil(null)
        setErro(String(e?.message || e))
      }
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
  if (!perfil && !erro) {
    return <div className="flex h-full items-center justify-center text-sm text-white/45">{t("profile.carregando")}</div>
  }

  if (perfil) {
    const localByApp = new Map(games.map((g) => [String(g.id).replace(/^steam:/, ""), g]))
    const jogos = perfil.games.map((item) => {
      // user_library guarda appid com prefixo ("steam:578080") — tira na hora
      // de casar com o jogo local E de montar a capa da CDN da Steam (senão a
      // URL vira .../apps/steam:578080/... → 404 → tile sem capa).
      const appidNum = String(item.appid || "").replace(/^steam:/, "")
      const local = localByApp.get(appidNum)
      return {
        ...(local || {}),
        id: local?.id || `steam:${appidNum}`,
        title: item.title || local?.title || item.appid,
        launcher: local?.launcher || "steam",
        installed: false,
        cover: local?.cover || `https://cdn.cloudflare.steamstatic.com/steam/apps/${appidNum}/library_600x900.jpg`,
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
          // readOnly já esconde o "Editar perfil"; sem onJogoClick os tiles do
          // amigo ficam desabilitados em vez de clicáveis e mudos.
        />
        {confirmandoRemover ? (
          <div className="absolute right-8 top-6 z-10 flex items-center gap-2 rounded-xl border border-[color:var(--state-danger)]/30 bg-[color:var(--surface-2)]/95 p-2 shadow-xl backdrop-blur">
            <span className="px-1 text-xs text-white/70">{t("amigos.confirmar_remover")}</span>
            <button onClick={remover} disabled={removendo} className="rounded-lg bg-[color:var(--state-danger)] px-2.5 py-1.5 text-xs font-bold text-white disabled:opacity-40">
              {removendo ? "..." : t("amigos.remover")}
            </button>
            <button onClick={() => setConfirmandoRemover(false)} className="rounded-lg border border-white/10 px-2.5 py-1.5 text-xs text-white/60">
              {t("amigos.cancelar")}
            </button>
          </div>
        ) : (
          <button
            onClick={() => setConfirmandoRemover(true)}
            className="absolute right-8 top-6 z-10 rounded-lg border border-white/10 px-3 py-2 text-xs text-white/55 hover:border-[color:var(--state-danger)]/40 hover:text-[color:var(--state-danger)]"
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

      {erro && (
        <p role="alert" className="mt-6 rounded-xl border border-[color:var(--state-danger)]/25 bg-[color:var(--state-danger)]/[0.07] px-3.5 py-2.5 text-xs text-[color:var(--state-danger)]">
          {erro}
        </p>
      )}

      {/* Ações — só entre amigos aceitos */}
      {aceito && (
        <div className="mt-auto pt-8">
          {confirmandoRemover ? (
            <div className="flex items-center gap-2 rounded-xl border border-[color:var(--state-danger)]/25 bg-[color:var(--state-danger)]/[0.06] p-3">
              <span className="text-sm text-white/70">{t("amigos.confirmar_remover")}</span>
              <button
                onClick={remover}
                disabled={removendo}
                className="ml-auto rounded-lg bg-[color:var(--state-danger)] px-3 py-1.5 text-xs font-bold text-white transition-colors hover:brightness-110 disabled:opacity-40"
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
              className="rounded-lg border border-white/10 px-4 py-2 text-xs font-medium text-white/50 transition-colors hover:border-[color:var(--state-danger)]/40 hover:text-[color:var(--state-danger)]"
            >
              {t("amigos.remover")}
            </button>
          )}
        </div>
      )}
    </div>
  )
}
