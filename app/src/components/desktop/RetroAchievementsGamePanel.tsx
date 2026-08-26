"use client"

// Painel de conquistas RetroAchievements, no mesmo estilo visual do painel de
// Steam (AchievementsPanel.tsx): ícone 64px colorido se desbloqueada / cinza
// se não, contador done/total no título. Diferenças por natureza do RA:
//  - Não tem atualização em tempo real aqui (isso já acontece no overlay do
//    próprio emulador enquanto o jogo roda); este painel só reflete o estado
//    mais recente sincronizado com o site.
//  - Sem botão de forçar desbloqueio: não temos acesso ao processo do
//    emulador para simular isso com segurança.
//  - Se a Web API Key ainda não foi configurada (diferente do login usado
//    para o emulador), mostra um CTA em vez da lista.
import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { Panel } from "./GameDetailPanels"
import { AchievementsFullScreen } from "./AchievementsFullScreen"

type ItemConquista = {
  id: number
  title: string
  description: string
  points: number
  badgeUrl?: string
  badgeLockedUrl?: string
  unlocked?: boolean
  unlockedHardcore?: boolean
}

export function RetroAchievementsGamePanel({
  title,
  systemId,
  compact = false,
}: {
  title: string
  systemId?: string
  compact?: boolean
}) {
  const { t } = useI18n()
  const [needsApiKey, setNeedsApiKey] = useState(false)
  const [apiKey, setApiKey] = useState("")
  const [savingKey, setSavingKey] = useState(false)
  const [keyError, setKeyError] = useState("")
  const [items, setItems] = useState<ItemConquista[] | null>(null)
  const [gameFound, setGameFound] = useState(true)
  const [error, setError] = useState("")

  const carregar = () => {
    if (!systemId) return
    setError("")
    setItems(null)
    window.launcherAPI?.retroachievementsGameProgress?.(title, systemId).then((r) => {
      if (!r?.ok) {
        if (r?.error === "sem_web_api_key") {
          setNeedsApiKey(true)
        } else if (r?.error === "sistema_nao_suportado") {
          setError(t("retroachievements.sistema_nao_suportado"))
        } else {
          setError(r?.error || t("retroachievements.erro_generico"))
        }
        setItems([])
        return
      }
      setNeedsApiKey(false)
      setGameFound(Boolean(r.game))
      setItems((r.achievements as ItemConquista[]) || [])
    })
  }

  useEffect(() => {
    carregar()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [title, systemId])

  const salvarApiKey = async () => {
    setSavingKey(true)
    setKeyError("")
    const r = await window.launcherAPI?.retroachievementsSetApiKey?.(apiKey.trim())
    setSavingKey(false)
    if (!r?.ok) {
      setKeyError(r?.error || t("retroachievements.erro_generico"))
      return
    }
    setApiKey("")
    carregar()
  }

  const done = items ? items.filter((x) => x.unlocked).length : 0
  const total = items ? items.length : 0
  const progress = total ? Math.round((done / total) * 100) : 0
  const [allOpen, setAllOpen] = useState(false)

  useEffect(() => {
    if (!allOpen) return
    const closeWithEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setAllOpen(false)
    }
    window.addEventListener("keydown", closeWithEscape)
    return () => window.removeEventListener("keydown", closeWithEscape)
  }, [allOpen])

  if (!systemId) return null

  if (compact) {
    return (
      <>
      <section className="h-[286px] overflow-hidden rounded-[7px] border border-white/[.1] bg-[#080a0d] p-3.5 shadow-[0_8px_20px_rgba(0,0,0,.22)]">
        <header className="mb-3 flex items-center justify-between gap-2">
          <h3 className="text-[10px] font-bold uppercase tracking-[.09em] text-white/78">Conquistas</h3>
          <div className="flex items-center gap-3">
            <span className="text-[9px] text-white/45">
              {items && !needsApiKey ? t("conquistas.contador", { done: String(done), total: String(total) }) : "Carregando…"}
            </span>
            {items && items.length > 6 && <button type="button" onClick={() => setAllOpen(true)} className="detail-achievements-all">
              Ver todas
            </button>}
          </div>
        </header>

        <div className="mb-3 h-1 overflow-hidden rounded bg-white/10">
          <span className="block h-full bg-[var(--desktop-green)]" style={{ width: `${progress}%` }} />
        </div>

        {needsApiKey ? (
          <div className="flex flex-col gap-2.5 py-2">
            <p className="text-[11px] text-white/50">{t("retroachievements.precisa_apikey")}</p>
            <div className="flex gap-2">
              <input
                value={apiKey}
                onChange={(event) => setApiKey(event.target.value)}
                type="password"
                spellCheck={false}
                placeholder={t("retroachievements.apikey_placeholder")}
                className="min-w-0 flex-1 rounded-[4px] border border-white/10 bg-white/[0.04] px-3 py-2 text-[11px] text-white outline-none placeholder:text-white/25 focus:border-[var(--desktop-green)]"
              />
              <button
                type="button"
                onClick={salvarApiKey}
                disabled={savingKey || !apiKey.trim()}
                className="detail-action-primary"
              >
                {savingKey ? t("retroachievements.conectando") : t("common.salvar")}
              </button>
            </div>
            {keyError && <p className="text-[10px] text-red-300/80">{keyError}</p>}
          </div>
        ) : items?.length ? (
          <div className="detail-achievement-grid grid max-h-[210px] gap-2 overflow-x-hidden overflow-y-auto pb-1 pr-1">
            {items.slice(0, 6).map((item) => (
              <article
                key={item.id}
                className={`detail-achievement-card flex h-[210px] min-w-0 flex-col overflow-hidden rounded-[6px] border px-2 py-2.5 text-center ${item.unlocked ? "border-[var(--desktop-green)]/45 bg-[var(--desktop-green)]/[.035]" : "border-white/[.08] bg-white/[.015]"}`}
              >
                <div className="detail-achievement-icon relative mx-auto mb-3 aspect-square w-16 shrink-0 overflow-hidden rounded-[5px] bg-white/5">
                  {item.badgeUrl || item.badgeLockedUrl ? (
                    <img
                      src={item.unlocked ? item.badgeUrl : item.badgeLockedUrl || item.badgeUrl}
                      alt=""
                      loading="lazy"
                      className={`h-full w-full object-cover ${item.unlocked ? "" : "opacity-55 sepia"}`}
                    />
                  ) : null}
                </div>
                <h4 className={`line-clamp-2 text-[9px] font-semibold leading-[1.35] ${item.unlocked ? "text-white/90" : "text-white/65"}`}>
                  {item.title}
                </h4>
                <p className="mt-2 line-clamp-3 text-[7px] leading-[1.45] text-white/35">
                  {item.description || (item.unlocked ? "Desbloqueada" : "Bloqueada")}
                </p>
              </article>
            ))}
          </div>
        ) : (
          <p className="py-8 text-center text-[10px] text-white/30">
            {items === null ? "Carregando conquistas…" : error || (!gameFound ? t("retroachievements.jogo_nao_encontrado") : t("conquistas.vazio"))}
          </p>
        )}
      </section>
      {allOpen && items?.length && <AchievementsFullScreen done={done} total={total} progress={progress} onClose={() => setAllOpen(false)}>
        <div className="detail-achievement-full-grid grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-3">
          {items.map((item) => (
            <article
              key={item.id}
              className={`detail-achievement-card flex min-h-[230px] min-w-0 flex-col overflow-hidden rounded-[6px] border px-3 py-3 text-center ${item.unlocked ? "border-[var(--desktop-green)]/45 bg-[var(--desktop-green)]/[.035]" : "border-white/[.08] bg-white/[.015]"}`}
            >
              <div className="detail-achievement-icon relative mx-auto mb-4 aspect-square w-20 shrink-0 overflow-hidden rounded-[5px] bg-white/5">
                {item.badgeUrl || item.badgeLockedUrl ? (
                  <img
                    src={item.unlocked ? item.badgeUrl : item.badgeLockedUrl || item.badgeUrl}
                    alt=""
                    loading="lazy"
                    className={`h-full w-full object-cover ${item.unlocked ? "" : "opacity-55 sepia"}`}
                  />
                ) : null}
              </div>
              <h4 className={`line-clamp-2 text-[11px] font-semibold leading-[1.35] ${item.unlocked ? "text-white/90" : "text-white/65"}`}>
                {item.title}
              </h4>
              <p className="mt-2 line-clamp-4 text-[9px] leading-[1.45] text-white/35">
                {item.description || (item.unlocked ? "Desbloqueada" : "Bloqueada")}
              </p>
            </article>
          ))}
        </div>
      </AchievementsFullScreen>}
      </>
    )
  }

  return (
    <Panel
      title={t("retroachievements.conquistas_titulo")}
      right={
        <span className="flex items-center gap-1.5">
          {items && !needsApiKey ? t("conquistas.contador", { done: String(done), total: String(total) }) : "…"}
        </span>
      }
    >
      {needsApiKey ? (
        <div className="flex flex-col gap-2.5">
          <p className="text-[12px] text-white/55">{t("retroachievements.precisa_apikey")}</p>
          <div className="flex gap-2">
            <input
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              type="password"
              spellCheck={false}
              placeholder={t("retroachievements.apikey_placeholder")}
              className="flex-1 rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2 text-[13px] text-white outline-none transition-colors placeholder:text-white/25 focus:border-[color:var(--accent)]"
            />
            <button
              type="button"
              onClick={salvarApiKey}
              disabled={savingKey || !apiKey.trim()}
              className="rounded-lg px-3.5 py-2 text-[12px] font-bold text-black transition-transform enabled:hover:scale-[1.03] disabled:opacity-50"
              style={{ background: "var(--accent)" }}
            >
              {savingKey ? t("retroachievements.conectando") : t("common.salvar")}
            </button>
          </div>
          {keyError && <p className="text-[12px] text-red-300/80">{keyError}</p>}
          <button
            type="button"
            onClick={() => void window.launcherAPI?.openExternal?.("https://retroachievements.org/controlpanel.php")}
            className="text-left text-[12px] text-white/45 underline decoration-white/20 hover:text-white/70"
          >
            {t("retroachievements.onde_pegar_apikey")}
          </button>
        </div>
      ) : (
        <>
          {error && <p className="mb-3 text-[12px] text-red-300/80">{error}</p>}

          {items === null && (
            <div className="flex flex-col gap-2">
              {[0, 1, 2, 3].map((i) => (
                <div key={i} className="flex items-center gap-3">
                  <div className="h-12 w-12 shrink-0 animate-pulse rounded-md bg-white/5" />
                  <div className="flex flex-col gap-2">
                    <div className="h-3 w-32 animate-pulse rounded bg-white/5" />
                    <div className="h-3 w-20 animate-pulse rounded bg-white/5" />
                  </div>
                </div>
              ))}
            </div>
          )}

          {items !== null && !error && !gameFound && (
            <p className="text-[12px] text-white/45">{t("retroachievements.jogo_nao_encontrado")}</p>
          )}

          {items !== null && gameFound && items.length === 0 && !error && (
            <p className="text-[12px] text-white/45">{t("conquistas.vazio")}</p>
          )}

          {items && items.length > 0 && (
            <div className="max-h-[420px] space-y-1 overflow-y-auto pr-1">
              {items.map((it) => (
                <div key={it.id} className="flex items-center gap-3 rounded-md p-2 hover:bg-white/[0.04]">
                  {it.badgeUrl || it.badgeLockedUrl ? (
                    <img
                      src={it.unlocked ? it.badgeUrl : it.badgeLockedUrl || it.badgeUrl}
                      alt=""
                      loading="lazy"
                      className="h-12 w-12 shrink-0 rounded-md object-cover"
                      style={!it.unlocked ? { filter: "grayscale(0.85) opacity(0.5)" } : undefined}
                    />
                  ) : (
                    <div className="h-12 w-12 shrink-0 rounded-md bg-white/5 ring-1 ring-white/10" />
                  )}
                  <div className="min-w-0 flex-1">
                    <div className={`truncate text-[13px] ${it.unlocked ? "font-semibold text-white" : "text-white/40"}`}>
                      {it.title}
                    </div>
                    {it.description && (
                      <div className="truncate text-[11px] text-white/35">{it.description}</div>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </>
      )}
    </Panel>
  )
}
