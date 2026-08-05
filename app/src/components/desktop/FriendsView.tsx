"use client"

// Tela de amigos: busca de usuários, pedidos recebidos/enviados e lista.
// Requer conta logada (mostra convite para entrar caso contrário).
import { useCallback, useEffect, useRef, useState } from "react"
import { useI18n } from "../../i18n/I18nContext"
import { useAccount } from "../account/AccountContext"
import { useFriends } from "../account/FriendsContext"

interface Resultado {
  id: string
  username: string
  status?: "pending" | "accepted" | null
  incoming?: boolean
}

export function FriendsView() {
  const { t } = useI18n()
  const { status } = useAccount()
  const { data, refresh } = useFriends()

  const [busca, setBusca] = useState("")
  const [resultados, setResultados] = useState<Resultado[] | null>(null)
  const [buscando, setBuscando] = useState(false)
  const [feedback, setFeedback] = useState<{ texto: string; cor: "ok" | "erro" } | null>(null)
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
        <p className="text-lg font-semibold text-white/80">{t("amigos.nao_logado")}</p>
        <p className="max-w-md text-sm text-white/40">{t("amigos.nao_logado_hint")}</p>
      </div>
    )
  }

  const linha = (p: Resultado, acao?: { label: string; onClick: () => void }) => (
    <div
      key={p.id}
      className="flex items-center justify-between rounded-xl border border-white/10 bg-white/[0.04] px-4 py-2.5"
    >
      <div className="flex min-w-0 items-center gap-3">
        <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-[#0072ce] text-xs font-bold text-white">
          {p.username[0]?.toUpperCase()}
        </div>
        <span className="truncate text-sm text-white/85">{p.username}</span>
      </div>
      {acao && (
        <button
          onClick={acao.onClick}
          className="shrink-0 rounded-lg bg-[#0072ce]/80 px-3 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-[#0072ce]"
        >
          {acao.label}
        </button>
      )}
    </div>
  )

  return (
    <div className="h-full overflow-y-auto p-6">
      <h2 className="mb-4 text-lg font-semibold text-white">{t("amigos.titulo")}</h2>

      {feedback && (
        <p
          className={`mb-3 text-sm ${feedback.cor === "ok" ? "text-[#4ade80]" : "text-[#ff6b6b]"}`}
        >
          {feedback.texto}
        </p>
      )}

      {/* Busca */}
      <input
        value={busca}
        onChange={(e) => setBusca(e.target.value)}
        placeholder={t("amigos.busca_placeholder")}
        className="mb-4 w-full rounded-xl border border-white/10 bg-white/[0.05] px-4 py-2.5 text-sm text-white placeholder-white/30 outline-none focus:border-[#00a8ff]"
      />

      {buscando && <p className="mb-3 text-xs text-white/40">…</p>}

      {resultados && (
        <div className="mb-6 flex flex-col gap-2">
          <h3 className="text-xs font-semibold uppercase tracking-wider text-white/40">
            {t("amigos.resultados")}
          </h3>
          {resultados.length === 0 && (
            <p className="text-sm text-white/40">{t("amigos.sem_resultados")}</p>
          )}
          {resultados.map((r) => {
            if (r.status === "accepted")
              return linha(r)
            if (r.status === "pending")
              return linha(r, {
                label: r.incoming ? t("amigos.aceitar") : t("amigos.pendente"),
                onClick: r.incoming ? () => aceitar(r.id) : () => {},
              })
            return linha(r, { label: t("amigos.adicionar"), onClick: () => enviar(r.id) })
          })}
        </div>
      )}

      {/* Pedidos recebidos */}
      {(data?.incoming?.length ?? 0) > 0 && (
        <div className="mb-6 flex flex-col gap-2">
          <h3 className="text-xs font-semibold uppercase tracking-wider text-[#f5a623]">
            {t("amigos.pedidos_recebidos")} ({data?.incoming?.length})
          </h3>
          {data?.incoming.map((p) =>
            linha(
              { ...p, status: "pending", incoming: true },
              { label: t("amigos.aceitar"), onClick: () => aceitar(p.id) },
            ),
          )}
        </div>
      )}

      {/* Pedidos enviados */}
      {(data?.outgoing?.length ?? 0) > 0 && (
        <div className="mb-6 flex flex-col gap-2">
          <h3 className="text-xs font-semibold uppercase tracking-wider text-white/40">
            {t("amigos.pedidos_enviados")} ({data?.outgoing?.length})
          </h3>
          {data?.outgoing.map((p) =>
            linha({ ...p, status: "pending" }, { label: t("amigos.cancelar"), onClick: () => recusar(p.id) }),
          )}
        </div>
      )}

      {/* Amigos */}
      <div className="flex flex-col gap-2">
        <h3 className="text-xs font-semibold uppercase tracking-wider text-white/40">
          {t("amigos.lista")} ({data?.friends?.length ?? 0})
        </h3>
        {(data?.friends?.length ?? 0) === 0 && (
          <p className="text-sm text-white/40">{t("amigos.vazio")}</p>
        )}
        {data?.friends.map((p) => linha(p))}
      </div>
    </div>
  )
}
