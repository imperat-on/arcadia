"use client"

// TELA DE LOGIN do Arcadia — fullscreen, com a identidade do launcher:
// fundo escuro #0d0d10, neon azul (#0072ce/#00a8ff), glassmorphism e
// micro-animações CSS (sem dependências extra).
// Cadastro: EMAIL + USERNAME + SENHA (sem verificação — projeto libertário).
// Login: USERNAME + SENHA (email resolvido no main via RPC login_email).
import { useEffect, useRef, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { useAccount } from "../account/AccountContext"

interface AuthDialogProps {
  open: boolean
  onClose: () => void
  /** true = pós sign-out: o diálogo não pode ser dispensado (X/Escape/backdrop
   *  desativados) até o usuário escolher uma conta. */
  semFechar?: boolean
}

type Modo = "criar" | "entrar"

const inputBase =
  "w-full rounded-xl border bg-white/[0.04] px-4 py-3 pl-11 text-sm text-white " +
  "placeholder-white/25 outline-none transition-all duration-200 " +
  "border-white/10 focus:border-[#00a8ff]/60 focus:bg-white/[0.06] focus:shadow-[0_0_0_3px_rgba(0,168,255,0.12)]"

function IconMail() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" className="h-4 w-4">
      <rect x="3" y="5" width="18" height="14" rx="2" />
      <path d="m3 7 9 6 9-6" />
    </svg>
  )
}

function IconUser() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" className="h-4 w-4">
      <circle cx="12" cy="8" r="4" />
      <path d="M4 21c1.5-3.5 4.5-5 8-5s6.5 1.5 8 5" />
    </svg>
  )
}

function IconLock() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" className="h-4 w-4">
      <rect x="5" y="11" width="14" height="9" rx="2" />
      <path d="M8 11V8a4 4 0 0 1 8 0v3" />
    </svg>
  )
}

