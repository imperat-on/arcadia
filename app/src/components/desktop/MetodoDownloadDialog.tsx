"use client"

import { useEffect, useRef, useState } from "react"
import type { JogoLoja, OpcaoTorrent } from "../useStoreActions"
import { useI18n } from "../../i18n/I18nContext"
import { useGamepadNav } from "../ps5-launcher/useGamepadNav"

// Diálogo de download em 3 etapas (jogo da loja que também existe nas fontes):
//   1. MÉTODO: "Download via Depot" (fluxo Steam de sempre) ou "via Torrent".
//   2. FONTE: uma linha por fonte com magnet — nome, release completa, tamanho.
//   3. PASTA: padrão do Arcadia (config default_install_path ou ~/Games/Arcadia);
//      clicar nela abre o seletor de pastas. Confirmar dispara o torrent.
// Botão "Voltar" em cada etapa devolve para a anterior sem fechar o diálogo.
type Etapa = "metodo" | "fonte" | "pasta"

// Ícones próprios do diálogo, na gramática do Sidebar (grade 24, traço 1.6,
// pontas arredondadas) — sem puxar uma lib de ícones só por causa de quatro
// símbolos.
const svgBase = {
  width: 20,
  height: 20,
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.6,
  strokeLinecap: "round",
  strokeLinejoin: "round",
} as const

// Depot: nuvem com seta para baixo (download nativo, registrado na Steam).
function IconDepot() {
  return (
    <svg {...svgBase}>
      <path d="M17.4 18H7a4 4 0 0 1-.6-7.96 5.5 5.5 0 0 1 10.72-1.2A3.75 3.75 0 0 1 17.4 18Z" />
      <path d="M12 11.5v4" />
      <path d="m10 13.6 2 2 2-2" />
    </svg>
  )
}

// Torrent: ímã em U, com as listras dos polos nas duas pernas.
function IconMagnet() {
  return (
    <svg {...svgBase}>
      <path d="M7 5v7a5 5 0 0 0 10 0V5" />
      <path d="M5.5 9h3" />
      <path d="M15.5 9h3" />
    </svg>
  )
}

// Fonte: elo de corrente, o mesmo símbolo da seção Fontes do app.
function IconLink() {
  return (
    <svg {...svgBase}>
      <path d="M10 14a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1.5 1.5" />
      <path d="M14 10a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7L12.5 19" />
    </svg>
  )
}

function IconChevron({ className }: { className?: string }) {
  return (
    <svg {...svgBase} width={16} height={16} className={className}>
      <path d="m9 6 6 6-6 6" />
    </svg>
  )
}

function IconAlert() {
  return (
    <svg {...svgBase}>
      <path d="M12 4 3 20h18L12 4Z" />
      <path d="M12 10.5v4" />
      <path d="M12 17.2h.01" />
    </svg>
  )
}

function IconFolder() {
  return (
    <svg {...svgBase}>
      <path d="M4 7a2 2 0 0 1 2-2h3l2 2h7a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2Z" />
    </svg>
  )
}

