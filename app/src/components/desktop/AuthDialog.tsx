"use client"

// Diálogo de conta online (Supabase): entrar / criar conta com código OTP.
// Fluxo: email (+ username p/ conta nova) → código de 6 dígitos → logado.
import { useEffect, useRef, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { useAccount } from "../account/AccountContext"

interface AuthDialogProps {
  open: boolean
  onClose: () => void
}

type Passo = "email" | "codigo" | "logado"

export function AuthDialog({ open, onClose }: AuthDialogProps) {
  const { t } = useI18n()
  const { status, session, requestCode, verifyCode, signOut } = useAccount()

  const [passo, setPasso] = useState<Passo>("email")
  const [email, setEmail] = useState("")
  const [username, setUsername] = useState("")
  const [token, setToken] = useState("")
  const [erro, setErro] = useState<string | null>(null)
  const [enviando, setEnviando] = useState(false)
  const emailRef = useRef<HTMLInputElement>(null)

  // Sincroniza o passo com o estado real da conta.
  useEffect(() => {
    if (open && status === "logado") setPasso("logado")
    if (open && status === "deslogado" && passo === "logado") setPasso("email")
  }, [open, status, passo])

  useEffect(() => {
    if (open) setTimeout(() => emailRef.current?.focus(), 50)
  }, [open])

  if (!open) return null

  const onEsc = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") onClose()
  }

  const enviarCodigo = async () => {
    setErro(null)
    setEnviando(true)
    const r = await requestCode(email.trim(), username.trim() || undefined)
    setEnviando(false)
    if (!r.ok) {
      setErro(erroKey(r.error))
      return
    }
    setPasso("codigo")
  }

  const confirmar = async () => {
    setErro(null)
    setEnviando(true)
    const r = await verifyCode(email.trim(), token.trim())
    setEnviando(false)
    if (!r.ok) {
      setErro(erroKey(r.error))
      return
    }
    setPasso("logado")
  }

  const erroKey = (raw?: string) => {
    switch (raw) {
      case "email_invalido":
        return t("account.erro_email")
      case "username_invalido":
        return t("account.erro_username")
      case "username_ocupado":
        return t("account.erro_ocupado")
      case "codigo_invalido":
        return t("account.erro_codigo")
      default:
        return t("account.erro_geral") + (raw ? ` (${raw})` : "")
    }
  }

  return (
    <div
      className="gp-scope fixed inset-0 z-[90] flex items-center justify-center bg-black/70 backdrop-blur-sm"
      onKeyDown={onEsc}
      tabIndex={-1}
    >
      <div className="w-[400px] rounded-2xl border border-white/10 bg-[#16161c]/95 p-6 shadow-2xl shadow-black/50">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-lg font-semibold text-white">{t("account.titulo")}</h2>
          <button
            onClick={onClose}
            className="rounded-lg px-2 py-1 text-sm text-white/50 transition-colors hover:bg-white/10 hover:text-white"
          >
            ✕
          </button>
        </div>

        {passo === "email" && (
          <div className="flex flex-col gap-3">
            <p className="text-sm text-white/60">{t("account.descricao")}</p>
            <input
              ref={emailRef}
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder={t("account.email")}
              className="rounded-lg border border-white/10 bg-white/[0.05] px-3 py-2 text-sm text-white placeholder-white/30 outline-none focus:border-[#00a8ff]"
            />
            <input
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder={t("account.username")}
              title={t("account.username_hint")}
              className="rounded-lg border border-white/10 bg-white/[0.05] px-3 py-2 text-sm text-white placeholder-white/30 outline-none focus:border-[#00a8ff]"
            />
            <p className="text-[11px] text-white/40">{t("account.username_hint")}</p>
            {erro && <p className="text-sm text-[#ff6b6b]">{erro}</p>}
            <button
              onClick={enviarCodigo}
              disabled={enviando || !email.trim()}
              className="mt-1 rounded-lg bg-[#0072ce] px-4 py-2 text-sm font-semibold text-white transition-colors hover:brightness-110 disabled:opacity-40"
            >
              {enviando ? "…" : t("account.enviar_codigo")}
            </button>
          </div>
        )}

        {passo === "codigo" && (
          <div className="flex flex-col gap-3">
            <p className="text-sm text-white/60">
              {t("account.codigo_enviado", { email: email.trim() })}
            </p>
            <input
              value={token}
              onChange={(e) => setToken(e.target.value.replace(/\D/g, "").slice(0, 6))}
              placeholder="000000"
              inputMode="numeric"
              autoFocus
              className="rounded-lg border border-white/10 bg-white/[0.05] px-3 py-2 text-center text-xl tracking-[0.5em] text-white placeholder-white/30 outline-none focus:border-[#00a8ff]"
            />
            {erro && <p className="text-sm text-[#ff6b6b]">{erro}</p>}
            <button
              onClick={confirmar}
              disabled={enviando || token.length !== 6}
              className="mt-1 rounded-lg bg-[#0072ce] px-4 py-2 text-sm font-semibold text-white transition-colors hover:brightness-110 disabled:opacity-40"
            >
              {enviando ? "…" : t("account.confirmar")}
            </button>
            <button
              onClick={() => setPasso("email")}
              className="text-xs text-white/40 transition-colors hover:text-white/70"
            >
              ← {t("account.voltar")}
            </button>
          </div>
        )}

        {passo === "logado" && (
          <div className="flex flex-col gap-3">
            <div className="flex items-center gap-3 rounded-xl border border-white/10 bg-white/[0.04] p-3">
              <div className="flex h-10 w-10 items-center justify-center rounded-full bg-[#0072ce] text-sm font-bold text-white">
                {(session?.user?.username?.[0] || session?.user?.email?.[0] || "?").toUpperCase()}
              </div>
              <div className="min-w-0 leading-tight">
                <div className="truncate text-sm font-semibold text-white">
                  {session?.user?.username || session?.user?.email}
                </div>
                <div className="truncate text-xs text-white/40">{session?.user?.email}</div>
              </div>
            </div>
            <button
              onClick={async () => {
                await signOut()
                setToken("")
                setPasso("email")
              }}
              className="rounded-lg border border-white/10 px-4 py-2 text-sm font-semibold text-[#ff6b6b] transition-colors hover:bg-white/5"
            >
              {t("account.sair")}
            </button>
          </div>
        )}
      </div>
    </div>
  )
}
