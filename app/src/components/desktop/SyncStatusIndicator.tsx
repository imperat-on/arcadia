"use client"

// Indicador de sincronização de conquistas (canto inferior direito).
// Só aparece logado. Mostra: sincronizando (spin), ok, fila pendente ou erro,
// com botão "Sincronizar agora".
import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { useAccount } from "../account/AccountContext"

export function SyncStatusIndicator() {
  const { t } = useI18n()
  const { status } = useAccount()
  const [st, setSt] = useState<SyncState | null>(null)
  const [sincronizando, setSincronizando] = useState(false)

  useEffect(() => {
    if (status !== "logado") {
      setSt(null)
      return
    }
    let vivo = true
    window.launcherAPI?.syncState().then((s) => vivo && setSt(s))
    const off = window.launcherAPI?.onSyncState((s) => vivo && setSt(s))
    return () => {
      vivo = false
      off?.()
    }
  }, [status])

  if (status !== "logado" || !st) return null

  const sincronizar = async () => {
    setSincronizando(true)
    try {
      const result = await window.launcherAPI?.syncNow()
      if (result && !result.ok) {
        setSt((current) =>
          current ? { ...current, lastError: result.error || "falha_no_sync" } : current,
        )
      }
    } catch (error) {
      setSt((current) =>
        current
          ? { ...current, lastError: String(error?.message || error || "falha_no_sync") }
          : current,
      )
    } finally {
      setSincronizando(false)
    }
  }

  const fullSync = async () => {
    setSincronizando(true)
    try {
      const result = await window.launcherAPI?.syncFull()
      if (result && result.ok) {
        setSt((current) => current ? { ...current, lastError: null } : current)
      } else if (result && !result.ok) {
        setSt((current) =>
          current ? { ...current, lastError: result.error || "falha_no_sync" } : current,
        )
      }
    } catch (error) {
      setSt((current) =>
        current
          ? { ...current, lastError: String(error?.message || error || "falha_no_sync") }
          : current,
      )
    } finally {
      setSincronizando(false)
    }
  }

  const temErro = !!st.lastError
  const temFila = st.queueLen > 0
  const cor = temErro ? "#ff6b6b" : temFila ? "#f5a623" : "#4ade80"
  const texto = temErro
    ? t("sync.erro")
    : sincronizando
      ? t("sync.sincronizando")
      : temFila
        ? t("sync.fila", { n: String(st.queueLen) })
        : t("sync.ok")

  return (
    <button
      onClick={sincronizar}
      onContextMenu={(e) => { e.preventDefault(); fullSync() }}
      title={st.lastError || "Clique direito = full sync"}
      className="desktop-sync-status fixed bottom-[56px] right-4 z-[80] flex items-center gap-2 rounded-full border border-white/10 bg-[#16161c]/90 px-3 py-1.5 text-xs font-medium text-white/80 shadow-lg backdrop-blur transition-colors hover:bg-[#1d1d24]"
    >
      <span
        className={`h-2 w-2 rounded-full ${sincronizando ? "animate-pulse" : ""}`}
        style={{ background: cor }}
      />
      {sincronizando ? t("sync.sincronizando") : texto}
      {temFila && !sincronizando && <span className="text-white/40">↻</span>}
    </button>
  )
}
