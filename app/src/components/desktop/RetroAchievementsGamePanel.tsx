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

export function RetroAchievementsGamePanel({ title, systemId }: { title: string; systemId?: string }) {
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

  if (!systemId) return null

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
