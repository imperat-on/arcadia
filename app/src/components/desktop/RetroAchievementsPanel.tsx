"use client"

import { useEffect, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"

/**
 * Login RetroAchievements (usuário + senha, trocado por um token de sessão
 * guardado em config.json — a senha nunca fica salva). Uma vez conectado, o
 * token é injetado automaticamente na config do emulador certo (PCSX2,
 * DuckStation, PPSSPP) antes de lançar um jogo retro com ROM configurada.
 */
const APPLY_TARGETS = [
  { id: "pcsx2", label: "PCSX2 (PS2)" },
  { id: "duckstation", label: "DuckStation (PS1)" },
  { id: "ppsspp", label: "PPSSPP (PSP)" },
] as const

export function RetroAchievementsPanel() {
  const { t } = useI18n()
  const [connected, setConnected] = useState(false)
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const [applying, setApplying] = useState("")
  const [applyMsg, setApplyMsg] = useState("")

  const carregar = () => {
    window.launcherAPI?.retroachievementsStatus?.().then((r) => {
      setConnected(Boolean(r?.connected))
      setUsername(r?.username || "")
    })
  }

  useEffect(() => {
    carregar()
  }, [])

  const conectar = async () => {
    setBusy(true)
    setError("")
    const r = await window.launcherAPI?.retroachievementsLogin?.(username.trim(), password)
    setBusy(false)
    if (!r?.ok) {
      setError(r?.error || t("retroachievements.erro_login"))
      return
    }
    setPassword("")
    carregar()
  }

  const desconectar = async () => {
    setBusy(true)
    await window.launcherAPI?.retroachievementsLogout?.()
    setBusy(false)
    carregar()
  }

  const aplicar = async (emulatorId: string) => {
    setApplying(emulatorId)
    setApplyMsg("")
    const r = await window.launcherAPI?.retroachievementsApplyToEmulator?.(emulatorId)
    setApplying("")
    setApplyMsg(
      r?.ok
        ? t("retroachievements.aplicado_sucesso")
        : r?.error || t("retroachievements.aplicado_falha"),
    )
  }

  return (
    <section className="mb-8 rounded-2xl border border-white/[0.08] bg-white/[0.02] p-4">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-sm font-medium text-white">{t("retroachievements.titulo")}</h3>
        {connected && (
          <span className="text-[11px] font-medium" style={{ color: "#4adf9a" }}>
            {t("retroachievements.conectado")}
          </span>
        )}
      </div>
      <p className="mb-3 text-xs text-white/45">{t("retroachievements.desc")}</p>

      {connected ? (
        <div>
          <div className="flex items-center gap-3">
            <span className="text-[13px] text-white/80">{username}</span>
            <button
              type="button"
              onClick={desconectar}
              disabled={busy}
              className="rounded-lg border border-white/10 px-3.5 py-1.5 text-[11px] font-medium text-white/65 transition-colors hover:border-white/25 hover:text-white disabled:opacity-40"
            >
              {t("retroachievements.desconectar")}
            </button>
          </div>
          <p className="mb-2 mt-4 text-[12px] text-white/45">{t("retroachievements.aplicar_desc")}</p>
          <div className="flex flex-wrap gap-2">
            {APPLY_TARGETS.map((target) => (
              <button
                key={target.id}
                type="button"
                onClick={() => void aplicar(target.id)}
                disabled={Boolean(applying)}
                className="rounded-lg border border-white/10 bg-white/[0.03] px-3.5 py-1.5 text-[11px] font-medium text-white/70 transition-colors hover:border-white/25 hover:text-white disabled:opacity-40"
              >
                {applying === target.id ? t("retroachievements.aplicando") : target.label}
              </button>
            ))}
          </div>
          {applyMsg && <p className="mt-2 text-[12px] text-white/55">{applyMsg}</p>}
        </div>
      ) : (
        <div className="flex flex-col gap-2.5 sm:flex-row sm:items-end">
          <div className="flex-1">
            <label className="mb-1.5 block text-[12px] text-white/55">{t("retroachievements.usuario")}</label>
            <input
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              spellCheck={false}
              autoComplete="username"
              className="w-full rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none transition-colors placeholder:text-white/25 focus:border-[color:var(--accent)]"
            />
          </div>
          <div className="flex-1">
            <label className="mb-1.5 block text-[12px] text-white/55">{t("retroachievements.senha")}</label>
            <input
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              type="password"
              autoComplete="current-password"
              className="w-full rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-2.5 text-[13px] text-white outline-none transition-colors placeholder:text-white/25 focus:border-[color:var(--accent)]"
            />
          </div>
          <button
            type="button"
            onClick={conectar}
            disabled={busy || !username.trim() || !password}
            className="rounded-lg px-4 py-2.5 text-[12px] font-bold text-black transition-transform enabled:hover:scale-[1.03] disabled:opacity-50"
            style={{ background: "var(--accent)" }}
          >
            {busy ? t("retroachievements.conectando") : t("retroachievements.conectar")}
          </button>
        </div>
      )}
      {error && <p className="mt-2.5 text-[12px] text-red-300/80">{error}</p>}
    </section>
  )
}