export function AuthDialog({ open, onClose, semFechar }: AuthDialogProps) {
  const { t } = useI18n()
  const { status, session, signUp, signIn, signOut } = useAccount()

  const [modo, setModo] = useState<Modo>("criar")
  const [email, setEmail] = useState("")
  const [username, setUsername] = useState("")
  const [senha, setSenha] = useState("")
  const [erro, setErro] = useState<string | null>(null)
  const [enviando, setEnviando] = useState(false)
  // Username REAL criado (pode diferir do pedido se o trigger renomeou por
  // colisão: joao → joao_1). O user_metadata do session traz o pedido.
  const [usernameReal, setUsernameReal] = useState<string | null>(null)
  const firstRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (open) {
      setUsernameReal(null)
      setTimeout(() => firstRef.current?.focus(), 250)
    }
  }, [open, modo])

  if (!open) return null

  const onEsc = (e: React.KeyboardEvent) => {
    if (e.key === "Escape" && !semFechar) onClose()
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
    try {
      const r =
        modo === "criar"
          ? await signUp(email.trim(), username.trim(), senha)
          : await signIn(username.trim(), senha)
      if (!r.ok) {
        setErro(erroKey(r.error))
        return
      }
      if (modo === "criar" && r.usernameReal) setUsernameReal(r.usernameReal)
      setSenha("")
    } catch (e) {
      setErro(t("account.erro_geral") + ` (${e?.message || "exceção"})`)
    } finally {
      setEnviando(false)
    }
  }

  const trocarModo = (m: Modo) => {
    setModo(m)
    setErro(null)
    setSenha("")
  }

  const podeEnviar =
    !enviando &&
    (modo === "criar"
      ? email.trim() && username.trim() && senha.length >= 6
      : username.trim() && senha.length >= 1)

  const logado = status === "logado" && session

  return (
    <div
      className="gp-scope fixed inset-0 z-[90] overflow-hidden bg-[#0d0d10]"
      onKeyDown={onEsc}
      tabIndex={-1}
    >
      {/* Fundo: glows neon + grid sutil */}
      <style>{`
        @keyframes arc-glow-a { 0%,100%{opacity:.55;transform:translate(0,0) scale(1)} 50%{opacity:.85;transform:translate(40px,-30px) scale(1.1)} }
        @keyframes arc-glow-b { 0%,100%{opacity:.4;transform:translate(0,0) scale(1)} 50%{opacity:.7;transform:translate(-35px,28px) scale(.92)} }
        @keyframes arc-fade-up { from{opacity:0;transform:translateY(16px)} to{opacity:1;transform:none} }
        @keyframes arc-fade-in { from{opacity:0} to{opacity:1} }
        @keyframes arc-pop { 0%{transform:scale(.6);opacity:0} 60%{transform:scale(1.12)} 100%{transform:scale(1);opacity:1} }
        @keyframes arc-spin { to{transform:rotate(360deg)} }
        @keyframes arc-shine { 0%{transform:translateX(-120%)} 60%,100%{transform:translateX(220%)} }
        .arc-anim-a { animation: arc-glow-a 9s ease-in-out infinite }
        .arc-anim-b { animation: arc-glow-b 12s ease-in-out infinite }
        .arc-fade-up { animation: arc-fade-up .45s cubic-bezier(.2,.7,.3,1) both }
        .arc-fade-in { animation: arc-fade-in .3s ease-out both }
        .arc-pop { animation: arc-pop .5s cubic-bezier(.2,.7,.3,1.4) both }
        .arc-btn-shine { position:relative; overflow:hidden }
        .arc-btn-shine::after { content:""; position:absolute; top:0; bottom:0; left:0; width:45%; background:linear-gradient(90deg,transparent,rgba(255,255,255,.28),transparent); animation:arc-shine 2.8s ease-in-out infinite }
      `}</style>

      {/* Glows de fundo */}
      <div className="pointer-events-none absolute inset-0">
        <div className="arc-anim-a absolute -top-40 left-1/4 h-[480px] w-[480px] rounded-full bg-[#0072ce]/25 blur-[130px]" />
        <div className="arc-anim-b absolute -bottom-48 right-1/5 h-[420px] w-[420px] rounded-full bg-[#00a8ff]/20 blur-[120px]" />
        <div className="absolute left-1/2 top-1/2 h-[300px] w-[600px] -translate-x-1/2 -translate-y-1/2 rounded-full bg-[#0072ce]/[0.06] blur-[90px]" />
        {/* grid sutil */}
        <div
          className="absolute inset-0 opacity-[0.35]"
          style={{
            backgroundImage:
              "linear-gradient(rgba(255,255,255,.025) 1px, transparent 1px), linear-gradient(90deg, rgba(255,255,255,.025) 1px, transparent 1px)",
            backgroundSize: "56px 56px",
            maskImage: "radial-gradient(ellipse at center, black 30%, transparent 75%)",
            WebkitMaskImage: "radial-gradient(ellipse at center, black 30%, transparent 75%)",
          }}
        />
      </div>

      {/* Botão fechar (escondido quando pós sign-out — login é obrigatório) */}
      {!semFechar && (
        <button
          onClick={onClose}
          className="absolute right-5 top-5 z-10 flex h-9 w-9 items-center justify-center rounded-full border border-white/10 bg-white/[0.04] text-white/60 transition-all hover:border-white/20 hover:text-white"
          aria-label="Fechar"
        >
          ✕
        </button>
      )}

      <div className="relative flex h-full w-full items-center justify-center p-6">
        <div className="flex w-full max-w-[420px] flex-col items-center">
          {/* Logo */}
          <div className="arc-fade-up mb-8 flex flex-col items-center" style={{ animationDelay: "0ms" }}>
            <div className="text-3xl font-bold tracking-[0.18em] text-white">
              ARCAD<span className="text-[#00a8ff]">IA</span>
            </div>
            <div className="mt-1 text-[11px] uppercase tracking-[0.35em] text-white/30">
              Launcher
            </div>
          </div>

          {/* Card */}
          <div
            className="arc-fade-up w-full rounded-3xl border border-white/10 bg-white/[0.03] p-7 shadow-[0_20px_80px_rgba(0,0,0,0.6)] backdrop-blur-2xl"
            style={{ animationDelay: "90ms" }}
          >
            {logado ? (
              <div className="arc-fade-in flex flex-col items-center gap-4 py-4">
                <div className="arc-pop flex h-20 w-20 items-center justify-center rounded-full bg-gradient-to-br from-[#4adf9a]/25 to-[#4adf9a]/5 text-[#4adf9a] shadow-[0_0_50px_rgba(74,223,154,0.25)]">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" className="h-9 w-9">
                    <path d="m5 13 4 4L19 7" />
                  </svg>
                </div>
                <div className="text-center">
                  <div className="text-lg font-semibold text-white">
                    {usernameReal || session.user?.username || session.user?.email}
                  </div>
                  <div className="mt-0.5 text-xs text-white/40">{session.user?.email}</div>
                </div>
                <div className="mt-2 w-full rounded-xl border border-white/10 bg-white/[0.03] px-4 py-2.5 text-center text-xs text-white/50">
                  {t("account.logado_ok")}
                </div>
                {usernameReal && usernameReal !== session.user?.username && (
                  <div className="mt-2 w-full rounded-xl border border-[#ffb454]/20 bg-[#ffb454]/[0.06] px-4 py-2.5 text-center text-xs text-[#ffb454]">
                    {t("account.username_ajustado", { pedido: session.user?.username, real: usernameReal })}
                  </div>
                )}
                <button
                  onClick={async () => {
                    await signOut()
                    setSenha("")
                  }}
                  className="mt-1 w-full rounded-xl border border-white/10 px-4 py-2.5 text-sm font-semibold text-[#ff6b81] transition-all hover:border-[#ff6b81]/40 hover:bg-[#ff6b81]/[0.06]"
                >
                  {t("account.sair")}
                </button>
              </div>
            ) : (
              <div className="flex flex-col gap-4">
                {/* Tabs */}
                <div className="relative flex rounded-xl border border-white/10 bg-black/30 p-1">
                  <div
                    className="absolute bottom-1 top-1 w-[calc(50%-4px)] rounded-lg bg-gradient-to-b from-[#0072ce] to-[#0057a3] shadow-[0_0_20px_rgba(0,114,206,0.4)] transition-transform duration-300 ease-out"
                    style={{ transform: modo === "criar" ? "translateX(0)" : "translateX(calc(100% + 8px))" }}
                  />
                  {(["criar", "entrar"] as Modo[]).map((m) => (
                    <button
                      key={m}
                      onClick={() => trocarModo(m)}
                      className={`relative z-10 flex-1 rounded-lg px-3 py-2 text-sm font-semibold transition-colors duration-300 ${
                        modo === m ? "text-white" : "text-white/40 hover:text-white/70"
                      }`}
                    >
                      {m === "criar" ? t("account.modo_criar") : t("account.modo_entrar")}
                    </button>
                  ))}
                </div>

                <p className="text-center text-xs text-white/40">
                  {modo === "criar" ? t("account.descricao_criar") : t("account.descricao")}
                </p>

                {/* Campos */}
                <div className="flex flex-col gap-3">
                  {modo === "criar" && (
                    <div className="relative">
                      <span className="pointer-events-none absolute left-3.5 top-1/2 -translate-y-1/2 text-white/30">
                        <IconMail />
                      </span>
                      <input
                        ref={firstRef}
                        type="email"
                        value={email}
                        onChange={(e) => setEmail(e.target.value)}
                        placeholder={t("account.email")}
                        className={inputBase}
                      />
                    </div>
                  )}

                  <div className="relative">
                    <span className="pointer-events-none absolute left-3.5 top-1/2 -translate-y-1/2 text-white/30">
                      <IconUser />
                    </span>
                    <input
                      ref={modo === "criar" ? undefined : firstRef}
                      type="text"
                      value={username}
                      onChange={(e) => setUsername(e.target.value)}
                      placeholder={t("account.username")}
                      autoCapitalize="none"
                      className={inputBase}
                    />
                  </div>

                  <div className="relative">
                    <span className="pointer-events-none absolute left-3.5 top-1/2 -translate-y-1/2 text-white/30">
                      <IconLock />
                    </span>
                    <input
                      type="password"
                      value={senha}
                      onChange={(e) => setSenha(e.target.value)}
                      placeholder={t("account.senha")}
                      onKeyDown={(e) => e.key === "Enter" && podeEnviar && continuar()}
                      className={inputBase}
                    />
                  </div>

                  {modo === "criar" && (
                    <p className="text-[11px] text-white/25">{t("account.username_hint")}</p>
                  )}

                  {erro && (
                    <div className="arc-fade-in rounded-xl border border-[#ff6b81]/25 bg-[#ff6b81]/[0.07] px-3.5 py-2.5 text-xs text-[#ff6b81]">
                      {erro}
                    </div>
                  )}
                </div>

                {/* Botão */}
                <button
                  onClick={continuar}
                  disabled={!podeEnviar}
                  className={`arc-btn-shine group relative mt-1 flex items-center justify-center gap-2 rounded-xl bg-gradient-to-b from-[#0072ce] to-[#0057a3] px-4 py-3 text-sm font-bold text-white shadow-[0_8px_30px_rgba(0,114,206,0.35)] transition-all duration-200 hover:shadow-[0_8px_40px_rgba(0,168,255,0.5)] hover:brightness-110 active:scale-[0.98] disabled:opacity-40 disabled:shadow-none ${
                    enviando ? "cursor-wait" : ""
                  }`}
                >
                  {enviando ? (
                    <>
                      <span
                        className="h-4 w-4 rounded-full border-2 border-white/30 border-t-white"
                        style={{ animation: "arc-spin .7s linear infinite" }}
                      />
                      {t("account.enviando")}
                    </>
                  ) : (
                    <>
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" className="h-4 w-4 transition-transform group-hover:translate-x-0.5">
                        <path d="M5 12h14M13 6l6 6-6 6" />
                      </svg>
                      {modo === "criar" ? t("account.criar_conta_btn") : t("account.entrar_btn")}
                    </>
                  )}
                </button>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
