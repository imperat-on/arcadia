"use client"

// Perfil do amigo: header gigante (avatar, username, "amigos desde") +
// grid de conquistas recentes dele (RPC friend_achievements — só entre
// amigos). Enriquece com título/ícone quando o jogo existe localmente.
import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { corDeUsername, inicialDe, formatarData } from "../account/avatar"

interface Props {
  amigo: FriendProfile
  onVoltar: () => void
  onRemovido: () => void
}

type ConquistaEnriquecida = FriendAchievement & {
  title?: string
  icon?: string
  unlockLocal?: number
}

export function FriendProfileView({ amigo, onVoltar, onRemovido }: Props) {
  const { t } = useI18n()
  const [conquistas, setConquistas] = useState<ConquistaEnriquecida[] | null>(null)
  const [erro, setErro] = useState<string | null>(null)
  const [confirmandoRemover, setConfirmandoRemover] = useState(false)
  const [removendo, setRemovendo] = useState(false)

  useEffect(() => {
    let vivo = true
    ;(async () => {
      const r = await window.launcherAPI?.friendsAchievements(amigo.id)
      if (!vivo) return
      if (!r?.ok) {
        setErro(r?.error || t("amigos.erro_geral"))
        setConquistas([])
        return
      }
      // Enriquecimento: título/ícone do achievements.json local (se o jogo existir aqui)
      const lista = (r.achievements || []) as ConquistaEnriquecida[]
      const porApp: Record<string, Array<{ apiname?: string; title?: string; icon?: string; unlock?: number }> | null> = {}
      for (const a of lista) {
        if (!(a.appid in porApp)) {
          const g = await window.launcherAPI?.achievementsGet(a.appid)
          porApp[a.appid] = Array.isArray(g) ? g : null
        }
        const itens = porApp[a.appid]
        const item = itens?.find((i) => i.apiname === a.apiname)
        a.title = item?.title || a.apiname
        a.icon = item?.icon
        a.unlockLocal = item?.unlock
      }
      if (vivo) setConquistas(lista)
    })()
    return () => {
      vivo = false
    }
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
            <div className="text-2xl font-bold text-white">{amigo.username}</div>
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
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
            {conquistas.map((c) => (
              <div
                key={c.appid + c.apiname}
                className="group flex items-center gap-3 rounded-xl border border-white/10 bg-white/[0.03] p-3 transition-all hover:border-[#00a8ff]/40 hover:bg-white/[0.05]"
              >
                {c.icon ? (
                  <img
                    src={c.icon}
                    alt=""
                    className="h-10 w-10 rounded-lg border border-white/10 object-cover"
                  />
                ) : (
                  <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-gradient-to-br from-[#0072ce]/30 to-[#00a8ff]/10 text-lg text-[#7fd0ff]">
                    🏆
                  </div>
                )}
                <div className="min-w-0">
                  <div className="truncate text-xs font-medium text-white">{c.title || c.apiname}</div>
                  <div className="text-[10px] text-white/35">
                    {formatarData(c.unlocked_at)} · {c.appid}
                  </div>
                </div>
              </div>
            ))}
          </div>
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