export function MetodoDownloadDialog({
  jogo,
  opcoes,
  onDepot,
  onTorrent,
  onClose,
  depotDisponivel = true,
}: {
  jogo: JogoLoja
  opcoes: OpcaoTorrent[]
  onDepot: () => void
  onTorrent: (magnet: string, savePath: string) => void
  onClose: () => void
  // Quando integração SLSsteam está desligada, só torrent existe — pulamos a
  // tela de escolha e o botão Depot some.
  depotDisponivel?: boolean
}) {
  const { t } = useI18n()
  const ref = useRef<HTMLDivElement>(null)
  const [etapa, setEtapa] = useState<Etapa>(depotDisponivel ? "metodo" : "fonte")
  const [escolhida, setEscolhida] = useState<OpcaoTorrent | null>(null)
  const [pasta, setPasta] = useState("")
  const [livre, setLivre] = useState<number | null>(null)
  // Política 2026-09-12: release sem debrid não baixa. null = checando.
  const [debridOk, setDebridOk] = useState<boolean | null>(null)

  useGamepadNav(ref, true, onClose)

  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      ref.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus({ preventScroll: true })
    })
    return () => cancelAnimationFrame(frame)
  }, [etapa, opcoes.length])

  // Pasta padrão ao entrar na etapa 3: config.default_install_path ou
  // ~/Games/Arcadia (mesma regra do InstallDialog).
  useEffect(() => {
    if (etapa !== "pasta") return
    window.launcherAPI?.getConfig().then((c) => {
      setPasta(c?.default_install_path || `${window.launcherPaths?.home || "~"}/Games/Arcadia`)
    })
  }, [etapa])

  useEffect(() => {
    if (!pasta) return
    window.launcherAPI?.diskSpace(pasta).then((r) => {
      setLivre(r?.ok ? (r.free ?? null) : null)
    })
  }, [pasta])

  // Debrid configurado? (sem ele, downloads de release ficam bloqueados)
  useEffect(() => {
    let vivo = true
    window.launcherAPI
      ?.debridStatus?.()
      .then((r) => {
        if (vivo) setDebridOk(r?.ok ? !!r.configured : null)
      })
      .catch(() => {})
    return () => {
      vivo = false
    }
  }, [])

  const escolherPasta = async () => {
    const r = await window.launcherAPI?.pickFolder()
    if (r?.ok && r.path) setPasta(r.path)
  }

  const voltar =
    etapa === "fonte"
      ? depotDisponivel
        ? () => setEtapa("metodo")
        : null
      : etapa === "pasta"
        ? () => setEtapa("fonte")
        : null

  return (
    <div data-no-drag
      ref={ref}
      className="fixed inset-0 z-[70] flex items-center justify-center bg-black/70 backdrop-blur-sm"
      onClick={onClose}
    >
      <div data-no-drag
        className="w-[440px] max-w-[92vw] rounded-2xl border border-white/[0.08] bg-[color:var(--surface-1)] p-5 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {etapa === "metodo" && (
          <>
            <h3 className="text-[15px] font-semibold text-white">
              {t("store.metodo.titulo", { title: jogo.title })}
            </h3>
            <p className="mt-1 mb-4 text-[12px] leading-relaxed text-white/40">
              {t("store.metodo.sub")}
            </p>
            <div className="flex flex-col gap-2.5">
              {depotDisponivel && (
                <button
                  onClick={onDepot}
                  className="group flex w-full items-center gap-3.5 rounded-2xl border border-white/10 bg-[color:var(--surface-2)] p-4 text-left transition-all duration-200 hover:-translate-y-0.5 hover:border-[color:var(--accent)]/60 hover:bg-[color:var(--surface-3)] hover:shadow-[0_14px_30px_-16px_rgba(0,0,0,0.9)] focus-visible:border-[color:var(--accent)]/60 focus-visible:bg-[color:var(--surface-3)]"
                >
                  <span className="grid h-11 w-11 shrink-0 place-items-center rounded-xl border border-[color:var(--brand-steam)]/25 bg-[color:var(--brand-steam)]/15 text-[color:var(--state-info-soft)] transition-colors group-hover:bg-[color:var(--brand-steam)]/25">
                    <IconDepot />
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="flex items-center gap-2">
                      <span className="truncate text-[14px] font-semibold text-white">
                        {t("store.metodo.depot")}
                      </span>
                      <span className="shrink-0 rounded-full border border-[color:var(--accent)]/40 bg-[color:var(--accent)]/10 px-1.5 py-0.5 text-[9px] font-bold uppercase tracking-wider text-[color:var(--accent)]">
                        {t("store.metodo.recomendado")}
                      </span>
                    </span>
                    <span className="mt-0.5 block truncate text-[11.5px] text-white/45">
                      {t("store.metodo.depot_sub")}
                    </span>
                  </span>
                  <IconChevron className="shrink-0 text-white/20 transition-all group-hover:translate-x-0.5 group-hover:text-[color:var(--accent)]" />
                </button>
              )}
              <button
                onClick={() => setEtapa("fonte")}
                disabled={debridOk === false}
                className="group flex w-full items-center gap-3.5 rounded-2xl border border-white/10 bg-[color:var(--surface-2)] p-4 text-left transition-all duration-200 hover:-translate-y-0.5 hover:border-[color:var(--accent)]/60 hover:bg-[color:var(--surface-3)] hover:shadow-[0_14px_30px_-16px_rgba(0,0,0,0.9)] focus-visible:border-[color:var(--accent)]/60 focus-visible:bg-[color:var(--surface-3)] disabled:translate-y-0 disabled:opacity-40 disabled:hover:border-white/10 disabled:hover:bg-[color:var(--surface-2)] disabled:hover:shadow-none"
              >
                <span className="grid h-11 w-11 shrink-0 place-items-center rounded-xl border border-white/[0.08] bg-white/[0.05] text-white/60 transition-colors group-hover:text-[color:var(--accent)]">
                  <IconMagnet />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[14px] font-semibold text-white">
                    {t("store.metodo.torrent")}
                  </span>
                  <span className="mt-0.5 block truncate text-[11.5px] text-white/45">
                    {debridOk === false
                      ? t("store.metodo.torrent_debrid")
                      : t("store.metodo.torrent_sub", { count: String(opcoes.length) })}
                  </span>
                </span>
                <IconChevron className="shrink-0 text-white/20 transition-all group-hover:translate-x-0.5 group-hover:text-[color:var(--accent)]" />
              </button>
            </div>
          </>
        )}

        {etapa === "fonte" && (
          <>
            <h3 className="text-[15px] font-semibold text-white">{t("store.fonte.titulo")}</h3>
            <p className="mt-1 mb-4 text-[12px] leading-relaxed text-white/40">
              {t("store.fonte.sub")}
            </p>
            {debridOk === false ? (
              <div className="flex items-start gap-3 rounded-2xl border border-[color:var(--state-warn)]/30 bg-[color:var(--state-warn)]/[0.08] p-4">
                <span className="mt-0.5 shrink-0 text-[color:var(--state-warn)]">
                  <IconAlert />
                </span>
                <p className="text-[12px] leading-relaxed text-white/70">
                  {t("store.fonte.debrid_obrigatorio")}
                </p>
              </div>
            ) : (
            <div className="flex max-h-[50vh] flex-col gap-2.5 overflow-y-auto pr-0.5">
              {opcoes.map((o, i) => (
                <button
                  key={o.ref}
                  onClick={() => {
                    setEscolhida(o)
                    setEtapa("pasta")
                  }}
                  className={`group flex w-full items-center gap-3.5 rounded-2xl border p-4 text-left transition-all duration-200 hover:-translate-y-0.5 hover:border-[color:var(--accent)]/60 hover:bg-[color:var(--surface-3)] hover:shadow-[0_14px_30px_-16px_rgba(0,0,0,0.9)] ${
                    i === 0
                      ? "border-[color:var(--accent)]/50 bg-[color:var(--accent)]/[0.07]"
                      : "border-white/10 bg-[color:var(--surface-2)]"
                  }`}
                >
                  <span
                    className={`grid h-11 w-11 shrink-0 place-items-center rounded-xl border transition-colors ${
                      i === 0
                        ? "border-[color:var(--accent)]/30 bg-[color:var(--accent)]/10 text-[color:var(--accent)]"
                        : "border-white/[0.08] bg-white/[0.05] text-white/60 group-hover:text-[color:var(--accent)]"
                    }`}
                  >
                    <IconLink />
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="flex items-center gap-2">
                      <span className="truncate text-[13.5px] font-semibold text-white/90">
                        {o.fonte}
                      </span>
                      {o.http && (
                        <span className="shrink-0 rounded bg-white/10 px-1.5 py-0.5 text-[9px] font-bold uppercase text-white/60">
                          HTTP
                        </span>
                      )}
                    </span>
                    <span className="mt-0.5 block truncate text-[11px] text-white/40">
                      {o.tituloFonte}
                    </span>
                  </span>
                  <span className="shrink-0 rounded-lg border border-white/[0.06] bg-white/[0.04] px-2 py-1 text-[11px] font-semibold text-white/70">
                    {o.fileSize}
                  </span>
                </button>
              ))}
            </div>
            )}
          </>
        )}

        {etapa === "pasta" && (
          <>
            <h3 className="text-[15px] font-semibold text-white">
              {t("store.torrent.titulo", { title: jogo.title })}
            </h3>
            <p className="mt-1 mb-4 text-[12px] leading-relaxed text-white/40">
              {escolhida?.fonte} · {escolhida?.fileSize} — {t("store.torrent.sub")}
            </p>
            <button
              onClick={escolherPasta}
              className="group flex w-full items-center gap-3.5 rounded-2xl border border-[color:var(--accent)]/50 bg-[color:var(--accent)]/[0.07] p-4 text-left transition-all duration-200 hover:border-[color:var(--accent)]/80 hover:bg-[color:var(--accent)]/[0.1]"
            >
              <span className="grid h-11 w-11 shrink-0 place-items-center rounded-xl border border-[color:var(--accent)]/30 bg-[color:var(--accent)]/10 text-[color:var(--accent)]">
                <IconFolder />
              </span>
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[13.5px] font-semibold text-white/90">
                  {pasta.replace(/^\/home\/[^/]+/, "~")}
                </span>
                <span className="mt-0.5 block text-[11px] text-white/45">
                  {livre !== null ? t("store.gb_livres", { free: livre.toFixed(2) }) : ""}
                </span>
              </span>
              <IconChevron className="shrink-0 text-white/20 transition-all group-hover:translate-x-0.5 group-hover:text-[color:var(--accent)]" />
            </button>
            <button
              onClick={() => escolhida && onTorrent(escolhida.magnet, pasta)}
              disabled={!pasta || !escolhida || debridOk === false}
              className="mt-3 w-full rounded-lg py-2.5 text-[12px] font-bold text-black transition-transform hover:scale-[1.02] disabled:opacity-40"
              style={{ background: "var(--accent)" }}
            >
              {t("store.torrent.confirmar")}
            </button>
          </>
        )}

        <div className="mt-3 flex gap-2">
          {voltar && (
            <button
              onClick={voltar}
              className="flex-1 rounded-lg border border-white/10 py-2 text-[12px] font-semibold text-white/50 transition-colors hover:border-white/25 hover:text-white/80"
            >
              {t("common.voltar")}
            </button>
          )}
          <button
            onClick={onClose}
            className="flex-1 rounded-lg border border-white/10 py-2 text-[12px] font-semibold text-white/50 transition-colors hover:border-white/25 hover:text-white/80"
          >
            {t("common.cancelar")}
          </button>
        </div>
      </div>
    </div>
  )
}
