"use client"

// Tela de amigos — identidade do launcher: cards com avatar colorido,
// hover com glow, ações por estado e perfil do amigo (FriendProfileView).
// Seções: busca, pedidos recebidos/enviados e lista de amigos.
import { useCallback, useEffect, useRef, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { useAccount } from "../account/AccountContext"
import { useFriends } from "../account/FriendsContext"
import { corDeUsername, inicialDe, formatarData } from "../account/avatar"
import { FriendProfileView } from "./FriendProfileView"

interface Resultado {
  id: string
  username: string
  avatar_url?: string | null
  status?: "pending" | "accepted" | null
  incoming?: boolean
}

function Avatar({ nome, url, tamanho = "md" }: { nome: string; url?: string | null; tamanho?: "md" | "lg" }) {
  const cls = tamanho === "lg" ? "h-12 w-12 text-lg" : "h-10 w-10 text-sm"
  if (url) return <img src={url} alt="" className={`${cls} shrink-0 rounded-xl border border-white/10 object-cover`} />
  return (
    <div
      className={`${cls} flex shrink-0 items-center justify-center rounded-xl border border-white/10 font-bold text-white`}
      style={{ background: corDeUsername(nome) }}
    >
      {inicialDe(nome)}
    </div>
  )
}

export function FriendsView() {
  const { t } = useI18n()
  const { status } = useAccount()
  const { data, refresh } = useFriends()

  const [busca, setBusca] = useState("")
  const [resultados, setResultados] = useState<Resultado[] | null>(null)
  const [buscando, setBuscando] = useState(false)
  const [feedback, setFeedback] = useState<{ texto: string; cor: "ok" | "erro" } | null>(null)
  const [amigoPerfil, setAmigoPerfil] = useState<FriendProfile | null>(null)
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const avisar = (texto: string, cor: "ok" | "erro") => {
    setFeedback({ texto, cor })
    setTimeout(() => setFeedback(null), 2500)
  }

  const buscar = useCallback(async (q: string) => {
    const termo = q.trim()
    if (!termo) {
      setResultados(null)
      return
    }
    setBuscando(true)
    const r = await window.launcherAPI?.friendsSearch(termo)
    setBuscando(false)
    if (r?.ok) setResultados(r.results ?? [])
    else avisar(r?.error || t("amigos.erro_busca"), "erro")
  }, [t])

  // Busca com debounce (300ms).
  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current)
    debounceRef.current = setTimeout(() => buscar(busca), 300)
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current)
    }
  }, [busca, buscar])

  const enviar = async (id: string) => {
    const r = await window.launcherAPI?.friendsSend(id)
    if (r?.ok) {
      avisar(t("amigos.pedido_enviado"), "ok")
      await refresh()
      buscar(busca)
    } else avisar(r?.error || t("amigos.erro_geral"), "erro")
  }

  const aceitar = async (id: string) => {
    const r = await window.launcherAPI?.friendsAccept(id)
    if (r?.ok) {
      avisar(t("amigos.aceito"), "ok")
      await refresh()
    } else avisar(r?.error || t("amigos.erro_geral"), "erro")
  }

  const recusar = async (id: string) => {
    const r = await window.launcherAPI?.friendsCancel(id)
    if (r?.ok) {
      avisar(t("amigos.recusado"), "ok")
      await refresh()
    } else avisar(r?.error || t("amigos.erro_geral"), "erro")
  }

  if (status !== "logado") {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 p-8 text-center">
        <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-white/[0.04] text-3xl">
          👋
        </div>
        <p className="text-lg font-semibold text-white/80">{t("amigos.nao_logado")}</p>
        <p className="max-w-md text-sm text-white/40">{t("amigos.nao_logado_hint")}</p>
      </div>
    )
  }

  // Perfil do amigo aberto
  if (amigoPerfil) {
    return (
      <div className="h-full overflow-y-auto p-6">
        <FriendProfileView
          amigo={amigoPerfil}
          onVoltar={() => {
            setAmigoPerfil(null)
            refresh()
          }}
          onRemovido={() => {
            avisar(t("amigos.removido"), "ok")
            setAmigoPerfil(null)
            refresh()
          }}
        />
      </div>
    )
  }

  const botaoAcao = (label: string, onClick: () => void, cor: "azul" | "verde" | "laranja" | "cinza" = "azul") => (
    <button
      onClick={onClick}
      className={`shrink-0 rounded-lg px-3 py-1.5 text-xs font-semibold transition-all disabled:opacity-40 ${
        cor === "azul"
          ? "bg-[#0072ce]/85 text-white hover:bg-[#0072ce] hover:shadow-[0_0_18px_rgba(0,114,206,0.4)]"
          : cor === "verde"
            ? "bg-[#4adf9a]/15 text-[#4adf9a] hover:bg-[#4adf9a]/25"
            : cor === "laranja"
              ? "bg-[#f5a623]/15 text-[#f5a623] hover:bg-[#f5a623]/25"
              : "border border-white/10 text-white/50 hover:text-white"
      }`}
    >
      {label}
    </button>
  )

  const card = (
    p: { id: string; username: string; avatar_url?: string | null; since?: string | null },
    acoes?: React.ReactNode,
    clicavel = false,
  ) => (
    <div
      key={p.id}
      onClick={clicavel ? () => setAmigoPerfil(p as FriendProfile) : undefined}
      className={`group flex items-center justify-between gap-3 rounded-xl border border-white/10 bg-white/[0.03] px-4 py-3 transition-all ${
        clicavel
          ? "cursor-pointer hover:border-[#00a8ff]/40 hover:bg-white/[0.06] hover:shadow-[0_0_24px_rgba(0,168,255,0.12)]"
          : ""
      }`}
    >
      <div className="flex min-w-0 items-center gap-3">
        <Avatar nome={p.username} url={p.avatar_url} />
        <div className="min-w-0 leading-tight">
          <div className="truncate text-sm font-medium text-white/90">{p.username}</div>
          {p.since && (
            <div className="text-[11px] text-white/35">
              {t("amigos.desde")} {formatarData(p.since)}
            </div>
          )}
        </div>
      </div>
      {acoes}
    </div>
  )

  const secao = (titulo: string, contador: number, children: React.ReactNode, cor?: string) => (
    <div className="mb-6">
      <div className="mb-2.5 flex items-center gap-2">
        <h3 className="text-[11px] font-semibold uppercase tracking-[0.15em] text-white/40">
          {titulo}
        </h3>
        {contador > 0 && (
          <span
            className="flex h-4 min-w-4 items-center justify-center rounded-full px-1.5 text-[10px] font-bold"
            style={{ background: cor || "rgba(255,255,255,0.08)", color: cor ? "#fff" : "rgba(255,255,255,0.5)" }}
          >
            {contador}
          </span>
        )}
      </div>
      {children}
    </div>
  )

  return (
    <div className="arc-fade-up h-full overflow-y-auto p-6">
      <style>{`
        @keyframes arc-spin { to { transform: rotate(360deg) } }
        @keyframes arc-fade-up { from { opacity: 0; transform: translateY(16px) } to { opacity: 1; transform: none } }
        @keyframes arc-fade-in { from { opacity: 0 } to { opacity: 1 } }
      `}</style>
      <h2 className="mb-5 text-xl font-bold text-white">{t("amigos.titulo")}</h2>

      {feedback && (
        <div
          className={`arc-fade-in mb-4 rounded-xl border px-3.5 py-2.5 text-xs ${
            feedback.cor === "ok"
              ? "border-[#4adf9a]/25 bg-[#4adf9a]/[0.07] text-[#4adf9a]"
              : "border-[#ff6b81]/25 bg-[#ff6b81]/[0.07] text-[#ff6b81]"
          }`}
        >
          {feedback.texto}
        </div>
      )}

      {/* Busca */}
      <div className="relative mb-6">
        <span className="pointer-events-none absolute left-3.5 top-1/2 -translate-y-1/2 text-white/30">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4">
            <circle cx="11" cy="11" r="7" />
            <path d="m20 20-3.5-3.5" />
          </svg>
        </span>
        <input
          value={busca}
          onChange={(e) => setBusca(e.target.value)}
          placeholder={t("amigos.busca_placeholder")}
          className="w-full rounded-xl border border-white/10 bg-white/[0.04] py-2.5 pl-10 pr-4 text-sm text-white placeholder-white/25 outline-none transition-all focus:border-[#00a8ff]/60 focus:shadow-[0_0_0_3px_rgba(0,168,255,0.12)]"
        />
        {buscando && (
          <span
            className="absolute right-4 top-1/2 h-4 w-4 -translate-y-1/2 rounded-full border-2 border-white/20 border-t-white/80"
            style={{ animation: "arc-spin .7s linear infinite" }}
          />
        )}
      </div>

      {/* Resultados da busca */}
      {resultados && (
        <div className="mb-6">
          <h3 className="mb-2.5 text-[11px] font-semibold uppercase tracking-[0.15em] text-white/40">
            {t("amigos.resultados")}
          </h3>
          {resultados.length === 0 ? (
            <p className="text-sm text-white/40">{t("amigos.sem_resultados")}</p>
          ) : (
            <div className="flex flex-col gap-2">
              {resultados.map((r) => {
                if (r.status === "accepted")
                  return card(r, botaoAcao(t("amigos.ver_perfil"), () => setAmigoPerfil(r as FriendProfile)), true)
                if (r.status === "pending")
                  return card(
                    r,
                    r.incoming
                      ? botaoAcao(t("amigos.aceitar"), () => aceitar(r.id), "verde")
                      : botaoAcao(t("amigos.pendente"), () => {}, "laranja"),
                  )
                return card(r, botaoAcao(t("amigos.adicionar"), () => enviar(r.id)))
              })}
            </div>
          )}
        </div>
      )}

      {/* Pedidos recebidos */}
      {(data?.incoming?.length ?? 0) > 0 &&
        secao(
          t("amigos.pedidos_recebidos"),
          data?.incoming?.length ?? 0,
          <div className="flex flex-col gap-2">
            {data?.incoming.map((p) =>
              card(p, (
                <>
                  {botaoAcao(t("amigos.aceitar"), () => aceitar(p.id), "verde")}
                  {botaoAcao(t("amigos.recusar"), () => recusar(p.id), "cinza")}
                </>
              )),
            )}
          </div>,
          "#f5a623",
        )}

      {/* Pedidos enviados */}
      {(data?.outgoing?.length ?? 0) > 0 &&
        secao(
          t("amigos.pedidos_enviados"),
          data?.outgoing?.length ?? 0,
          <div className="flex flex-col gap-2">
            {data?.outgoing.map((p) =>
              card(p, botaoAcao(t("amigos.cancelar"), () => recusar(p.id), "cinza")),
            )}
          </div>,
        )}

      {/* Amigos */}
      {secao(
        t("amigos.lista"),
        data?.friends?.length ?? 0,
        (data?.friends?.length ?? 0) === 0 ? (
          <div className="rounded-2xl border border-dashed border-white/10 py-10 text-center">
            <div className="mb-2 text-2xl">🫂</div>
            <p className="text-sm text-white/35">{t("amigos.vazio")}</p>
          </div>
        ) : (
          <div className="flex flex-col gap-2">
            {data?.friends.map((p) =>
              card(p, botaoAcao(t("amigos.ver_perfil"), () => setAmigoPerfil(p), "cinza"), true),
            )}
          </div>
        ),
      )}
    </div>
  )
}
