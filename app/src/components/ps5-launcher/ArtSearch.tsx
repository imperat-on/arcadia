"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import type { ArtCandidate } from "../../global"
import { useGamepadNav } from "./useGamepadNav"
import { useI18n } from "../../i18n/I18nContext"

interface ArtSearchProps {
  gameId: string
  titulo: string
  kind: "cover" | "hero" | "logo"
  onClose: () => void
  /** Recebe o caminho local já baixado. */
  onPicked: (path: string) => void
}

// Resoluções oferecidas no filtro. A lista fechada que vale é a do
// electron/metadata.js — ele descarta o que não reconhecer.
const RESOLUCOES: Record<string, { valor: string; rotulo: string }[]> = {
  cover: [
    { valor: "600x900", rotulo: "600×900 (PS5)" },
    { valor: "660x930", rotulo: "660×930" },
    { valor: "342x482", rotulo: "342×482" },
    { valor: "920x430", rotulo: "920×430 (deitada)" },
    { valor: "460x215", rotulo: "460×215 (deitada)" },
  ],
  hero: [
    { valor: "1920x620", rotulo: "1920×620 (Full HD)" },
    { valor: "3840x1240", rotulo: "3840×1240 (4K)" },
    { valor: "1600x650", rotulo: "1600×650" },
  ],
  logo: [],
}

const CONSOLE = typeof window !== "undefined" && window.launcherMode !== "desktop"

