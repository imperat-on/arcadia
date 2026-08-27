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
    <div
      ref={ref}
      className="fixed inset-0 z-[70] flex items-center justify-center bg-black/70 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="w-[440px] max-w-[92vw] rounded-2xl border border-white/[0.08] bg-[#0d0d10] p-5 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {etapa === "metodo" && (
          <>
            <h3 className="mb-1 text-base font-semibold text-white">
              {t("store.metodo.titulo", { title: jogo.title })}
            </h3>
            <p className="mb-4 text-[12px] text-white/40">{t("store.metodo.sub")}</p>
            <div className="flex flex-col gap-2">
              {depotDisponivel && (
                <button
                  onClick={onDepot}
                  className="flex items-center justify-between rounded-xl border border-white/10 px-4 py-3.5 text-left transition-colors hover:border-white/25"
                >
                  <span className="text-[13px] font-medium text-white/90">
                    {t("store.metodo.depot")}
                  </span>
                  <span className="text-[11px] text-white/50">{t("store.metodo.depot_desc")}</span>
                </button>
              )}
              <button
                onClick={() => setEtapa("fonte")}
                className="flex items-center justify-between rounded-xl border border-white/10 px-4 py-3.5 text-left transition-colors hover:border-white/25"
              >
                <span className="text-[13px] font-medium text-white/90">
                  {t("store.metodo.torrent")}
                </span>
                <span className="text-[11px] text-white/50">
                  {t("store.metodo.torrent_fontes", { count: String(opcoes.length) })}
                </span>
              </button>
            </div>
          </>
        )}

        {etapa === "fonte" && (
          <>
            <h3 className="mb-1 text-base font-semibold text-white">{t("store.fonte.titulo")}</h3>
            <p className="mb-4 text-[12px] text-white/40">{t("store.fonte.sub")}</p>
            <div className="flex max-h-[50vh] flex-col gap-2 overflow-y-auto">
              {opcoes.map((o, i) => (
                <button
                  key={o.ref}
                  onClick={() => {
                    setEscolhida(o)
                    setEtapa("pasta")
                  }}
                  className={`flex items-center justify-between gap-3 rounded-xl border px-4 py-3 text-left transition-colors ${
                    i === 0
                      ? "border-[color:var(--accent)] bg-[color-mix(in_srgb,var(--accent)_12%,transparent)]"
                      : "border-white/10 hover:border-white/25"
                  }`}
                >
                  <span className="min-w-0">
                    <span className="block truncate text-[13px] font-medium text-white/90">
                      {o.fonte}
                      {o.http && (
                        <span className="ml-2 rounded bg-white/10 px-1.5 py-0.5 text-[9px] font-bold uppercase text-white/60">
                          HTTP
                        </span>
                      )}
                    </span>
                    <span className="block truncate text-[11px] text-white/40">
                      {o.tituloFonte}
                    </span>
                  </span>
                  <span className="shrink-0 text-[11px] font-semibold text-white/50">
                    {o.fileSize}
                  </span>
                </button>
              ))}
            </div>
          </>
        )}

        {etapa === "pasta" && (
          <>
            <h3 className="mb-1 text-base font-semibold text-white">
              {t("store.torrent.titulo", { title: jogo.title })}
            </h3>
            <p className="mb-4 text-[12px] text-white/40">
              {escolhida?.fonte} · {escolhida?.fileSize} — {t("store.torrent.sub")}
            </p>
            <button
              onClick={escolherPasta}
              className="flex w-full items-center justify-between rounded-xl border border-[color:var(--accent)] bg-[color-mix(in_srgb,var(--accent)_12%,transparent)] px-4 py-3 text-left transition-colors"
            >
              <span className="min-w-0 truncate text-[13px] font-medium text-white/90">
                {pasta.replace(/^\/home\/[^/]+/, "~")}
              </span>
              <span className="ml-3 shrink-0 text-[11px] font-semibold text-white/50">
                {livre !== null ? t("store.gb_livres", { free: livre.toFixed(2) }) : ""}
              </span>
            </button>
            <button
              onClick={() => escolhida && onTorrent(escolhida.magnet, pasta)}
              disabled={!pasta || !escolhida}
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
