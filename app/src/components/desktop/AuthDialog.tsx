"use client"

// Diálogo de conta online (Supabase).
// Dois modos:
//  - "criar": cadastro INSTANTÂNEO — email + username, sem verificação
//    (projeto open-source libertário: zero fricção).
//  - "entrar": conta existente — email → código de 6 dígitos (prova de posse).
import { useEffect, useRef, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { useAccount } from "../account/AccountContext"

interface AuthDialogProps {
  open: boolean
  onClose: () => void
}

type Passo = "email" | "codigo" | "logado"
type Modo = "criar" | "entrar"

export function AuthDialog({ open, onClose }: AuthDialogProps) {
  const { t } = useI18n()
  const { status, session, signUp, requestCode, verifyCode, signOut } = useAccount()

  const [passo, setPasso] = useState<Passo>("email")
  const [modo, setModo] = useState<Modo>("criar")
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
      case "confirmacao_necessaria":
        return t("account.erro_confirmacao")
      default:
        return t("account.erro_geral") + (raw ? ` (${raw})` : "")
    }
  }

  const continuar = async () => {
    setErro(null)
    setEnviando(true)
    if (modo === "criar") {
      const r = await signUp(email.trim(), username.trim())
      setEnviando(false)
      if (!r.ok) {
        setErro(erroKey(r.error))
        return
      }
      setPasso("logado")
    } else {
      const r = await requestCode(email.trim())
      setEnviando(false)
      if (!r.ok) {
        setErro(erroKey(r.error))
        return
      }
      setPasso("codigo")
    }
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

  const trocarModo = (m: Modo) => {
    setModo(m)
    setErro(null)
    setToken("")
    setPasso("email")
  }

  const inputCls =
    "rounded-lg border border-white/10 bg-white/[0.05] px-3 py-2 text-sm text-white placeholder-white/30 outline-none focus:border-[#00a8ff]"

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
            {/* Seletor de modo */}
            <div className="flex rounded-lg border border-white/10 p-0.5">
              {(["criar", "entrar"] as Modo[]).map((m) => (
                <button
                  key={m}
                  onClick={() => trocarModo(m)}
                  className={`flex-1 rounded-md px-3 py-1.5 text-sm font-medium transition-colors ${
                    modo === m ? "bg-[#0072ce] text-white" : "text-white/50 hover:text-white/80"
                  }`}
                >
                  {m === "criar" ? t("account.modo_criar") : t("account.modo_entrar")}
                </button>
              ))}
            </div>

            <p className="text-sm text-white/60">
              {modo === "criar" ? t("account.descricao_criar") : t("account.descricao")}
            </p>

            <input
              ref={emailRef}
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder={t("account.email")}
              className={inputCls}
            />

            {modo === "criar" && (
              <>
                <input
                  type="text"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  placeholder={t("account.username")}
                  className={inputCls}
                />
                <p className="text-[11px] text-white/40">{t("account.username_hint")}</p>
              </>
            )}

            {erro && <p className="text-sm text-[#ff6b6b]">{erro}</p>}

            <button
              onClick={continuar}
              disabled={
                enviando ||
                !email.trim() ||
                (modo === "criar" && !username.trim())
              }
              className="mt-1 rounded-lg bg-[#0072ce] px-4 py-2 text-sm font-semibold text-white transition-colors hover:brightness-110 disabled:opacity-40"
            >
              {enviando
                ? "…"
                : modo === "criar"
                  ? t("account.criar_conta_btn")
                  : t("account.enviar_codigo")}
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
