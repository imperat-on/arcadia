"use client"

// Diálogo de conta online (Supabase).
// Cadastro: EMAIL + USERNAME + SENHA (sem verificação — projeto libertário).
// Login: USERNAME + SENHA (o email é resolvido no main via RPC login_email).
import { useEffect, useRef, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { useAccount } from "../account/AccountContext"

interface AuthDialogProps {
  open: boolean
  onClose: () => void
}

type Modo = "criar" | "entrar"

export function AuthDialog({ open, onClose }: AuthDialogProps) {
  const { t } = useI18n()
  const { status, session, signUp, signIn, signOut } = useAccount()

  const [modo, setModo] = useState<Modo>("criar")
  const [email, setEmail] = useState("")
  const [username, setUsername] = useState("")
  const [senha, setSenha] = useState("")
  const [erro, setErro] = useState<string | null>(null)
  const [enviando, setEnviando] = useState(false)
  const firstRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (open && status === "logado") return // fica na tela "logado"
    if (open) setTimeout(() => firstRef.current?.focus(), 50)
  }, [open, status])

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
      case "senha_curta":
        return t("account.erro_senha_curta")
      case "usuario_nao_existe":
        return t("account.erro_usuario_nao_existe")
      case "credenciais_invalidas":
        return t("account.erro_credenciais")
      case "confirmacao_necessaria":
        return t("account.erro_confirmacao")
      default:
        return t("account.erro_geral") + (raw ? ` (${raw})` : "")
    }
  }

  const continuar = async () => {
    setErro(null)
    setEnviando(true)
    const r =
      modo === "criar"
        ? await signUp(email.trim(), username.trim(), senha)
        : await signIn(username.trim(), senha)
    setEnviando(false)
    if (!r.ok) {
      setErro(erroKey(r.error))
      return
    }
    // logado: limpa campos pra próxima vez
    setSenha("")
  }

  const trocarModo = (m: Modo) => {
    setModo(m)
    setErro(null)
    setSenha("")
  }

  const inputCls =
    "rounded-lg border border-white/10 bg-white/[0.05] px-3 py-2 text-sm text-white placeholder-white/30 outline-none focus:border-[#00a8ff]"

  const podeEnviar =
    !enviando &&
    (modo === "criar"
      ? email.trim() && username.trim() && senha.length >= 6
      : username.trim() && senha.length >= 1)

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

        {status === "logado" && session ? (
          <div className="flex flex-col gap-3">
            <div className="flex items-center gap-3 rounded-xl border border-white/10 bg-white/[0.04] p-3">
              <div className="flex h-10 w-10 items-center justify-center rounded-full bg-[#0072ce] text-sm font-bold text-white">
                {(session.user?.username?.[0] || session.user?.email?.[0] || "?").toUpperCase()}
              </div>
              <div className="min-w-0 leading-tight">
                <div className="truncate text-sm font-semibold text-white">
                  {session.user?.username || session.user?.email}
                </div>
                <div className="truncate text-xs text-white/40">{session.user?.email}</div>
              </div>
            </div>
            <button
              onClick={async () => {
                await signOut()
                setSenha("")
              }}
              className="rounded-lg border border-white/10 px-4 py-2 text-sm font-semibold text-[#ff6b6b] transition-colors hover:bg-white/5"
            >
              {t("account.sair")}
            </button>
          </div>
        ) : (
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

            {modo === "criar" && (
              <input
                ref={firstRef}
                type="email"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                placeholder={t("account.email")}
                className={inputCls}
              />
            )}

            <input
              ref={modo === "criar" ? undefined : firstRef}
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder={t("account.username")}
              autoCapitalize="none"
              className={inputCls}
            />
            {modo === "criar" && (
              <p className="text-[11px] text-white/40">{t("account.username_hint")}</p>
            )}

            <input
              type="password"
              value={senha}
              onChange={(e) => setSenha(e.target.value)}
              placeholder={t("account.senha")}
              className={inputCls}
            />

            {erro && <p className="text-sm text-[#ff6b6b]">{erro}</p>}

            <button
              onClick={continuar}
              disabled={!podeEnviar}
              className="mt-1 rounded-lg bg-[#0072ce] px-4 py-2 text-sm font-semibold text-white transition-colors hover:brightness-110 disabled:opacity-40"
            >
              {enviando ? "…" : modo === "criar" ? t("account.criar_conta_btn") : t("account.entrar_btn")}
            </button>
          </div>
        )}
      </div>
    </div>
  )
}
