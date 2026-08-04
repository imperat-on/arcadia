"use client"

// Seção de fixes (crack/bypass/online) na página do jogo. Grid 2x2 estilo
// luatools-moon: Crack/Bypass, Online Fix, Apply, Un-Fix. Estado por appid,
// progresso de download e detecção de fix já aplicado.

import { useEffect, useMemo, useRef, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"

type FixCheck = {
  ok: boolean
  generic?: { available: boolean; status: number; url?: string }
  online?: { available: boolean; status: number; url?: string }
  crack?: {
    available: boolean
    status: number
    url?: string
    badge?: string
    requiresAuth?: boolean
  }
  authConfigured?: boolean
}

type ApplyState =
  | { kind: "idle" }
  | { kind: "downloading"; bytesRead: number; totalBytes: number }
  | { kind: "extracting" }
  | { kind: "failed"; error: string; errorCode?: string }
  | { kind: "done" }

const ICON_CRACK = (
  <svg
    width="20"
    height="20"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.8"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <path d="M12 22c5.523 0 10-4.477 10-10S17.523 2 12 2 2 6.477 2 12s4.477 10 10 10z" />
    <path d="M8.5 9l-1 1 1 1M15.5 9l1 1-1 1M12 8v8" />
  </svg>
)
const ICON_ONLINE = (
  <svg
    width="20"
    height="20"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.8"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <path d="M12 2v20M2 12h20M4.93 4.93l14.14 14.14M4.93 19.07 19.07 4.93" />
  </svg>
)
const ICON_UNFIX = (
  <svg
    width="20"
    height="20"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.8"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <path d="M12 22c5.523 0 10-4.477 10-10S17.523 2 12 2 2 6.477 2 12s4.477 10 10 10z" />
    <path d="M9 12l2 2 4-4" />
  </svg>
)

export function FixesPanel({ appid, installPath }: { appid: string; installPath: string }) {
  const { t } = useI18n()
  const [check, setCheck] = useState<FixCheck | null>(null)
  const [applied, setApplied] = useState(false)
  const [state, setState] = useState<ApplyState>({ kind: "idle" })
  const [authKey, setAuthKey] = useState("")
  const [showAuth, setShowAuth] = useState(false)
  const pollRef = useRef<number | null>(null)

  const recarregar = () => {
    if (!appid) return
    window.launcherAPI?.fixesCheck(appid).then((r) => setCheck(r || null))
    if (installPath) {
      window.launcherAPI
        ?.fixesInstalled({ appid, installPath })
        .then((r) => setApplied(Boolean(r?.installed)))
    }
  }

  useEffect(() => {
    recarregar()
    return () => {
      if (pollRef.current) window.clearInterval(pollRef.current)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [appid, installPath])

  const iniciarPoll = () => {
    if (pollRef.current) window.clearInterval(pollRef.current)
    pollRef.current = window.setInterval(async () => {
      const s = await window.launcherAPI?.fixesStatus(appid)
      if (!s) return
      if (s.status === "downloading") {
        setState({
          kind: "downloading",
          bytesRead: s.bytesRead || 0,
          totalBytes: s.totalBytes || 0,
        })
      } else if (s.status === "extracting") {
        setState({ kind: "extracting" })
      } else if (s.status === "failed") {
        setState({
          kind: "failed",
          error: s.error || t("fixes.erro_generico"),
          errorCode: s.errorCode,
        })
        if (pollRef.current) {
          window.clearInterval(pollRef.current)
          pollRef.current = null
        }
      } else if (s.status === "done") {
        setState({ kind: "done" })
        if (pollRef.current) {
          window.clearInterval(pollRef.current)
          pollRef.current = null
        }
        recarregar()
      }
    }, 400)
  }

  const aplicar = async (tipo: "generic" | "online" | "crack", url?: string) => {
    if (!url || !installPath) return
    if (tipo === "crack" && !check?.authConfigured) {
      setShowAuth(true)
      return
    }
    setState({ kind: "downloading", bytesRead: 0, totalBytes: 0 })
    const r = await window.launcherAPI?.fixesApply({ appid, url, type: tipo, installPath })
    if (!r?.ok) {
      setState({
        kind: "failed",
        error: r?.error || t("fixes.erro_generico"),
        errorCode: r?.errorCode,
      })
      if (r?.errorCode === "authentication") setShowAuth(true)
      return
    }
    iniciarPoll()
  }

  const cancelar = async () => {
    await window.launcherAPI?.fixesCancel(appid)
    if (pollRef.current) {
      window.clearInterval(pollRef.current)
      pollRef.current = null
    }
    setState({ kind: "idle" })
  }

  const desfazer = async () => {
    await window.launcherAPI?.fixesUnfix({ appid, installPath })
    recarregar()
  }

  const salvarAuth = async () => {
    await window.launcherAPI?.fixesSetRyuuAuth(authKey)
    setShowAuth(false)
    setAuthKey("")
    recarregar()
  }

  const busy = state.kind === "downloading" || state.kind === "extracting"

  if (!check) {
    return (
      <div
        className="rounded-2xl p-5"
        style={{
          background: "rgba(255,255,255,0.025)",
          boxShadow: "inset 0 0 0 1px rgba(255,255,255,0.05)",
        }}
      >
        <p className="text-[12px] text-white/40">{t("fixes.carregando")}</p>
      </div>
    )
  }

  return (
    <div
      className="rounded-2xl p-5"
      style={{
        background: "rgba(255,255,255,0.025)",
        boxShadow: "inset 0 0 0 1px rgba(255,255,255,0.06)",
      }}
    >
      <div className="mb-4 flex items-center justify-between gap-3">
        <h3 className="flex items-center gap-2.5 text-[11px] font-bold uppercase tracking-[0.14em] text-white/70">
          <span className="h-3 w-[2px] rounded-full" style={{ background: "var(--accent)" }} />
          {t("fixes.titulo")}
        </h3>
        {applied && (
          <span className="rounded-full bg-[#4adf9a]/15 px-2.5 py-1 text-[10.5px] font-semibold text-[#4adf9a]">
            {t("fixes.aplicado")}
          </span>
        )}
      </div>

      {/* Estado ativo (download/extract/falha) */}
      {busy && (
        <div className="mb-4 rounded-xl border border-white/[0.06] bg-white/[0.02] p-3">
          <div className="mb-2 flex items-center justify-between text-[12px] text-white/70">
            <span>{state.kind === "downloading" ? t("fixes.baixando") : t("fixes.extraindo")}</span>
            <button
              onClick={cancelar}
              className="text-[11px] font-semibold text-[#ff6b81] hover:underline"
            >
              {t("common.cancelar")}
            </button>
          </div>
          {state.kind === "downloading" && state.totalBytes > 0 && (
            <>
              <div className="h-1.5 overflow-hidden rounded-full bg-white/[0.08]">
                <div
                  className="h-full rounded-full"
                  style={{
                    width: `${Math.round((state.bytesRead / state.totalBytes) * 100)}%`,
                    background: "var(--accent)",
                  }}
                />
              </div>
              <p className="mt-1.5 text-[10.5px] text-white/40">
                {formatBytes(state.bytesRead)} / {formatBytes(state.totalBytes)}
              </p>
            </>
          )}
          {state.kind === "extracting" && (
            <div className="h-1.5 animate-pulse rounded-full bg-white/[0.08]" />
          )}
        </div>
      )}
      {state.kind === "failed" && (
        <div className="mb-4 rounded-xl border border-[#ff6b81]/30 bg-[#ff6b81]/[0.06] p-3">
          <p className="text-[12px] text-[#ffb3c0]">{state.error}</p>
          {state.errorCode === "authentication" && (
            <button
              onClick={() => setShowAuth(true)}
              className="mt-1 text-[11px] font-semibold text-[#ff6b81] hover:underline"
            >
              {t("fixes.adicionar_chave")}
            </button>
          )}
        </div>
      )}
      {state.kind === "done" && (
        <div className="mb-4 rounded-xl border border-[#4adf9a]/30 bg-[#4adf9a]/[0.06] p-3">
          <p className="text-[12px] text-[#4adf9a]">{t("fixes.sucesso")}</p>
        </div>
      )}

      {/* Grid 2x2 */}
      <div className="grid grid-cols-2 gap-2.5">
        <FixButton
          icon={ICON_CRACK}
          label={t("fixes.crack")}
          sub={
            check.crack?.available
              ? check.crack.badge
                ? `(${check.crack.badge})`
                : t("fixes.disponivel")
              : t("fixes.indisponivel")
          }
          available={Boolean(check.crack?.available)}
          disabled={busy || !check.crack?.available}
          onClick={() => aplicar("crack", check.crack?.url)}
        />
        <FixButton
          icon={ICON_ONLINE}
          label={t("fixes.online")}
          sub={check.online?.available ? t("fixes.disponivel") : t("fixes.indisponivel")}
          available={Boolean(check.online?.available)}
          disabled={busy || !check.online?.available}
          onClick={() => aplicar("online", check.online?.url)}
        />
        <FixButton
          icon={<span className="text-[18px] font-bold">+</span>}
          label={t("fixes.generic")}
          sub={check.generic?.available ? t("fixes.disponivel") : t("fixes.indisponivel")}
          available={Boolean(check.generic?.available)}
          disabled={busy || !check.generic?.available}
          onClick={() => aplicar("generic", check.generic?.url)}
        />
        <FixButton
          icon={ICON_UNFIX}
          label={t("fixes.unfix")}
          sub={applied ? t("fixes.reverter") : t("fixes.nada_aplicado")}
          available={applied}
          disabled={busy || !applied}
          onClick={desfazer}
        />
      </div>

      {!installPath && (
        <p className="mt-3 text-[11px] text-[#ffb86b]">{t("fixes.sem_install_path")}</p>
      )}

      {/* Modal auth ryuu */}
      {showAuth && (
        <div
          className="fixed inset-0 z-[70] flex items-center justify-center bg-black/70 backdrop-blur-sm"
          onClick={() => setShowAuth(false)}
        >
          <div
            className="w-[420px] max-w-[92vw] rounded-2xl border border-white/[0.08] bg-[#0d0d10] p-5"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="mb-2 text-[15px] font-semibold text-white">{t("fixes.auth_titulo")}</h3>
            <p className="mb-4 text-[12.5px] leading-relaxed text-white/55">
              {t("fixes.auth_desc")}
            </p>
            <input
              value={authKey}
              onChange={(e) => setAuthKey(e.target.value)}
              placeholder={t("fixes.auth_placeholder")}
              spellCheck={false}
              className="mb-4 w-full rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2 text-[13px] text-white outline-none focus:border-[color:var(--accent)]"
            />
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setShowAuth(false)}
                className="rounded-full px-4 py-2 text-[12.5px] text-white/60 hover:text-white"
              >
                {t("common.cancelar")}
              </button>
              <button
                onClick={salvarAuth}
                disabled={!authKey.trim()}
                className="rounded-full bg-white px-4 py-2 text-[12.5px] font-semibold text-black disabled:opacity-40"
              >
                {t("common.salvar")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

function FixButton({
  icon,
  label,
  sub,
  available,
  disabled,
  onClick,
}: {
  icon: React.ReactNode
  label: string
  sub: string
  available: boolean
  disabled: boolean
  onClick: () => void
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={`group flex flex-col items-center justify-center gap-1.5 rounded-xl border px-3 py-3.5 text-center transition-all ${
        available && !disabled
          ? "border-[color:var(--accent)]/30 bg-[color:var(--accent)]/[0.06] text-white hover:border-[color:var(--accent)]/50 hover:bg-[color:var(--accent)]/[0.1]"
          : "border-white/[0.06] bg-white/[0.02] text-white/40"
      } disabled:cursor-not-allowed disabled:opacity-50`}
    >
      <span className={available && !disabled ? "text-[color:var(--accent)]" : "text-white/30"}>
        {icon}
      </span>
      <span className="text-[12px] font-semibold leading-tight">{label}</span>
      <span className="text-[10px] leading-tight opacity-70">{sub}</span>
    </button>
  )
}

function formatBytes(n: number) {
  if (!n) return "0 B"
  const u = ["B", "KB", "MB", "GB"]
  const i = Math.min(u.length - 1, Math.floor(Math.log2(n) / 10))
  return `${(n / Math.pow(1024, i)).toFixed(i === 0 ? 0 : 1)} ${u[i]}`
}
