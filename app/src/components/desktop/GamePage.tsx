"use client"

import { useEffect, useState } from "react"
import type { Game } from "../ps5-launcher/types"
import { fmtBytes, fmtMiB } from "../tamanho"

interface Sysinfo {
  download_size?: number
  disk_size?: number
  version?: string
  req_min?: string
  req_rec?: string
}

// Requisitos vêm como HTML da Steam — vira texto simples.
function stripHtml(s: string) {
  return String(s || "")
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/<\/li>/gi, "\n")
    .replace(/<\/p>/gi, "\n")
    .replace(/<[^>]+>/g, "")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/\n{3,}/g, "\n\n")
    .trim()
}

// Página do jogo (estilo Heroic): abre ao clicar no card. Hero + metadados à
// esquerda, dados de instalação/requisitos à direita.
export function GamePage({
  game: g,
  onClose,
  onJogar,
  onInstalar,
  onImportar,
  onConfig,
}: {
  game: Game
  onClose: () => void
  onJogar: () => void
  onInstalar: () => void
  onImportar: () => void
  onConfig: () => void
}) {
  const instalado = g.installed !== false
  const epic = g.launcher === "epic"
  const [aba, setAba] = useState<"dados" | "requisitos">("dados")
  const [sys, setSys] = useState<Sysinfo | null>(null)
  const [sysBusy, setSysBusy] = useState(true)
  // Fixes (GameBypass/OnlineFix, estilo luatools).
  const [fixes, setFixes] = useState<{ generic?: boolean; online?: boolean } | null>(null)
  const [fixBusy, setFixBusy] = useState("")
  const [fixMsg, setFixMsg] = useState("")

  // Dados reais: tamanhos (legendary, Epic) e requisitos (Steam appdetails).
  useEffect(() => {
    setSysBusy(true)
    window.launcherAPI?.gameSysinfo(g).then((r) => {
      setSys(r?.info || {})
      setSysBusy(false)
    })
    // Fixes só fazem sentido em jogo instalado.
    if (g.installed !== false) {
      const appid = String(g.id).replace(/^steam:/, "")
      window.launcherAPI?.storeCheckFixes(appid).then((r) => {
        if (r?.ok && (r.generic || r.online)) setFixes({ generic: r.generic, online: r.online })
      })
    }
  }, [g.id])

  const aplicarFix = async (type: "generic" | "online") => {
    setFixBusy(type)
    setFixMsg("")
    const appid = String(g.id).replace(/^steam:/, "")
    const { path: installPath } = (await window.launcherAPI?.storeInstallDir(g)) || { path: "" }
    if (!installPath) {
      setFixBusy("")
      setFixMsg("Pasta de instalação não encontrada.")
      return
    }
    const r = await window.launcherAPI?.storeApplyFix({ appid, type, installPath })
    setFixBusy("")
    setFixMsg(r?.ok ? "Fix aplicado!" : r?.error || "Falha ao aplicar o fix")
  }

  const gibBytes = fmtBytes
  const gib = fmtMiB
  const tamDownload = sys?.download_size ? gibBytes(sys.download_size) : gib(g.size)
  const tamInstalado = sys?.disk_size ? gibBytes(sys.disk_size) : instalado ? gib(g.size) : "—"
  const ultimaVez = g.last_played
    ? new Date(g.last_played).toLocaleDateString("pt-BR", { day: "2-digit", month: "2-digit", year: "numeric" })
    : "Nunca"

  return (
    <div className="fixed inset-0 z-[55] flex flex-col bg-black" style={{ animation: "gp-in 0.18s ease-out" }}>
      {/* Fundo: hero desfocado */}
      {g.hero || g.cover ? (
        <img src={g.hero || g.cover} alt="" className="pointer-events-none absolute inset-0 h-full w-full object-cover opacity-25 blur-md" draggable={false} />
      ) : null}
      <div className="pointer-events-none absolute inset-0 bg-black/60" />

      {/* Botão voltar */}
      <button
        onClick={onClose}
        className="absolute left-4 top-4 z-10 flex h-10 w-10 items-center justify-center rounded-full bg-white/[0.08] text-white/80 backdrop-blur-sm transition-colors hover:bg-white/[0.16] hover:text-white"
        title="Voltar"
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="15 18 9 12 15 6" />
        </svg>
      </button>

      <div className="relative z-[1] mx-auto grid h-full w-full max-w-[1400px] flex-1 grid-cols-2 gap-5 overflow-hidden p-5 pt-16">
        {/* Coluna esquerda: arte + info */}
        <div className="flex min-h-0 flex-col overflow-hidden rounded-2xl border border-white/[0.08] bg-[#0d0d10]/90">
          <div className="relative h-[42%] shrink-0 overflow-hidden bg-black">
            {g.hero || g.cover ? (
              <img src={g.hero || g.cover} alt="" className="h-full w-full object-cover" draggable={false} />
            ) : null}
            <div className="absolute inset-0 bg-gradient-to-b from-black/20 via-transparent to-[#0d0d10]" />
            {g.logo ? (
              <img src={g.logo} alt="" className="absolute left-5 top-5 max-h-[64px] max-w-[55%] object-contain object-left drop-shadow-[0_2px_6px_rgba(0,0,0,0.9)]" draggable={false} />
            ) : null}
          </div>

          <div className="flex min-h-0 flex-1 flex-col px-6 py-4">
            <h1 className="truncate text-2xl font-light text-white">{g.title}</h1>
            {(g.developer || g.publisher) && (
              <p className="mt-0.5 text-[13px] italic text-white/50">{g.developer || g.publisher}</p>
            )}
            <p className="mt-3 min-h-0 flex-1 overflow-y-auto text-[13px] leading-relaxed text-white/65">
              {g.description || "Sem descrição."}
            </p>

            <div className="mt-4 shrink-0">
              <p className="flex items-center gap-2 text-[12px] text-white/45">
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                  <circle cx="12" cy="12" r="9" /><polyline points="12 7 12 12 15 14" />
                </svg>
                Jogado pela última vez em: <span className="text-white/70">{ultimaVez}</span>
              </p>
              {!instalado && <p className="mt-1.5 text-[13px] italic" style={{ color: "var(--accent)" }}>Este jogo não está instalado</p>}

              <div className="mt-3 flex gap-2.5">
                {instalado ? (
                  <>
                    <button
                      onClick={onJogar}
                      className="flex items-center gap-2 rounded-lg px-6 py-2.5 text-[13px] font-bold tracking-wide text-black transition-transform hover:scale-[1.03]"
                      style={{ background: "var(--accent)" }}
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z" /></svg>
                      JOGAR
                    </button>
                    <button
                      onClick={onConfig}
                      className="rounded-lg border border-white/20 px-6 py-2.5 text-[13px] font-semibold tracking-wide text-white/80 transition-colors hover:bg-white/[0.06] hover:text-white"
                    >
                      CONFIGURAÇÕES
                    </button>
                  </>
                ) : (
                  <>
                    <button
                      onClick={onInstalar}
                      className="flex items-center gap-2 rounded-lg px-6 py-2.5 text-[13px] font-bold tracking-wide text-black transition-transform hover:scale-[1.03]"
                      style={{ background: "var(--accent)" }}
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
                        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><polyline points="7 10 12 15 17 10" /><line x1="12" y1="15" x2="12" y2="3" />
                      </svg>
                      INSTALAR
                    </button>
                    {epic && (
                      <button
                        onClick={onImportar}
                        className="rounded-lg border border-white/20 px-6 py-2.5 text-[13px] font-semibold tracking-wide text-white/80 transition-colors hover:bg-white/[0.06] hover:text-white"
                      >
                        IMPORTAR JOGO
                      </button>
                    )}
                  </>
                )}
              </div>
            </div>

            {/* Fixes (GameBypass/OnlineFix) quando disponíveis para o jogo */}
            {fixes && (
              <div className="mt-3 shrink-0 rounded-xl border border-white/[0.08] bg-white/[0.03] p-3">
                <div className="mb-2 text-[12px] font-semibold uppercase tracking-wider text-white/50">Fixes disponíveis</div>
                <div className="flex gap-2">
                  {fixes.generic && (
                    <button
                      onClick={() => aplicarFix("generic")}
                      disabled={Boolean(fixBusy)}
                      className="rounded-lg px-3.5 py-1.5 text-[11px] font-bold text-black transition-transform enabled:hover:scale-[1.03] disabled:opacity-50"
                      style={{ background: "var(--accent)" }}
                    >
                      {fixBusy === "generic" ? "Aplicando…" : "GameBypass"}
                    </button>
                  )}
                  {fixes.online && (
                    <button
                      onClick={() => aplicarFix("online")}
                      disabled={Boolean(fixBusy)}
                      className="rounded-lg px-3.5 py-1.5 text-[11px] font-bold text-black transition-transform enabled:hover:scale-[1.03] disabled:opacity-50"
                      style={{ background: "var(--accent)" }}
                    >
                      {fixBusy === "online" ? "Aplicando…" : "OnlineFix"}
                    </button>
                  )}
                </div>
                {fixMsg && <p className="mt-2 text-[11px] text-white/55">{fixMsg}</p>}
              </div>
            )}
          </div>
        </div>

        {/* Coluna direita: dados */}
        <div className="flex min-h-0 flex-col overflow-hidden rounded-2xl border border-white/[0.08] bg-[#0d0d10]/90 p-5">
          <div className="mx-auto mb-5 flex gap-1 rounded-full border border-white/10 bg-white/[0.03] p-1">
            {([
              ["dados", "DADOS DA INSTALAÇÃO"],
              ["requisitos", "REQUISITOS"],
            ] as const).map(([id, label]) => (
              <button
                key={id}
                onClick={() => setAba(id)}
                className={`rounded-full px-4 py-1.5 text-[11px] font-semibold tracking-wider transition-colors ${
                  aba === id ? "bg-white/[0.1] text-white" : "text-white/45 hover:text-white/75"
                }`}
              >
                {label}
              </button>
            ))}
          </div>

          {aba === "dados" ? (
            <div className="flex flex-col gap-4">
              <div className="flex items-center justify-between border-b border-white/[0.06] pb-3">
                <span className="flex items-center gap-2.5 text-[13px] font-medium text-white/85">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><polyline points="7 10 12 15 17 10" /><line x1="12" y1="15" x2="12" y2="3" />
                  </svg>
                  Tamanho do download:
                </span>
                <span className="text-[13px] text-white/70">{sysBusy && !sys ? "…" : tamDownload}</span>
              </div>
              <div className="flex items-center justify-between border-b border-white/[0.06] pb-3">
                <span className="flex items-center gap-2.5 text-[13px] font-medium text-white/85">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                    <rect x="2" y="3" width="20" height="7" rx="2" /><rect x="2" y="14" width="20" height="7" rx="2" />
                  </svg>
                  Tamanho instalado:
                </span>
                <span className="text-[13px] text-white/70">{sysBusy && !sys ? "…" : tamInstalado}</span>
              </div>
              {sys?.version && (
                <div className="flex items-center justify-between border-b border-white/[0.06] pb-3">
                  <span className="text-[13px] font-medium text-white/85">Versão:</span>
                  <span className="text-[13px] text-white/70">{sys.version}</span>
                </div>
              )}
              <div className="flex items-center justify-between border-b border-white/[0.06] pb-3">
                <span className="text-[13px] font-medium text-white/85">Loja:</span>
                <span className="text-[13px] capitalize text-white/70">{g.launcher}</span>
              </div>
            </div>
          ) : (
            <div className="min-h-0 flex-1 overflow-y-auto text-[13px]">
              {sysBusy && !sys ? (
                <p className="text-white/45">Buscando requisitos…</p>
              ) : sys?.req_min || sys?.req_rec ? (
                <div className="flex flex-col gap-4">
                  {sys.req_min ? (
                    <div>
                      <h4 className="mb-1.5 text-[12px] font-semibold uppercase tracking-wider" style={{ color: "var(--accent)" }}>Mínimos</h4>
                      <p className="whitespace-pre-line leading-relaxed text-white/70">{stripHtml(sys.req_min)}</p>
                    </div>
                  ) : null}
                  {sys.req_rec ? (
                    <div>
                      <h4 className="mb-1.5 text-[12px] font-semibold uppercase tracking-wider" style={{ color: "var(--accent)" }}>Recomendados</h4>
                      <p className="whitespace-pre-line leading-relaxed text-white/70">{stripHtml(sys.req_rec)}</p>
                    </div>
                  ) : null}
                  {g.launcher !== "steam" && (
                    <p className="text-[11px] text-white/30">Fonte: página equivalente na Steam.</p>
                  )}
                </div>
              ) : (
                <div className="flex flex-col gap-3">
                  {[
                    ["Gênero", g.genre],
                    ["Ano", g.year],
                    ["Jogadores", g.players],
                    ["Metacritic", g.metacritic != null ? `${g.metacritic}/100` : undefined],
                  ].map(([k, v]) => (
                    <div key={String(k)} className="flex items-center justify-between border-b border-white/[0.06] pb-2.5">
                      <span className="font-medium text-white/85">{k}:</span>
                      <span className="text-white/70">{v != null && v !== "" ? String(v) : "—"}</span>
                    </div>
                  ))}
                  <p className="mt-2 text-[12px] text-white/35">Requisitos de sistema não disponíveis para este jogo.</p>
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      <style>{`
        @keyframes gp-in {
          from { opacity: 0; transform: scale(1.01); }
          to { opacity: 1; transform: scale(1); }
        }
      `}</style>
    </div>
  )
}