export function ArtSearch({ gameId, titulo, kind, onClose, onPicked }: ArtSearchProps) {
  const { t } = useI18n()

  const TITULOS = {
    cover: t("editmetadata.buscar_capa"),
    hero: t("editmetadata.buscar_fundo"),
    logo: t("editmetadata.buscar_logo"),
  }

  const ref = useRef<HTMLDivElement>(null)
  useGamepadNav(ref, true, onClose)

  const [termo, setTermo] = useState(titulo)
  const [carregando, setCarregando] = useState(false)
  const [baixando, setBaixando] = useState<string | null>(null)
  const [candidatos, setCandidatos] = useState<ArtCandidate[]>([])
  const [erros, setErros] = useState<string[]>([])
  // Resoluções marcadas. Vazio = o padrão do tipo (definido no Electron).
  const [resolucoes, setResolucoes] = useState<string[]>([])

  const buscar = useCallback(
    async (q: string, dims: string[]) => {
      setCarregando(true)
      setErros([])
      const res = await window.launcherAPI?.searchArt(gameId, q, kind, dims)
      setCandidatos(res?.candidatos ?? [])
      setErros(res?.erros ?? [])
      setCarregando(false)
    },
    [gameId, kind],
  )

  useEffect(() => {
    buscar(titulo, [])
  }, [buscar, titulo])

  // Marcar/desmarcar já refaz a busca: filtro que exige clicar em "Buscar"
  // passa a impressão de estar quebrado.
  const alternarResolucao = (v: string) => {
    const next = resolucoes.includes(v)
      ? resolucoes.filter((r) => r !== v)
      : [...resolucoes, v]
    setResolucoes(next)
    buscar(termo, next)
  }

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose()
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [onClose])

  // Baixa a arte escolhida e devolve o caminho local para o editor.
  const escolher = async (c: ArtCandidate) => {
    setBaixando(c.url)
    const res = await window.launcherAPI?.downloadArt(gameId, kind, c.url)
    setBaixando(null)
    if (res?.ok && res.path) {
      onPicked(res.path)
      onClose()
    } else {
      const errMsg = res?.error ?? t("editmetadata.erro_desconhecido")
      setErros((e) => [...e, t("editmetadata.falha_baixar", { error: errMsg })])
    }
  }

  return (
    <>
      <div className="fixed inset-0 z-[60]" style={{ background: "rgba(0,0,0,0.75)" }} />
      <div
        ref={ref}
        className="gp-scope fixed z-[61] left-1/2 top-1/2 w-[860px] max-h-[86vh] flex flex-col rounded-2xl overflow-hidden"
        style={{
          transform: "translate(-50%, -50%)",
          background: CONSOLE ? "rgba(10,22,54,0.98)" : "var(--sidebar-bg)",
          border: "1px solid rgba(255,255,255,0.14)",
          boxShadow: "0 24px 70px rgba(0,0,0,0.7)",
        }}
        role="dialog"
        aria-label={TITULOS[kind]}
      >
        <div className="px-6 py-4 flex items-center gap-3" style={{ background: "rgba(0,0,0,0.5)" }}>
          <span className="text-white text-[15px] font-semibold tracking-wide uppercase whitespace-nowrap">
            {TITULOS[kind]}
          </span>
          <input
            value={termo}
            onChange={(e) => setTermo(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && buscar(termo, resolucoes)}
            placeholder={t("editmetadata.nome_jogo")}
            className="flex-1 px-3 py-2 rounded-lg text-white text-[14px] outline-none"
            style={{
              background: "rgba(255,255,255,0.07)",
              border: "1px solid rgba(255,255,255,0.12)",
            }}
          />
          <button
            onClick={() => buscar(termo, resolucoes)}
            className="px-4 py-2 rounded-lg text-[14px] font-semibold text-white"
            style={{ background: "var(--accent)" }}
          >
            {t("editmetadata.buscar")}
          </button>
          <button
            onClick={onClose}
            aria-label={t("editmetadata.fechar")}
            className="px-3 py-2 rounded-lg text-[14px] text-white/70 hover:text-white"
            style={{ background: "rgba(255,255,255,0.08)" }}
          >
            {t("editmetadata.fechar")}
          </button>
        </div>

        {/* Filtro de resolução (o SteamGridDB só aceita esta lista) */}
        {RESOLUCOES[kind].length > 0 && (
          <div
            className="px-6 py-3 flex items-center gap-2 flex-wrap"
            style={{ background: "rgba(0,0,0,0.25)" }}
          >
            <span className="text-[11px] font-bold tracking-wider uppercase text-[#8a93a6] mr-1">
              {t("editmetadata.resolucao")}
            </span>
            {RESOLUCOES[kind].map((r) => {
              const on = resolucoes.includes(r.valor)
              return (
                <button
                  key={r.valor}
                  onClick={() => alternarResolucao(r.valor)}
                  className="px-3 py-1 rounded-full text-[12px] font-medium transition-colors"
                  style={{
                    color: on ? "#fff" : "#8a93a6",
                    background: on ? "var(--accent)" : "rgba(255,255,255,0.06)",
                    border: `1px solid ${on ? "transparent" : "rgba(255,255,255,0.10)"}`,
                  }}
                >
                  {r.rotulo}
                </button>
              )
            })}
            {resolucoes.length > 0 && (
              <button
                onClick={() => {
                  setResolucoes([])
                  buscar(termo, [])
                }}
                className="text-[12px] text-[#8a93a6] hover:text-white underline ml-1"
              >
                {t("editmetadata.limpar_filtro")}
              </button>
            )}
          </div>
        )}

        <div className="flex-1 overflow-y-auto p-6">
          {carregando ? (
            <p className="text-center text-[#8a93a6] py-12">{t("editmetadata.procurando")}</p>
          ) : candidatos.length === 0 ? (
            <p className="text-center text-[#8a93a6] py-12">
              {t("editmetadata.nada_encontrado", { termo })}
            </p>
          ) : (
            <div
              className="grid gap-4"
              style={{
                gridTemplateColumns:
                  kind === "cover"
                    ? "repeat(auto-fill, minmax(140px, 1fr))"
                    : "repeat(auto-fill, minmax(260px, 1fr))",
              }}
            >
              {candidatos.map((c) => (
                <button
                  key={c.url}
                  onClick={() => escolher(c)}
                  disabled={Boolean(baixando)}
                  className="relative rounded-lg overflow-hidden text-left transition-transform hover:scale-[1.04] disabled:opacity-50"
                  style={{
                    aspectRatio: kind === "cover" ? "2 / 3" : "16 / 9",
                    background: "rgba(255,255,255,0.06)",
                    border: "1px solid rgba(255,255,255,0.10)",
                  }}
                >
                  <img src={c.thumb} alt="" className="w-full h-full object-cover" loading="lazy" />

                  {/* Etiquetas: fonte e se é animado */}
                  <span
                    className="absolute top-1.5 left-1.5 px-1.5 py-0.5 rounded text-[9px] font-bold text-white"
                    style={{ background: "rgba(0,0,0,0.7)" }}
                  >
                    {c.fonte}
                  </span>
                  {c.animado && (
                    <span
                      className="absolute top-1.5 right-1.5 px-1.5 py-0.5 rounded text-[9px] font-bold text-black"
                      style={{ background: "#ffd166" }}
                    >
                      {t("editmetadata.animado")}
                    </span>
                  )}
                  {baixando === c.url && (
                    <span
                      className="absolute inset-0 flex items-center justify-center text-white text-xs font-semibold"
                      style={{ background: "rgba(0,0,0,0.6)" }}
                    >
                      {t("common.baixando")}
                    </span>
                  )}
                </button>
              ))}
            </div>
          )}

          {/* Fonte que falhou não some calada: vira aviso. */}
          {erros.length > 0 && (
            <div className="mt-6 flex flex-col gap-1">
              {erros.map((e) => (
                <p key={e} className="text-[12px] text-[#ffa07a]">
                  {e}
                </p>
              ))}
            </div>
          )}
        </div>
      </div>
    </>
  )
}
